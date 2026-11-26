//! A session's trajectory: its events folded into typed turns (040).
//!
//! [`Fold`] is the pure part: one event in, the next state and the ops that
//! say what changed out, with no store and no clock. [`read`] picks the
//! stored events a fold opens on, so that an open reads a bounded count of
//! payloads however long the session ran. The HTTP handlers and the hub that
//! shares one fold between the subscribers of a session are in
//! `crate::http::trajectory`.

mod fold;
pub(crate) mod read;

pub(crate) use fold::{Fold, session_state};
