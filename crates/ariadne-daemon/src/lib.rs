//! Daemon internals, exposed as a library so integration tests can exercise
//! the managers directly. The `ariadned` binary is a thin wrapper.

pub mod acp;
mod acp_calls;
pub mod acp_discovery;
mod acp_schema;
pub mod acp_sessions;
mod acp_transport;
pub mod agents;
pub mod attention;
pub mod branch;
pub mod bus;
pub mod checkpoint;
pub mod config;
pub mod gitwt;
pub mod http;
pub mod knowledge;
pub mod launcher;
pub mod log;
pub mod resource;
pub mod scheduler;
pub(crate) mod sleep;
pub mod timeouts;
pub mod transcript;
