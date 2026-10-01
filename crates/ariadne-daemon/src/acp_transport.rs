//! The agent's pipes as the ACP SDK's transport.
//!
//! The daemon spawns the agent, holds it, signals its process group and reaps
//! it; the SDK only ever sees the two pipes. `ByteStreams` takes the
//! `futures` halves, which is what `tokio_util::compat` makes of tokio's.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use agent_client_protocol::ByteStreams;
use tokio::io::AsyncWrite;
use tokio::process::{ChildStdin, ChildStdout};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

/// The transport for one spawned agent: what the daemon writes to it, and
/// what it reads back.
pub(crate) fn pipes(
    stdin: ChildStdin,
    stdout: ChildStdout,
) -> ByteStreams<Compat<ChildStdin>, Compat<ChildStdout>> {
    ByteStreams::new(stdin.compat_write(), stdout.compat())
}

/// [`pipes`], with a witness of every `session/prompt` written whole to the
/// agent's stdin.
///
/// The SDK says when a request is answered, never when it was written. A
/// claimed agent message needs the second: a prompt the agent never got
/// gives its claim back, and one it got keeps it (018).
pub(crate) fn witnessed_pipes(
    stdin: ChildStdin,
    stdout: ChildStdout,
    witness: PromptWrites,
) -> ByteStreams<Compat<Witnessed<ChildStdin>>, Compat<ChildStdout>> {
    let stdin = Witnessed {
        inner: stdin,
        line: Vec::new(),
        witness,
    };
    ByteStreams::new(stdin.compat_write(), stdout.compat())
}

/// Whether a `session/prompt` went out whole since the driver last asked.
///
/// One turn runs at a time, so the driver resets it right before it sends a
/// prompt, and the next `session/prompt` line written is that prompt's.
#[derive(Clone, Default)]
pub(crate) struct PromptWrites(Arc<AtomicBool>);

impl PromptWrites {
    /// Forget the prompts before this one.
    pub(crate) fn reset(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    /// Whether a `session/prompt` was written whole since the reset.
    pub(crate) fn written(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The agent's stdin, watched line by line. A line counts once the pipe has
/// taken its last byte: the agent can read it from then on, even if it never
/// does.
pub(crate) struct Witnessed<W> {
    inner: W,
    /// The bytes of the line being written, up to its newline.
    line: Vec<u8>,
    witness: PromptWrites,
}

impl<W> Witnessed<W> {
    fn took(&mut self, mut bytes: &[u8]) {
        while let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
            self.line.extend_from_slice(&bytes[..end]);
            if is_prompt_request(&self.line) {
                self.witness.0.store(true, Ordering::SeqCst);
            }
            self.line.clear();
            bytes = &bytes[end + 1..];
        }
        self.line.extend_from_slice(bytes);
    }
}

/// Whether one JSON-RPC line is a `session/prompt` request.
fn is_prompt_request(line: &[u8]) -> bool {
    // Most lines are not prompts, and a prompt is long: parse only a line
    // that names the method at all.
    if !line.windows(14).any(|window| window == b"session/prompt") {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(line).is_ok_and(|message| {
        message.get("method").and_then(serde_json::Value::as_str) == Some("session/prompt")
            && message.get("id").is_some()
    })
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Witnessed<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let written = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(taken)) = written {
            self.took(&buf[..taken]);
        }
        written
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::AsyncWriteExt;

    /// A prompt counts once its newline is taken, however the writes split
    /// it, and no other request counts.
    #[tokio::test]
    async fn a_prompt_counts_once_its_whole_line_is_written() {
        let witness = PromptWrites::default();
        let mut stdin = Witnessed {
            inner: Vec::new(),
            line: Vec::new(),
            witness: witness.clone(),
        };
        stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"session/cancel\"}\n")
            .await
            .unwrap();
        assert!(!witness.written(), "a cancel is no prompt");

        stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"2\",\"method\":\"session/pro")
            .await
            .unwrap();
        assert!(!witness.written(), "half a line is not written");
        stdin.write_all(b"mpt\"}\n").await.unwrap();
        assert!(witness.written());

        witness.reset();
        assert!(!witness.written(), "a reset forgets the prompt before");
    }
}
