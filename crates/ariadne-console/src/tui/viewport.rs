//! The inline viewport: how it opens, how it stays as tall as what it holds,
//! and the backend that makes both work where the terminal cannot say where
//! the cursor is.

use anyhow::Result;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::Color;
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::Console;

/// How tall the viewport opens: one row, since nothing is known of what it
/// will hold. The first [`fit`] makes it the height of the pane.
const OPENING: u16 = 1;

/// How tall the viewport of a test's own terminal is: a fixed one, for a test
/// that draws the console and never runs the loop that fits the pane.
#[cfg(test)]
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
/// put. ratatui asks after every block it inserts above the viewport, on
/// every resize and each time [`fit`] opens the viewport again at another
/// height, and by then the key stream is reading the terminal — and
/// crossterm's answer to a cursor query comes through the same reader the
/// key stream holds, so a query made while keys are read times out and would
/// end the console on its first finished block.
///
/// `fresh` makes a backend, and makes another for the second attempt: the
/// one the failed attempt took cannot be had back.
pub fn open<B: Screen>(fresh: impl Fn() -> B) -> Result<Terminal<Anchored<B>>> {
    if let Ok(terminal) = inline(Anchored::asking(fresh()), OPENING) {
        return Ok(terminal);
    }
    let mut backend = fresh();
    let bottom = Position {
        x: 0,
        y: backend.size()?.height.saturating_sub(1),
    };
    backend.set_cursor_position(bottom)?;
    Ok(inline(Anchored::at(backend, bottom), OPENING)?)
}

fn inline<B: Backend>(backend: B, height: u16) -> Result<Terminal<B>, B::Error> {
    Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )
}

impl Console {
    /// How tall the pane is on a terminal of `size`, once the blocks before
    /// `from` are in the scrollback: the lines of the blocks still in it,
    /// and the pinned rows under them. The terminal's height is the most it
    /// takes, and a pending picker has all of that but the pinned rows to
    /// fold into.
    pub(super) fn rows(&self, from: usize, size: Size) -> u16 {
        let pinned = self.pinned_rows(size.width);
        let room = size.height.saturating_sub(pinned);
        let live = self.live_lines(from, size.width, room).len();
        pinned
            .saturating_add(u16::try_from(live).unwrap_or(u16::MAX))
            .min(size.height)
            .max(1)
    }
}

/// Make the viewport `wanted` rows tall, where it is not: taller as the block
/// being written grows, shorter as blocks leave for the scrollback.
///
/// ratatui fixes the height of an inline viewport when it opens it, and has
/// nothing that changes it after. So the viewport is opened again, over the
/// same backend, from the row the old one began on: ratatui makes the room a
/// taller one lacks by scrolling the terminal, which moves what is above it
/// into the scrollback and deletes none of it, and a shorter one leaves the
/// rows under it blank for the blocks [`Console::commit`] inserts next, so
/// they land where they were being written. The old viewport is wiped first
/// and the next draw paints the new one whole. That happens only here, where
/// the height changes, and no other draw repaints more than the cells that
/// changed.
///
/// A terminal resized is the same case. A viewport that ran off the bottom of
/// a shorter terminal is opened again as many rows further up, and one on a
/// narrower terminal on the top row, the rows above it scrolled into the
/// scrollback: the terminal has wrapped every line it held.
///
/// The backend is [`Anchored`], so opening the viewport again asks the
/// terminal nothing.
///
/// ratatui takes the backend to open a viewport and drops it where the open
/// fails, so a failure here leaves the terminal with none
/// ([`Anchored::lost`]). The error is the terminal's own and ends the loop;
/// the terminal left behind does nothing, and is fitted no more.
pub(super) fn fit<B: Screen>(terminal: &mut Terminal<Anchored<B>>, wanted: u16) -> Result<()> {
    if terminal.backend().lost() {
        return Ok(());
    }
    let size = terminal.size()?;
    let area = terminal.get_frame().area();
    let wanted = wanted.min(size.height);
    if area.height == wanted && area.width == size.width && area.bottom() <= size.height {
        return Ok(());
    }
    let backend = terminal.backend_mut();
    let top = if size.width < area.width {
        // An erase of the whole screen would take the blocks above the pane
        // with it, where the terminal keeps no copy of what it erases. The
        // pane's rows are erased, and the rows above it are scrolled into
        // the scrollback.
        let above = area.y.min(size.height);
        backend.set_cursor_position(Position { x: 0, y: above })?;
        backend.clear_region(ClearType::AfterCursor)?;
        backend.set_cursor_position(Position {
            x: 0,
            y: size.height.saturating_sub(1),
        })?;
        backend.append_lines(above)?;
        0
    } else {
        // A terminal made shorter keeps its bottom rows, the cursor's among
        // them, so the viewport is as many rows further up as ran off.
        area.y
            .saturating_sub(area.bottom().saturating_sub(size.height))
    };
    let at = Position { x: 0, y: top };
    backend.set_cursor_position(at)?;
    backend.clear_region(ClearType::AfterCursor)?;
    let backend = Anchored {
        inner: backend.inner.take(),
        known: Some(at),
    };
    *terminal = inline(backend, wanted)?;
    Ok(())
}

