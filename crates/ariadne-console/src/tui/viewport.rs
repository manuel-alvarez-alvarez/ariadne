//! The inline viewport: how it opens, and the backend that makes it open
//! where the terminal cannot say where the cursor is.

use anyhow::Result;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};

/// How tall the inline viewport is. The bottom [`super::Console::pinned_rows`]
/// rows are the status row, the input box and the footer; the rest shows the
/// block still being written, which moves into the scrollback the moment it
/// is finished.
pub(super) const VIEWPORT: u16 = 13;

/// A ratatui backend whose failures `anyhow` can carry: the real terminal's
/// and the test one's alike.
pub trait Screen: Backend<Error: std::error::Error + Send + Sync + 'static> {}

impl<B> Screen for B
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
}

/// Open the inline viewport, from the cursor where the terminal says where
/// that is, and from the bottom row where it does not.
///
/// [`Viewport::Inline`] asks the terminal for the cursor position, and a
/// terminal that never answers — a pseudo-terminal with nothing behind it,
/// for one — costs crossterm's timeout and the error "The cursor position
/// could not be read within a normal duration". The console opens all the
/// same: the cursor is moved to the bottom row, which needs no answer, and
/// the viewport is opened from there. It is the one inline viewport on both
/// paths, never the alternate screen, so the scrollback keeps the transcript
/// either way.
///
/// The terminal is asked once, here, and never again: on both paths the
/// backend answers every later query itself, from where the cursor was last
/// put. ratatui asks after every block it inserts above the viewport and on
/// every resize, and by then the key stream is reading the terminal — and
/// crossterm's answer to a cursor query comes through the same reader the
/// key stream holds, so a query made while keys are read times out and would
/// end the console on its first finished block.
///
/// `fresh` makes a backend, and makes another for the second attempt: the
/// one the failed attempt took cannot be had back.
pub fn open<B: Screen>(fresh: impl Fn() -> B) -> Result<Terminal<Anchored<B>>> {
    let inline = |backend| {
        Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
    };
    if let Ok(terminal) = inline(Anchored::asking(fresh())) {
        return Ok(terminal);
    }
    let mut backend = fresh();
    let bottom = Position {
        x: 0,
        y: backend.size()?.height.saturating_sub(1),
    };
    backend.set_cursor_position(bottom)?;
    Ok(inline(Anchored::at(backend, bottom))?)
}

/// A backend whose cursor position is known without asking the terminal.
///
/// ratatui asks where the cursor is when it opens an inline viewport, when it
/// clears one — which it does after every block it inserts above it — and
/// when the terminal is resized. Anchored to a position, every one of those
/// is answered from here — where the last [`Backend::set_cursor_position`]
/// put it, which is where ratatui's own drawing leaves it. Asking, it puts
/// the one query it is made through to the terminal, and is anchored to the
/// answer from then on.
pub struct Anchored<B> {
    inner: B,
    known: Option<Position>,
}

impl<B> Anchored<B> {
    fn asking(inner: B) -> Self {
        Self { inner, known: None }
    }

    fn at(inner: B, at: Position) -> Self {
        Self {
            inner,
            known: Some(at),
        }
    }
}

