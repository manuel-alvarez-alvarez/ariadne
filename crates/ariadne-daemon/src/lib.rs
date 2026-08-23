//! Daemon internals, exposed as a library so integration tests can exercise
//! the managers directly. The `ariadned` binary is a thin wrapper.

pub mod agents;
pub mod attention;
pub mod bus;
pub mod config;
pub mod gitutil;
pub mod gitwt;
pub mod http;
pub mod launcher;
pub mod logbuf;
pub mod logtail;
pub mod logwatch;
pub mod opencode_plugin;
pub mod scheduler;
pub mod sleep;
pub mod tmux;