/// Clear the whole terminal, its scrollback too where the backend can, and
/// open the viewport again on the top row: the start of a redraw of the
/// whole transcript ([`Console::redraw`]).
pub(super) fn restart<B: Screen>(terminal: &mut Terminal<Anchored<B>>) -> Result<()> {
    if terminal.backend().lost() {
        return Ok(());
    }
    let backend = terminal.backend_mut();
    backend.clear()?;
    backend.set_cursor_position(Position::ORIGIN)?;
    let backend = Anchored {
        inner: backend.inner.take(),
        known: Some(Position::ORIGIN),
    };
    *terminal = inline(backend, OPENING)?;
    Ok(())
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
    /// `None` in the terminal [`fit`] took the backend from: one dropped at
    /// once, or the one left behind where the open that followed failed.
    inner: Option<B>,
    known: Option<Position>,
}

impl<B> Anchored<B> {
    fn asking(inner: B) -> Self {
        Self {
            inner: Some(inner),
            known: None,
        }
    }

    fn at(inner: B, at: Position) -> Self {
        Self {
            inner: Some(inner),
            known: Some(at),
        }
    }

    /// Whether the backend is gone: [`fit`] took it to open the viewport
    /// again, and the open failed and dropped it with its error. Nothing
    /// more can be written, so what is left to do is nothing, not a panic.
    pub(super) fn lost(&self) -> bool {
        self.inner.is_none()
    }

    /// The backend under this one, for a test to read its screen.
    #[cfg(test)]
    pub(super) fn under(&self) -> &B {
        self.inner.as_ref().expect("the test's backend is there")
    }

    /// The backend under this one, for a test to resize it.
    #[cfg(test)]
    pub(super) fn under_mut(&mut self) -> &mut B {
        self.inner.as_mut().expect("the test's backend is there")
    }
}

/// Every call goes to the backend under this one but the cursor query. With
/// the backend [`Anchored::lost`], every call does nothing and says so: the
/// terminal [`fit`] took it from shows its cursor as it is dropped, and a
/// host closes the console whatever ended it.
impl<B: Backend> Backend for Anchored<B> {
    type Error = B::Error;