impl<B: Backend> Backend for Anchored<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), B::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> Result<(), B::Error> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> Result<Position, B::Error> {
        match self.known {
            Some(at) => Ok(at),
            None => {
                let at = self.inner.get_cursor_position()?;
                self.known = Some(at);
                Ok(at)
            }
        }
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), B::Error> {
        let at = position.into();
        self.inner.set_cursor_position(at)?;
        self.known = Some(at);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), B::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), B::Error> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, B::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, B::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), B::Error> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use futures_util::stream;
    use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Size};
    use serde_json::json;

    use crate::tui::testing::*;
    use crate::tui::*;

    use super::*;

    /// The status row, the input box with its two border rows, and the
    /// footer.
    const PINNED: u16 = 5;

    /// A screen that says where its cursor is `answers` times and never
    /// again: the terminal that answers no query, as ratatui sees it, and the
    /// one that answers the first and none after it.
    struct Mute(TestBackend, usize);

    impl Mute {
        fn new() -> Self {
            Self(TestBackend::new(72, 40), 0)
        }

        /// The terminal once the key stream is reading it: crossterm's
        /// answer to a cursor query comes through the reader the stream
        /// holds, so the query made at the open is answered and every later
        /// one times out.
        fn answering_once() -> Self {
            Self(TestBackend::new(72, 40), 1)
        }
    }

    /// What the test backend cannot fail at, as the error the mute one can.
    fn sure<T>(result: Result<T, Infallible>) -> std::io::Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(never) => match never {},
        }
    }

    impl Backend for Mute {
        type Error = std::io::Error;

        fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            sure(self.0.draw(content))
        }

        fn append_lines(&mut self, n: u16) -> std::io::Result<()> {
            sure(self.0.append_lines(n))
        }

        fn hide_cursor(&mut self) -> std::io::Result<()> {
            sure(self.0.hide_cursor())
        }

        fn show_cursor(&mut self) -> std::io::Result<()> {
            sure(self.0.show_cursor())
        }

        fn get_cursor_position(&mut self) -> std::io::Result<Position> {
            if self.1 > 0 {
                self.1 -= 1;
                return sure(self.0.get_cursor_position());
            }
            Err(std::io::Error::other(
                "The cursor position could not be read within a normal duration",
            ))
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> std::io::Result<()> {
            sure(self.0.set_cursor_position(position))
        }

        fn clear(&mut self) -> std::io::Result<()> {
            sure(self.0.clear())
        }

        fn clear_region(&mut self, clear_type: ClearType) -> std::io::Result<()> {
            sure(self.0.clear_region(clear_type))
        }

        fn size(&self) -> std::io::Result<Size> {
            sure(self.0.size())
        }

        fn window_size(&mut self) -> std::io::Result<WindowSize> {
            sure(self.0.window_size())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            sure(self.0.flush())
        }
    }

    #[test]
    fn the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read() {
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());

        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = rows(terminal.backend().inner.0.buffer());
        assert_eq!(
            row_of(&shown, " author · claude:opus · running"),
            Some(40 - usize::from(PINNED)),
            "the status line is drawn, with the pane on the bottom rows: {shown}"
        );
        assert!(
            console.close(&mut terminal).is_ok(),
            "and closing, which asks where the cursor is again, still works"
        );
    }

    #[test]
    fn a_finished_block_reaches_the_scrollback_when_the_cursor_position_cannot_be_read() {
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            event(
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = rows(terminal.backend().inner.0.buffer());
        let prompt = row_of(&shown, "> first").expect(&shown);
        assert!(
            prompt < 40 - usize::from(VIEWPORT),
            "the finished prompt is above the pane, in the scrollback: {shown}"
        );
    }

    /// The terminal answered where its cursor was at the open, and answers
    /// nothing after: ratatui asks again after every block it inserts above
    /// the viewport, and the console answers that itself rather than end on
    /// its first finished block.
    #[test]
    fn a_finished_block_reaches_the_scrollback_when_only_the_first_cursor_query_is_answered() {
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            event(
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console
            .commit(&mut terminal)
            .expect("the block is inserted without asking the terminal again");
        terminal.draw(|frame| console.render(frame)).unwrap();

        let shown = rows(terminal.backend().inner.0.buffer());
        let prompt = row_of(&shown, "> first").expect(&shown);
        assert!(
            prompt < 40 - usize::from(VIEWPORT),
            "the finished prompt is above the pane, in the scrollback: {shown}"
        );
        assert!(
            console.close(&mut terminal).is_ok(),
            "and closing, which asks where the cursor is again, still works"
        );
    }

    /// The fallback viewport — the terminal that answers no cursor query —
    /// runs the loop and closes like the inline one.
    #[tokio::test]
    async fn the_console_runs_and_closes_on_the_fallback_viewport() {
        let mut stub = Stub::new(Vec::new());
        let source = stub.source();
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());
        let ctrl_c = TermEvent::Key(ctrl('c'));
        let keys = stream::iter(vec![Ok(ctrl_c.clone()), Ok(ctrl_c)]).chain(stream::pending());

        drive(
            &mut terminal,
            source,
            &mut stub,
            Box::pin(keys),
            &mut console,
        )
        .await
        .unwrap();

        assert!(console.close(&mut terminal).is_ok());
    }
}
