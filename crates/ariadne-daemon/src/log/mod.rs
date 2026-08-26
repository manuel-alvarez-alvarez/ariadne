//! The two logs the daemon serves.
//!
//! [`console`] is one agent session's terminal: the file tmux `pipe-pane`
//! appends to, read incrementally and woken by the kernel, behind
//! `/v1/sessions/{id}/logs/stream`.
//!
//! [`buffer`] is the daemon's own `tracing` output, kept in a ring in memory,
//! behind `/v1/logs`.

pub mod buffer;
pub mod console;

pub use buffer::LogBuffer;
pub use console::{LogTail, LogWatch, MAX_CHUNK};
