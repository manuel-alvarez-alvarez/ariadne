//! Test helpers the console's modules share: the bytes a backend writes, and
//! a terminal emulator to read them back with.

use std::num::NonZeroU16;
use std::sync::{Arc, Mutex};

use ratatui::layout::Position;

use crate::ansi::Window;

/// Bytes written by an ANSI backend while its terminal owns it.
#[derive(Clone, Default)]
pub(crate) struct Tap(pub(crate) Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Tap {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A terminal emulator of `rows` by `cols`, which keeps `back` rows of
/// scrollback.
pub(crate) fn emulator(rows: u16, cols: u16, back: usize) -> vt100::Parser {
    let (rows, cols) = size(rows, cols);
    vt100::Parser::new(rows, cols, back)
}

/// Resize `emulator` to `rows` by `cols`.
pub(crate) fn resize(emulator: &mut vt100::Parser, rows: u16, cols: u16) {
    let (rows, cols) = size(rows, cols);
    emulator.screen_mut().set_size(rows, cols);
}

fn size(rows: u16, cols: u16) -> (NonZeroU16, NonZeroU16) {
    let nonzero = |n| NonZeroU16::new(n).expect("a terminal has rows and columns");
    (nonzero(rows), nonzero(cols))
}

impl Tap {
    /// Read the terminal rows at `window`'s size and scrollback position.
    pub(crate) fn screen_at(&self, window: &Window, back: usize) -> Vec<String> {
        let size = window.size();
        let mut parser = emulator(size.height, size.width, back);
        parser.process(&self.0.lock().unwrap());
        parser.screen_mut().set_scrollback(back);
        parser
            .screen()
            .contents()
            .lines()
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    pub(crate) fn cursor(&self, window: &Window) -> Position {
        let size = window.size();
        let mut parser = emulator(size.height, size.width, 0);
        parser.process(&self.0.lock().unwrap());
        let (y, x) = parser.screen().cursor_position();
        Position { x, y }
    }
}
