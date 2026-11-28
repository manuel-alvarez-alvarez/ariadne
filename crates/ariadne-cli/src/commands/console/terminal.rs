//! The process terminal as `ariadne attach` takes it: whether there is one
//! on both ends, and [`Held`], which gives back what the console took from
//! it on every way out.

use std::io::IsTerminal;

use anyhow::Result;

/// Whether the full-screen console can be drawn at all.
///
/// Both ends have to be a terminal: the keys come from one and the screen
/// is drawn on the other, and a console with either of them redirected is a
/// script's, which reads the plain line protocol instead.
pub(super) fn interactive(stdin: bool, stdout: bool) -> bool {
    stdin && stdout
}

/// What the console takes from the terminal on the way in, and gives back on
/// the way out.
///
/// A trait so that the giving back can be proven: a console that left raw mode
/// on hands the shell back a terminal that echoes nothing.
pub(super) trait Terminals {
    fn enter(&mut self) -> Result<()>;
    fn leave(&mut self);
}

/// The terminal, held for as long as the console runs.
///
/// Giving it back is a `Drop` rather than a line at the end of the function,
/// so it happens on every way out: the normal one, an error, a panic, and the
/// Ctrl-C that is a key here rather than a signal.
pub(super) struct Held<T: Terminals>(T);

impl<T: Terminals> Held<T> {
    pub(super) fn take(terminal: T) -> Result<Self> {
        let mut held = Self(terminal);
        held.0.enter()?;
        Ok(held)
    }
}

impl<T: Terminals> Drop for Held<T> {
    fn drop(&mut self) {
        self.0.leave();
    }
}

/// Whether this process is attached to a terminal on both ends.
pub(super) fn on_a_terminal() -> bool {
    interactive(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_terminal_on_both_ends_gets_the_full_screen_console() {
        assert!(interactive(true, true));
        assert!(!interactive(false, true));
        assert!(!interactive(true, false));
        assert!(!interactive(false, false));
    }
}
