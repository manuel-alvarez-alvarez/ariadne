//! Shared newline-delimited JSON-RPC transport for ACP agent processes.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Mutex, MutexGuard};

/// Handles notifications and agent-to-client requests received while the
/// transport waits for a response or serves an idle session.
pub(crate) trait Incoming {
    async fn handle(&mut self, message: Value) -> Result<Option<Value>>;
}

/// The one ACP wire implementation used by live sessions and discovery.
pub(crate) struct RpcTransport {
    reader: tokio::io::Lines<BufReader<ChildStdout>>,
    /// Shared with every [`Outbound`] handed out, so a notification can go
    /// down the pipe while a request of this transport is still in flight.
    writer: Arc<Mutex<BufWriter<ChildStdin>>>,
    next_id: u64,
}

/// A handle that writes notifications to the agent from outside the
/// transport's own request loop — what `session/cancel` needs, since the
/// `session/prompt` it interrupts is still waiting for its response.
#[derive(Clone)]
pub(crate) struct Outbound {
    writer: Arc<Mutex<BufWriter<ChildStdin>>>,
}

/// The agent's stdin, held: nothing else — no request, no reply — goes down
/// it until this is dropped, so a caller can check the condition its
/// notification depends on and send it before that condition can move.
pub(crate) struct Pipe<'a>(MutexGuard<'a, BufWriter<ChildStdin>>);

impl Outbound {
    pub(crate) async fn lock(&self) -> Pipe<'_> {
        Pipe(self.writer.lock().await)
    }
}

impl Pipe<'_> {
    pub(crate) async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        write(
            &mut self.0,
            &json!({"jsonrpc": "2.0", "method": method, "params": params}),
        )
        .await
    }
}

impl RpcTransport {
    pub(crate) fn new(stdout: ChildStdout, stdin: ChildStdin) -> Self {
        Self {
            reader: BufReader::new(stdout).lines(),
            writer: Arc::new(Mutex::new(BufWriter::new(stdin))),
            next_id: 1,
        }
    }

    /// The notification path into this transport's agent.
    pub(crate) fn outbound(&self) -> Outbound {
        Outbound {
            writer: self.writer.clone(),
        }
    }

    pub(crate) async fn request<H: Incoming>(
        &mut self,
        method: &str,
        params: Value,
        incoming: &mut H,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let message = self
                .read()
                .await?
                .ok_or_else(|| anyhow!("ACP agent closed stdout during {method}"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    bail!("ACP {method} failed: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow!("ACP {method} response has no result"));
            }
            if let Some(response) = incoming.handle(message).await? {
                self.write(&response).await?;
            }
        }
    }

    /// Receive and handle one unsolicited message, or report closed stdout.
    pub(crate) async fn receive<H: Incoming>(&mut self, incoming: &mut H) -> Result<bool> {
        let Some(message) = self.read().await? else {
            return Ok(false);
        };
        if let Some(response) = incoming.handle(message).await? {
            self.write(&response).await?;
        }
        Ok(true)
    }

    async fn read(&mut self) -> Result<Option<Value>> {
        let Some(line) = self
            .reader
            .next_line()
            .await
            .context("reading from the ACP agent")?
        else {
            return Ok(None);
        };
        serde_json::from_str(&line)
            .with_context(|| format!("reading ACP message `{line}`"))
            .map(Some)
    }

    async fn write(&self, message: &Value) -> Result<()> {
        let mut writer = self.writer.lock().await;
        write(&mut writer, message).await
    }
}

async fn write(writer: &mut BufWriter<ChildStdin>, message: &Value) -> Result<()> {
    let mut line = serde_json::to_vec(message)?;
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .context("writing to the ACP agent")?;
    writer.flush().await.context("flushing ACP request")
}
