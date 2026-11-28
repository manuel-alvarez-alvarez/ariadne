//! The console of an agent session, for every host that shows one.
//!
//! A session's trajectory (040) is its typed turns, and a line typed at the
//! console becomes the next prompt. [`trajectory`] holds those turns on the
//! client side and prints them as plain text. [`app`] draws them as turns on
//! the whole screen. [`markdown`] draws an agent's markdown as styled
//! lines, and [`theme`] holds the styles and glyphs the screen draws with.
//!
//! Nothing here dials a daemon or owns a process terminal. The loop takes a
//! stream of frames and a host for what the console sends, and a ratatui
//! terminal to draw on: the CLI hands it the daemon's HTTP stream and its own
//! terminal, and the daemon hosts it in process on [`ansi`], a backend that
//! writes terminal bytes to a socket and asks no terminal anything.

pub mod ansi;
pub mod app;
mod diff;
mod fields;
pub mod markdown;
mod since;
#[cfg(test)]
mod testing;
pub mod theme;
pub mod trajectory;
mod wrap;

pub use ansi::{AnsiBackend, Window};
pub use since::Since;
