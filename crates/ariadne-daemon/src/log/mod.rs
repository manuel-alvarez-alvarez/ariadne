//! The log the daemon serves: [`buffer`], the daemon's own `tracing` output,
//! kept in a ring in memory, behind `/v1/logs`.

pub mod buffer;

pub use buffer::LogBuffer;
