//! A session's trajectory on the client side: the typed turns the daemon
//! folds (040), and nothing that draws on a terminal.
//!
//! [`View`] holds a snapshot and applies each update to it, so a client keeps
//! the turns as the daemon keeps them. [`Printer`] writes turns and steps as
//! plain text, which is what `ariadne session logs` and a piped `ariadne
//! attach` print.

mod print;
mod view;

pub(crate) use print::activity;
pub use print::{Printer, kind, settled, text};
pub use view::{Gap, Touched, View, step_at, step_id, step_turn};

#[cfg(test)]
pub(crate) mod fixtures;
