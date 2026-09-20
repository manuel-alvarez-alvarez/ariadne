//! The agent's pipes as the ACP SDK's transport.
//!
//! The daemon spawns the agent, holds it, signals its process group and reaps
//! it; the SDK only ever sees the two pipes. `ByteStreams` takes the
//! `futures` halves, which is what `tokio_util::compat` makes of tokio's.

use agent_client_protocol::ByteStreams;
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