    /// The blank cells that end a row are erased, not written as spaces. A
    /// terminal keeps a written space as text, so a terminal made narrower
    /// would wrap each row of the scrollback into a second row of spaces.
    fn draw<'a, I>(&mut self, content: I) -> Result<(), B::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(());
        };
        let width = inner.size()?.width;
        let cells: Vec<_> = content.collect();
        let mut written = Vec::with_capacity(cells.len());
        let mut erased = Vec::new();
        for row in cells.chunk_by(|a, b| a.1 == b.1) {
            // The blank cells, one after the other, up to the last column:
            // the cells the diff leaves out are unchanged, so only a run that
            // reaches the edge can be erased to the end of the row.
            let mut start = row.len();
            let mut edge = width;
            while start > 0 && row[start - 1].0 + 1 == edge && blank(row[start - 1].2) {
                start -= 1;
                edge = row[start].0;
            }
            written.extend_from_slice(&row[..start]);
            if let Some(&(x, y, _)) = row.get(start) {
                erased.push(Position { x, y });
            }
        }
        inner.draw(written.into_iter())?;
        for at in erased {
            inner.set_cursor_position(at)?;
            inner.clear_region(ClearType::UntilNewLine)?;
        }
        Ok(())
    }

    fn append_lines(&mut self, n: u16) -> Result<(), B::Error> {
        self.inner
            .as_mut()
            .map_or(Ok(()), |inner| inner.append_lines(n))
    }

    fn hide_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.as_mut().map_or(Ok(()), Backend::hide_cursor)
    }

    fn show_cursor(&mut self) -> Result<(), B::Error> {
        self.inner.as_mut().map_or(Ok(()), Backend::show_cursor)
    }

    fn get_cursor_position(&mut self) -> Result<Position, B::Error> {
        match (self.known, self.inner.as_mut()) {
            (Some(at), _) => Ok(at),
            (None, Some(inner)) => {
                let at = inner.get_cursor_position()?;
                self.known = Some(at);
                Ok(at)
            }
            (None, None) => Ok(Position::ORIGIN),
        }
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), B::Error> {
        let at = position.into();
        if let Some(inner) = self.inner.as_mut() {
            inner.set_cursor_position(at)?;
        }
        self.known = Some(at);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), B::Error> {
        self.inner.as_mut().map_or(Ok(()), Backend::clear)
    }

    /// An erase from the top-left corner to the end of the screen is made
    /// one row at a time: tmux takes that erase for a clear of the whole
    /// screen, and keeps a copy of the rows it erased in its scrollback.
    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), B::Error> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(());
        };
        if clear_type == ClearType::AfterCursor && self.known == Some(Position::ORIGIN) {
            for y in 0..inner.size()?.height {
                inner.set_cursor_position(Position { x: 0, y })?;
                inner.clear_region(ClearType::UntilNewLine)?;
            }
            return inner.set_cursor_position(Position::ORIGIN);
        }
        inner.clear_region(clear_type)
    }

    fn size(&self) -> Result<Size, B::Error> {
        self.inner
            .as_ref()
            .map_or(Ok(Size::default()), Backend::size)
    }

    fn window_size(&mut self) -> Result<WindowSize, B::Error> {
        self.inner.as_mut().map_or(
            Ok(WindowSize {
                columns_rows: Size::default(),
                pixels: Size::default(),
            }),
            Backend::window_size,
        )
    }

    fn flush(&mut self) -> Result<(), B::Error> {
        self.inner.as_mut().map_or(Ok(()), Backend::flush)
    }
}

