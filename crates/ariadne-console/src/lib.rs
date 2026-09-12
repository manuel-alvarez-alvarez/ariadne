//! The console of an agent session, for every host that shows one.
//!
//! An agent's recorded events are its transcript, and a line typed at the
//! console becomes the next prompt. [`transcript`] is the typed model of
//! those events, folded into the blocks a person reads, and [`markdown`]
//! draws an agent's markdown as styled lines. The inline pane itself — the
//! scrolling transcript, the status line and the input box, and the loop
//! that drives them — is [`tui`].
//!
//! Nothing here dials a daemon or owns a process terminal. The loop takes a
//! stream of frames and a sink for what the console sends, and a ratatui
//! terminal to draw on: the CLI hands it the daemon's HTTP stream and its own
//! terminal, and the daemon hosts it in process on [`ansi`], a backend that
//! writes terminal bytes to a socket and asks no terminal anything.

pub mod ansi;
pub mod markdown;
pub mod transcript;
pub mod tui;

pub use ansi::{AnsiBackend, Window};
pub use tui::{Action, Anchored, Console, Frame, Header, Screen, Sink, drive, open};