/// A cell an erase leaves as it is: a space in no colour and no style.
fn blank(cell: &Cell) -> bool {
    cell.symbol() == " "
        && cell.fg == Color::Reset
        && cell.bg == Color::Reset
        && cell.modifier.is_empty()
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use futures_util::stream;
    use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
    use ratatui::buffer::Cell;
    use ratatui::layout::{Position, Rect, Size};
    use serde_json::json;

    use crate::ansi::{AnsiBackend, Window};

    use crate::tui::testing::*;
    use crate::tui::*;

    use super::*;

    /// The status row, the input box with its two border rows, and the
    /// footer.
    const PINNED: u16 = 5;

    /// A screen that says where its cursor is `answers` times and never
    /// again: the terminal that answers no query, as ratatui sees it, and the
    /// one that answers the first and none after it. The flag is a terminal
    /// that went away: it takes no more line feeds, which is what ratatui
    /// makes room for a viewport with.
    struct Mute(TestBackend, usize, bool);

    impl Mute {
        fn new() -> Self {
            Self(TestBackend::new(72, 40), 0, false)
        }

        /// The terminal once the key stream is reading it: crossterm's
        /// answer to a cursor query comes through the reader the stream
        /// holds, so the query made at the open is answered and every later
        /// one times out.
        fn answering_once() -> Self {
            Self(TestBackend::new(72, 40), 1, false)
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
            if self.2 {
                return Err(std::io::Error::other("the terminal went away"));
            }
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

    /// A prompt, as the daemon confirms one typed at the console.
    fn prompt(text: &str) -> AgentEventDto {
        event(
            "user_prompt_submit",
            text,
            json!({"text": text, "source": "console"}),
        )
    }

    /// One chunk of the agent's text: a list item, which is one line of the
    /// block however the chunks before it ended. The numbers are padded, so
    /// no row's name is part of another's.
    fn row(n: usize) -> AgentEventDto {
        let text = format!("- row-{n:03}\n");
        event(
            "agent_message_chunk",
            &format!("row-{n:03}"),
            json!({"text": text}),
        )
    }

    /// A console on a screen that a first turn has filled, so the pane is on
    /// the bottom rows as it is in a session under way: a block of `filler`
    /// lines in the scrollback, and the prompt of the next turn.
    fn under_way<B: Screen>(terminal: &mut Terminal<Anchored<B>>) -> Console {
        under_way_at(terminal, chrono::Utc::now())
    }

    /// [`under_way`], with the wall clock behind the filled turn's start
    /// fixed to `now`: two consoles built with the same `now` start their
    /// clock at the same instant, so comparing their renders never turns on
    /// which one read the wall clock first.
    fn under_way_at<B: Screen>(
        terminal: &mut Terminal<Anchored<B>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Console {
        let filler: String = (1..=60).map(|n| format!("- filler-{n:03}\n")).collect();
        let mut console = Console::new(header());
        console.snapshot_at(
            &[
                event("agent_message", "filler", json!({"text": filler})),
                prompt("go"),
            ],
            now,
        );
        console.show(terminal).unwrap();
        console
    }

    /// The rows the pane is on.
    fn pane_of<B: Backend>(terminal: &mut Terminal<B>) -> Rect {
        terminal.get_frame().area()
    }

    /// Everything the terminal has shown: its scrollback, then its screen.
    fn history(terminal: &Terminal<Anchored<TestBackend>>) -> String {
        let backend = terminal.backend().under();
        format!("{}\n{}", rows(backend.scrollback()), rows(backend.buffer()))
    }

    /// Between turns every block is in the scrollback, the agent's last and
    /// the stop under it too: nothing is being written into either.
    #[test]
    fn an_idle_pane_is_as_tall_as_its_pinned_rows() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.show(&mut terminal).unwrap();
        assert_eq!(pane_of(&mut terminal).height, PINNED, "with no block yet");

        console.apply(&prompt("go"));
        console.apply(&event("agent_message", "done", json!({"text": "done"})));
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
        console.show(&mut terminal).unwrap();

        assert_eq!(pane_of(&mut terminal).height, PINNED, "after a turn");
        let shown = shown(&terminal);
        let top: Vec<&str> = shown.lines().take(5).collect();
        assert_eq!(
            top[..4],
            ["▌❯ go", "", "● done", ""],
            "each block of the turn is committed, a separator under each: {shown}"
        );
        assert!(
            top[4].starts_with(" author · claude:opus · idle"),
            "and the status row is right under the last separator: {shown}"
        );
    }

    #[test]
    fn no_blank_row_lies_between_the_scrollback_and_the_pane_but_the_block_separator() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("first"),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        let top: Vec<&str> = shown.lines().take(4).collect();
        assert_eq!(
            top[..3],
            ["▌❯ first", "", "● second"],
            "the committed prompt, the separator, the block in the pane: {shown}"
        );
        assert!(
            top[3].starts_with(" author · claude:opus"),
            "and the status line right under it: {shown}"
        );
    }

    #[test]
    fn each_line_of_a_block_in_work_shows_while_it_is_written() {
        let mut terminal = pane();
        let mut console = under_way(&mut terminal);

        for written in 1..=30 {
            console.apply(&row(written));
            console.show(&mut terminal).unwrap();

            let shown = shown(&terminal);
            let missing: Vec<usize> = (1..=written)
                .filter(|n| !shown.contains(&format!("row-{n:03}")))
                .collect();
            assert!(
                missing.is_empty(),
                "with {written} lines written, lines {missing:?} are off the pane: {shown}"
            );
        }
        assert_eq!(pane_of(&mut terminal).height, 30 + PINNED);
    }

    /// The end of the turn that wrote `rows` lines: the whole the daemon
    /// stores for the run of chunks (021), which closes the block, then the
    /// stop.
    fn turn_ends(console: &mut Console, rows: usize) {
        let whole: String = (1..=rows).map(|n| format!("- row-{n:03}\n")).collect();
        console.apply(&event("agent_message", "whole", json!({"text": whole})));
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
    }

    /// A block of 100 lines in work, each line shown as it is written.
    fn a_long_block(terminal: &mut Terminal<Anchored<TestBackend>>) -> Console {
        let mut console = under_way(terminal);
        for written in 1..=100 {
            console.apply(&row(written));
            console.show(terminal).unwrap();
        }
        console
    }

    #[test]
    fn a_block_longer_than_the_terminal_takes_every_row_and_reaches_the_scrollback_once() {
        let mut terminal = pane();
        let mut console = a_long_block(&mut terminal);

        let tail = shown(&terminal);
        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40));
        assert!(
            tail.contains("row-100") && !tail.contains("row-065"),
            "the pane shows the last lines of the block: {tail}"
        );

        turn_ends(&mut console, 100);
        console.show(&mut terminal).unwrap();

        let history = history(&terminal);
        let found: Vec<&str> = history
            .lines()
            .filter_map(|line| line.find("row-").map(|at| &line[at..]))
            .collect();
        let expected: Vec<String> = (1..=100).map(|n| format!("row-{n:03}")).collect();
        assert_eq!(found, expected, "each line once, in order: {history}");
        assert_eq!(
            history.matches("filler-").count(),
            60,
            "and the lines committed before it are neither lost nor said again: {history}"
        );
    }

    #[test]
    fn the_pane_shrinks_back_once_its_long_block_is_committed() {
        let mut terminal = pane();
        let mut console = a_long_block(&mut terminal);

        turn_ends(&mut console, 100);
        console.show(&mut terminal).unwrap();

        assert_eq!(
            pane_of(&mut terminal),
            Rect::new(0, 40 - PINNED, 72, PINNED),
            "the idle height: the pinned rows, on the bottom rows"
        );
        let shown = shown(&terminal);
        let above: Vec<&str> = shown
            .lines()
            .skip(40 - usize::from(PINNED) - 2)
            .take(2)
            .collect();
        assert_eq!(
            above,
            ["  • row-100", ""],
            "the block ends one separator row above the pane: {shown}"
        );
    }

    /// The terminal answered where its cursor was at the open, and answers
    /// nothing after. The pane grows, shrinks and follows a resize without
    /// asking again: each of them opens the viewport again, and ratatui asks
    /// the backend where the cursor is every time it does.
    #[test]
    fn no_cursor_query_follows_the_open_through_a_grow_a_shrink_and_a_resize() {
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = under_way(&mut terminal);

        for written in 1..=50 {
            console.apply(&row(written));
            console.show(&mut terminal).expect("the pane grows");
        }
        console.apply(&prompt("next"));
        console.show(&mut terminal).expect("the pane shrinks");
        assert_eq!(pane_of(&mut terminal).height, 1 + PINNED);

        for (width, height) in [(72, 20), (72, 50), (40, 50), (90, 30)] {
            terminal.backend_mut().under_mut().0.resize(width, height);
            console.show(&mut terminal).expect("the pane is resized");
            console.apply(&row(50 + usize::from(height)));
            console.show(&mut terminal).expect("and grows after it");
        }
        assert!(
            console.close(&mut terminal).is_ok(),
            "and closing, which asks where the cursor is again, still works"
        );
    }

    #[test]
    fn a_resize_to_a_shorter_terminal_keeps_the_whole_pane_on_the_screen() {
        let mut terminal = pane();
        let mut console = under_way(&mut terminal);
        for written in 1..=10 {
            console.apply(&row(written));
        }
        console.show(&mut terminal).unwrap();
        assert_eq!(
            pane_of(&mut terminal).bottom(),
            40,
            "the pane is on the bottom rows"
        );

        terminal.backend_mut().under_mut().resize(72, 20);
        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        let bottom = shown.lines().last().unwrap_or_default();
        assert!(
            bottom.starts_with(" enter send"),
            "the footer is on the last row of the shorter terminal: {shown}"
        );
        assert!(
            (1..=10).all(|n| shown.contains(&format!("row-{n:03}"))),
            "with the whole block in work over it: {shown}"
        );

        // A terminal shorter than the pane gets the pane's bottom rows.
        terminal.backend_mut().under_mut().resize(72, 8);
        console.show(&mut terminal).unwrap();

        let shown = rows(terminal.backend().under().buffer());
        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 8));
        assert!(
            shown.contains("row-010") && shown.contains("author · claude:opus"),
            "{shown}"
        );
        assert!(
            shown
                .lines()
                .last()
                .unwrap_or_default()
                .starts_with(" enter send"),
            "{shown}"
        );
    }

    /// The bytes [`AnsiBackend`] wrote, readable while the terminal owns it.
    #[derive(Clone, Default)]
    struct Tap(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Tap {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Tap {
        /// What a terminal of 72 by 40 shows once it has read the bytes.
        fn screen(&self) -> String {
            let mut parser = vt100::Parser::new(40, 72, 0);
            parser.process(&self.0.lock().unwrap());
            let shown = parser.screen().contents();
            let shown: Vec<&str> = shown.lines().map(str::trim_end).collect();
            shown.join("\n")
        }
    }

    /// The daemon's host draws the pane as the CLI's does: a terminal that
    /// reads the bytes shows the rows ratatui's own test backend holds. Both
    /// renders fold every event under the same `now`, so the turn's clock
    /// reads the same on each side regardless of when the test itself runs.
    #[tokio::test(start_paused = true)]
    async fn the_ansi_backend_shows_the_rows_the_test_backend_shows_after_a_grow_and_a_shrink() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let now = chrono::Utc::now();
        let mut ansi = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut test = pane();
        let mut on_ansi = under_way_at(&mut ansi, now);
        let mut on_test = under_way_at(&mut test, now);

        for written in 1..=50 {
            on_ansi.apply_at(&row(written), now);
            on_ansi.show(&mut ansi).unwrap();
            on_test.apply_at(&row(written), now);
            on_test.show(&mut test).unwrap();
        }
        let grown = shown(&test);
        assert!(
            grown.starts_with("  • row-016"),
            "the pane grew to every row: {grown}"
        );
        assert_eq!(tap.screen(), grown.trim_end(), "after the grow");

        on_ansi.apply_at(&prompt("next"), now);
        on_ansi.show(&mut ansi).unwrap();
        on_test.apply_at(&prompt("next"), now);
        on_test.show(&mut test).unwrap();
        let shrunk = shown(&test);
        assert_eq!(
            pane_of(&mut test).height,
            1 + PINNED,
            "the pane shrank: {shrunk}"
        );
        assert_eq!(tap.screen(), shrunk.trim_end(), "after the shrink");
    }

    /// Drive a console on a terminal of 72 by 40 over the ANSI backend,
    /// resize it to 60 columns once the transcript is drawn, and leave it:
    /// the bytes it wrote, all told.
    async fn resized(console: Console) -> Vec<u8> {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = console;
        let mut events = Vec::new();
        for n in 0..2 {
            events.push(prompt(&format!("go {n}")));
            events.push(event(
                "agent_message",
                &format!("done {n}"),
                json!({"text": format!("done {n}")}),
            ));
            events.push(event(
                "stop",
                &format!("stop {n}"),
                json!({"stop_reason": "end_turn"}),
            ));
        }
        let mut stub = Stub::new(events);
        let source = stub.source();
        let (typing, typed) = tokio::sync::mpsc::unbounded_channel();
        let keys = stream::unfold(typed, |mut typed| async move {
            typed.recv().await.map(|key| (Ok(key), typed))
        });
        let script = async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            window.set(60, 40);
            typing.send(TermEvent::Resize(60, 40)).unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
            typing.send(TermEvent::Key(ctrl('c'))).unwrap();
            typing.send(TermEvent::Key(ctrl('c'))).unwrap();
        };
        let (outcome, ()) = tokio::join!(
            drive(
                &mut terminal,
                source,
                &mut stub,
                Box::pin(keys),
                &mut console
            ),
            script
        );
        outcome.unwrap();
        tap.0.lock().unwrap().clone()
    }

    /// A terminal made narrower re-wraps its rows, so the pane's rows are no
    /// longer where they were drawn. The desktop app's console draws the
    /// whole transcript again once the resize settles: from a cleared
    /// screen and scrollback, each block once and one status row.
    #[tokio::test(start_paused = true)]
    async fn a_console_that_redraws_whole_draws_the_transcript_again_once_a_resize_settles() {
        let written = resized(Console::new(header()).redraws_whole_on_resize()).await;

        let clear = b"\x1b[2J\x1b[3J";
        let at = written
            .windows(clear.len())
            .rposition(|bytes| bytes == clear)
            .expect("the resize cleared the terminal, scrollback included");
        let mut emulator = vt100::Parser::new(40, 60, 0);
        emulator.process(&written[at..]);
        let shown = emulator.screen().contents();
        for once in [
            "╭",
            "go 0",
            "done 0",
            "go 1",
            "done 1",
            "claude:opus · idle",
        ] {
            assert_eq!(shown.matches(once).count(), 1, "{once} once: {shown}");
        }
        assert!(
            shown.contains(&"─".repeat(60)) && !shown.contains(&"─".repeat(61)),
            "drawn at the new width: {shown}"
        );
    }

    /// The CLI's terminal holds the user's shell above the console: a
    /// resize fits the pane in place and clears nothing above it.
    #[tokio::test(start_paused = true)]
    async fn a_console_that_fits_in_place_never_clears_the_scrollback() {
        let written = resized(Console::new(header())).await;

        let written = String::from_utf8_lossy(&written);
        assert!(!written.contains("\x1b[3J"), "{written:?}");
    }

    /// A terminal keeps a space it was sent as text: a row that ends in
    /// written spaces wraps into a second row of blanks where the terminal
    /// is made narrower, so every row of the scrollback would be doubled.
    #[test]
    fn the_blank_end_of_a_row_is_erased_and_not_written_as_spaces() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("go"),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);

        console.show(&mut terminal).unwrap();

        let written = String::from_utf8_lossy(&tap.0.lock().unwrap()).to_string();
        assert!(
            !written.contains(&" ".repeat(20)),
            "no row is padded with spaces: {written:?}"
        );
        let shown = tap.screen();
        assert!(
            shown.starts_with("▌❯ go\n\n● done\n\n author · claude:opus · idle"),
            "and the screen is what it was: {shown}"
        );
    }

    /// A narrower terminal opens the pane again at the top of the screen.
    /// The blocks that were on the screen above it go to the scrollback, not
    /// out of the terminal.
    #[test]
    fn a_narrower_terminal_keeps_the_blocks_that_were_on_the_screen() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("go"),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);
        console.show(&mut terminal).unwrap();
        let mut emulator = vt100::Parser::new(40, 72, 100);
        emulator.process(&std::mem::take(&mut *tap.0.lock().unwrap()));

        window.set(60, 40);
        emulator.screen_mut().set_size(40, 60);
        console.show(&mut terminal).unwrap();
        emulator.process(&tap.0.lock().unwrap());

        emulator.screen_mut().set_scrollback(usize::MAX);
        let shown: Vec<String> = emulator
            .screen()
            .rows(0, 60)
            .map(|row| row.trim_end().to_string())
            .collect();
        assert_eq!(
            shown[..5],
            ["▌❯ go", "", "● done", "", " author · claude:opus · idle"],
            "the blocks, then the pane at the new width: {shown:#?}"
        );
    }

    /// tmux takes an erase from the top-left corner to the end of the screen
    /// for a clear of the whole screen, and keeps a copy of what it erased
    /// in its scrollback: the pane, each time it is opened again on the top
    /// row, which is where a narrower terminal puts it.
    #[test]
    fn a_pane_on_the_top_row_is_erased_row_by_row_and_never_from_the_corner() {
        let tap = Tap::default();
        let window = Window::new(72, 24);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = under_way(&mut terminal);
        window.set(60, 24);
        console.show(&mut terminal).unwrap();
        assert_eq!(pane_of(&mut terminal).y, 0);
        tap.0.lock().unwrap().clear();

        for text in ["a", "b", "c"] {
            type_into(&mut console, text);
            console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
            console.show(&mut terminal).unwrap();
        }

        let written = String::from_utf8_lossy(&tap.0.lock().unwrap()).to_string();
        assert!(
            !written.contains("\x1b[1;1H\x1b[J"),
            "no erase starts at the corner: {written:?}"
        );
        assert_eq!(
            pane_of(&mut terminal).y,
            0,
            "the pane is still on the top row"
        );
    }

    /// The way out wipes the pane, and the shell comes back on its top row,
    /// under the last block: not on the row the input box had the cursor on.
    #[test]
    fn the_shell_comes_back_on_the_row_under_the_last_block() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("go"),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);
        console.show(&mut terminal).unwrap();

        console.close(&mut terminal).unwrap();

        let shown = shown(&terminal);
        assert_eq!(shown.trim_end(), "▌❯ go\n\n● done", "{shown}");
        assert_eq!(
            terminal
                .backend_mut()
                .under_mut()
                .get_cursor_position()
                .unwrap(),
            Position { x: 0, y: 4 },
            "the cursor is under the separator of the last block: {shown}"
        );
    }

    /// The picker folds its diff to the room it is given (rule 27), and the
    /// room is the terminal's height less the pinned rows, not a fixed pane's.
    #[test]
    fn a_pending_question_on_a_terminal_of_24_rows_shows_its_question_and_every_option() {
        let mut terminal = super::open(|| TestBackend::new(72, 24)).unwrap();
        let mut console = Console::new(header());
        let old: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        let new: String = (1..=30).map(|n| format!("row {n}\n")).collect();
        console.apply(&event(
            "permission_request",
            "Permission requested for Edit",
            json!({"tool_name": "Edit",
                   "acp": {"toolCallId": "edit", "kind": "edit",
                           "rawInput": {"file_path": "src/lib.rs"},
                           "content": [{"type": "diff", "path": "src/lib.rs",
                                        "oldText": old, "newText": new}]},
                   "options": [{"optionId": "once", "name": "Allow once"},
                               {"optionId": "always", "name": "Allow always"},
                               {"optionId": "no", "name": "Reject"}]}),
        ));

        console.show(&mut terminal).unwrap();

        let shown = shown(&terminal);
        assert!(
            shown.starts_with("─ permission ─") && shown.contains("\nEdit\n○ ✎ src/lib.rs\n"),
            "the question is the first row of the terminal: {shown}"
        );
        for option in ["❯ 1. Allow once", "2. Allow always", "3. Reject"] {
            assert!(shown.contains(option), "{option} is on the screen: {shown}");
        }
        assert!(
            shown.contains("-line 9\n") && shown.contains("more lines"),
            "the diff has the rows a fixed pane had not: {shown}"
        );
    }

    #[test]
    fn the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read() {
        let mut terminal = super::open(Mute::new).unwrap();
        let mut console = Console::new(header());

        console.show(&mut terminal).unwrap();

        let shown = rows(terminal.backend().under().0.buffer());
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
            prompt("first"),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console.show(&mut terminal).unwrap();

        let shown = rows(terminal.backend().under().0.buffer());
        let first = row_of(&shown, "❯ first").expect(&shown);
        assert_eq!(
            first,
            40 - usize::from(PINNED) - 3,
            "the finished prompt is above the pane, its separator and the block in work: {shown}"
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
            prompt("first"),
            event("agent_message", "second", json!({"text": "second"})),
        ]);

        console
            .show(&mut terminal)
            .expect("the block is inserted without asking the terminal again");

        let shown = rows(terminal.backend().under().0.buffer());
        assert_eq!(
            row_of(&shown, "❯ first"),
            Some(0),
            "the finished prompt is above the pane, in the scrollback: {shown}"
        );
        assert!(
            console.close(&mut terminal).is_ok(),
            "and closing, which asks where the cursor is again, still works"
        );
    }

    /// ratatui takes the backend to open the viewport again and drops it where
    /// that fails. The loop ends on the terminal's own error, and the close
    /// both hosts make after it, whatever ended the loop, does nothing.
    #[test]
    fn a_pane_that_cannot_be_opened_again_ends_on_the_error_and_closes_without_a_panic() {
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = under_way(&mut terminal);

        terminal.backend_mut().under_mut().2 = true;
        console.apply(&row(1));
        console.apply(&row(2));
        let error = console
            .show(&mut terminal)
            .expect_err("the pane cannot grow on a terminal that went away");

        assert!(
            error.to_string().contains("the terminal went away"),
            "{error}"
        );
        assert!(
            console.show(&mut terminal).is_ok(),
            "a later turn does nothing"
        );
        assert!(
            console.close(&mut terminal).is_ok(),
            "and neither does the close"
        );
        drop(terminal);
    }

    /// The loop hands that error to the host, which closes the console after
    /// it as after any other end.
    #[tokio::test]
    async fn the_loop_returns_the_error_of_a_pane_that_cannot_be_opened_again() {
        let mut stub = Stub::new(Vec::new());
        let source = stub.source();
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = Console::new(header());
        terminal.backend_mut().under_mut().2 = true;

        let outcome = drive(
            &mut terminal,
            source,
            &mut stub,
            Box::pin(stream::pending()),
            &mut console,
        )
        .await;

        let error = outcome.expect_err("the first fit of the pane fails");
        assert!(
            error.to_string().contains("the terminal went away"),
            "{error}"
        );
        assert!(console.close(&mut terminal).is_ok());
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
