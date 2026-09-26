//! The inline viewport: how it opens, how it takes the whole terminal, and
//! the backend that makes both work where the terminal cannot say where the
//! cursor is.

use std::ops::Range;

use anyhow::Result;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Rect, Size};
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
/// put. ratatui asks on every resize, each time [`fit`] opens the viewport
/// again and when the close clears it, and by then the key stream is
/// reading the terminal — and crossterm's answer to a cursor query comes
/// through the same reader the key stream holds, so a query made while keys
/// are read times out and would end the console.
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
    /// How tall the pane is on a terminal of `size`: the terminal's height,
    /// always (008). The pinned rows are its last rows, so the input box is
    /// on the bottom rows of the screen whatever the transcript holds, and
    /// the live area above them is everything else: the room a pending
    /// picker folds into is the terminal's height less the pinned rows.
    pub(super) fn rows(&self, size: Size) -> u16 {
        size.height.max(1)
    }
}

/// Open the pane again at `wanted` rows on the top row of the screen, where
/// it is not there already: as tall as the terminal, which is how it is
/// drawn, and as tall as what is left of it at the close.
///
/// ratatui fixes the height of an inline viewport when it opens it, and has
/// nothing that changes it after. So the viewport is opened again over the
/// same backend. The pane's own rows are erased and the rows above it are
/// scrolled into the scrollback, where the terminal keeps them: the banner
/// and the blocks the pane was opened under are not erased, and neither is
/// the shell above them. The next draw paints the new pane whole. That
/// happens here alone — at the open, on a resize and at the close — and no
/// other draw repaints more than the cells that changed: a finished block
/// leaves the pane through a scrolling region instead.
///
/// A terminal resized is the same case. The pane owns the screen, so a
/// terminal made shorter, taller, narrower or wider has every row of the
/// pane erased and the pane drawn again at the new size.
///
/// The backend is [`Anchored`], so opening the viewport again asks the
/// terminal nothing.
///
/// ratatui takes the backend to open a viewport and drops it where the open
/// fails, and this takes it before it writes anything, so a failure here
/// leaves the terminal with none ([`Anchored::lost`]). The error is the
/// terminal's own and ends the loop; the terminal left behind does nothing,
/// and is fitted no more.
pub(super) fn fit<B: Screen>(terminal: &mut Terminal<Anchored<B>>, wanted: u16) -> Result<()> {
    if terminal.backend().lost() {
        return Ok(());
    }
    let size = terminal.size()?;
    let area = terminal.get_frame().area();
    let wanted = wanted.min(size.height).max(1);
    if area == Rect::new(0, 0, size.width, wanted) {
        return Ok(());
    }
    let taken = terminal.backend_mut();
    let mut backend = Anchored {
        inner: taken.inner.take(),
        known: taken.known,
    };
    // An erase of the whole screen would take the blocks above the pane with
    // it, where the terminal keeps no copy of what it erases. The pane's
    // rows are erased, and the rows above it are scrolled into the
    // scrollback, which is where the pane on the top row leaves them.
    let above = area.y.min(size.height);
    backend.set_cursor_position(Position { x: 0, y: above })?;
    backend.clear_region(ClearType::AfterCursor)?;
    if above > 0 {
        backend.set_cursor_position(Position {
            x: 0,
            y: size.height.saturating_sub(1),
        })?;
        backend.append_lines(above)?;
        backend.set_cursor_position(Position::ORIGIN)?;
        backend.clear_region(ClearType::AfterCursor)?;
    }
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
/// clears one and when the terminal is resized. Anchored to a position,
/// every one of those
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

    /// `inner` under the pane's backend, its cursor on the first cell: how
    /// a test of another module opens an inline viewport of its own size on
    /// a backend, as [`open`] does for the pane.
    #[cfg(test)]
    pub(crate) fn over(inner: B) -> Self {
        Self::at(inner, Position::ORIGIN)
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

impl<B: Backend> Anchored<B> {
    /// Put the top row in the scrollback and leave every other row where it
    /// is: the one-row region of [`Backend::scroll_region_up`].
    fn scroll_top_row(&mut self) -> Result<(), B::Error> {
        let height = self.size()?.height;
        self.set_cursor_position(Position {
            x: 0,
            y: height.saturating_sub(1),
        })?;
        self.append_lines(1)?;
        match self.inner.as_mut() {
            Some(inner) if height > 1 => inner.scroll_region_down(0..height, 1),
            _ => Ok(()),
        }
    }

    /// Blank one row: a region of one row that no row moves into.
    fn erase_row(&mut self, y: u16) -> Result<(), B::Error> {
        self.set_cursor_position(Position { x: 0, y })?;
        self.inner
            .as_mut()
            .map_or(Ok(()), |inner| inner.clear_region(ClearType::CurrentLine))
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

    /// The scrolling region that takes a finished block into the scrollback
    /// ([`Console::commit`]): ratatui borrows the pane's top row, draws the
    /// block's line over it and scrolls that row away, rather than paint the
    /// pane again.
    ///
    /// That row is a region of one row, and no terminal takes one: `DECSTBM`
    /// wants its top row above its bottom one, and tmux, xterm and xterm.js
    /// drop a request for one row, then scroll the whole screen — so every
    /// row of the pane would move up and stay there. A region of one row on
    /// the top row is scrolled by the line feed on the bottom row instead,
    /// which scrolls the whole screen up and puts the top row in the
    /// scrollback, and the whole screen is scrolled down one row after,
    /// which puts every other row back and leaves the top row blank. A
    /// region of one row further down takes no row from anywhere, and is
    /// erased.
    fn scroll_region_up(&mut self, region: Range<u16>, lines: u16) -> Result<(), B::Error> {
        if self.inner.is_none() || lines == 0 || region.is_empty() {
            return Ok(());
        }
        match (region.start, region.end - region.start) {
            (0, 1) => self.scroll_top_row(),
            (row, 1) => self.erase_row(row),
            _ => self
                .inner
                .as_mut()
                .map_or(Ok(()), |inner| inner.scroll_region_up(region, lines)),
        }
    }

    fn scroll_region_down(&mut self, region: Range<u16>, lines: u16) -> Result<(), B::Error> {
        if self.inner.is_none() || lines == 0 || region.is_empty() {
            return Ok(());
        }
        match region.end - region.start {
            1 => self.erase_row(region.start),
            _ => self
                .inner
                .as_mut()
                .map_or(Ok(()), |inner| inner.scroll_region_down(region, lines)),
        }
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

        fn scroll_region_up(
            &mut self,
            region: std::ops::Range<u16>,
            lines: u16,
        ) -> std::io::Result<()> {
            sure(self.0.scroll_region_up(region, lines))
        }

        fn scroll_region_down(
            &mut self,
            region: std::ops::Range<u16>,
            lines: u16,
        ) -> std::io::Result<()> {
            sure(self.0.scroll_region_down(region, lines))
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

    /// Everything the terminal has shown, row by row: its scrollback, then
    /// every row of its screen, the blank ones too.
    fn history(terminal: &Terminal<Anchored<TestBackend>>) -> String {
        let backend = terminal.backend().under();
        format!("{}\n{}", rows(backend.scrollback()), rows(backend.buffer()))
    }

    /// The rows of a screen `rows` read, every one of them.
    fn lines(shown: &str) -> Vec<&str> {
        shown.split('\n').collect()
    }

    /// The last five rows of `screen` are the pinned ones, in their order:
    /// the status row, the box between its two rules, and the footer.
    fn assert_pinned(screen: &[&str], footer: &str, what: &str) {
        let at = screen.len() - usize::from(PINNED);
        let rule = "─".repeat(screen[at + 1].chars().count().max(1));
        assert!(
            screen[at].starts_with(" author · "),
            "{what}: the status row is the fifth row from the bottom: {screen:#?}"
        );
        assert!(
            screen[at + 1].starts_with('─') && screen[at + 1] == rule,
            "{what}: the box's upper rule is under it: {screen:#?}"
        );
        assert!(
            screen[at + 2].starts_with("❯ "),
            "{what}: the box is under its rule: {screen:#?}"
        );
        assert!(
            screen[at + 3].starts_with('─'),
            "{what}: the box's lower rule is under it: {screen:#?}"
        );
        assert!(
            screen[at + 4].starts_with(footer),
            "{what}: the footer is the last row: {screen:#?}"
        );
    }

    /// Between turns every block is in the scrollback, the agent's last and
    /// the stop under it too: the pane is the whole terminal all the same,
    /// its rows over the pinned ones blank.
    #[test]
    fn an_idle_pane_takes_the_whole_terminal_with_its_pinned_rows_last() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.show(&mut terminal).unwrap();
        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40), "at first");
        let screen = rows(terminal.backend().under().buffer());
        assert_pinned(&lines(&screen), " enter send", "with no block yet");

        console.apply(&prompt("go"));
        console.apply(&event("agent_message", "done", json!({"text": "done"})));
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
        console.show(&mut terminal).unwrap();

        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40), "after it");
        let scrollback = rows(terminal.backend().under().scrollback());
        assert_eq!(
            scrollback, "▌❯ go\n\n● done\n",
            "each block of the turn is in the scrollback, a separator under each"
        );
        let screen = rows(terminal.backend().under().buffer());
        let screen = lines(&screen);
        assert!(
            screen[..35].iter().all(|row| row.is_empty()),
            "the rows over the pinned ones are blank: {screen:#?}"
        );
        assert_pinned(&screen, " enter send", "after a turn of two lines");
    }

    /// The live area is bottom-aligned: the block in work ends on the row
    /// over the status row, and the rows over it are blank.
    #[test]
    fn the_block_in_work_is_drawn_right_over_the_status_row() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("first"),
            event("agent_message_chunk", "second", json!({"text": "second"})),
        ]);

        console.show(&mut terminal).unwrap();

        let scrollback = rows(terminal.backend().under().scrollback());
        assert_eq!(scrollback, "▌❯ first\n", "the finished prompt");
        let screen = rows(terminal.backend().under().buffer());
        let screen = lines(&screen);
        assert_eq!(
            screen[..35].iter().filter(|row| !row.is_empty()).count(),
            1,
            "one row of the live area is drawn: {screen:#?}"
        );
        assert_eq!(screen[34], "● second", "and it is the last: {screen:#?}");
        assert_pinned(&screen, " enter send", "under the block in work");
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
        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40));
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

        let tail = rows(terminal.backend().under().buffer());
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

    /// A screen full of transcript and a transcript of two lines leave the
    /// box on the same rows: the pane never shrinks to its content.
    #[test]
    fn the_box_stays_on_the_bottom_rows_once_a_long_block_is_committed() {
        let mut terminal = pane();
        let mut console = a_long_block(&mut terminal);

        turn_ends(&mut console, 100);
        console.show(&mut terminal).unwrap();

        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40));
        let screen = rows(terminal.backend().under().buffer());
        let screen = lines(&screen);
        assert!(
            screen[..35].iter().all(|row| row.is_empty()),
            "the block is in the scrollback, not on the screen: {screen:#?}"
        );
        assert_pinned(&screen, " enter send", "after a block of 100 lines");
    }

    /// The terminal answered where its cursor was at the open, and answers
    /// nothing after. The pane takes the terminal, writes a turn and follows
    /// a resize without asking again: each resize opens the viewport again,
    /// and ratatui asks the backend where the cursor is every time it does.
    #[test]
    fn no_cursor_query_follows_the_open_through_a_turn_and_a_resize() {
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = under_way(&mut terminal);

        for written in 1..=50 {
            console.apply(&row(written));
            console.show(&mut terminal).expect("the block is written");
        }
        console.apply(&prompt("next"));
        console.show(&mut terminal).expect("the block is committed");
        assert_eq!(pane_of(&mut terminal), Rect::new(0, 0, 72, 40));

        for (width, height) in [(72, 20), (72, 50), (40, 50), (90, 30)] {
            terminal.backend_mut().under_mut().0.resize(width, height);
            console.show(&mut terminal).expect("the pane is resized");
            console.apply(&row(50 + usize::from(height)));
            console
                .show(&mut terminal)
                .expect("and written in after it");
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
        /// Every row a terminal of 72 by 40 shows once it has read the
        /// bytes, the blank ones too.
        fn screen(&self) -> Vec<String> {
            let mut parser = vt100::Parser::new(40, 72, 0);
            parser.process(&self.0.lock().unwrap());
            parser
                .screen()
                .rows(0, 72)
                .map(|row| row.trim_end().to_string())
                .collect()
        }

        /// The bytes written since the last take, as text.
        fn take(&self) -> String {
            String::from_utf8_lossy(&std::mem::take(&mut *self.0.lock().unwrap())).to_string()
        }
    }

    /// Everything `emulator` has shown, row by row: its scrollback, then its
    /// screen.
    fn everything(emulator: &mut vt100::Parser) -> Vec<String> {
        let (height, width) = emulator.screen().size();
        emulator.screen_mut().set_scrollback(usize::MAX);
        // A page of the scrollback at a time: scrolled back by `back` rows,
        // the screen's first row is the scrollback's row `back` from its end.
        let mut back = emulator.screen().scrollback();
        let mut shown = Vec::new();
        while back > 0 {
            emulator.screen_mut().set_scrollback(back);
            let page = back.min(usize::from(height));
            shown.extend(
                emulator
                    .screen()
                    .rows(0, width)
                    .take(page)
                    .map(|row| row.trim_end().to_string()),
            );
            back -= page;
        }
        emulator.screen_mut().set_scrollback(0);
        shown.extend(
            emulator
                .screen()
                .rows(0, width)
                .map(|row| row.trim_end().to_string()),
        );
        shown
    }

    /// The rows `A` to `D` on a terminal of 8 by 4, through the pane's own
    /// backend over the ANSI backend and over ratatui's test backend, which
    /// is what says how a region scrolls: one call made on each, then what a
    /// terminal that read the bytes shows and holds in its scrollback, and
    /// what the test backend holds.
    fn one_row(region: Range<u16>, up: bool) -> (Vec<String>, Vec<String>, Vec<String>) {
        let tap = Tap::default();
        let mut ansi = Anchored::at(
            AnsiBackend::new(tap.clone(), Window::new(8, 4)),
            Position::ORIGIN,
        );
        let mut test = Anchored::at(TestBackend::new(8, 4), Position::ORIGIN);
        let cells: Vec<Cell> = ["A", "B", "C", "D"]
            .iter()
            .map(|symbol| {
                let mut cell = Cell::default();
                cell.set_symbol(symbol);
                cell
            })
            .collect();
        let rows = || (0u16..).zip(&cells).map(|(y, cell)| (0, y, cell));
        ansi.draw(rows()).unwrap();
        test.draw(rows()).unwrap();

        if up {
            ansi.scroll_region_up(region.clone(), 1).unwrap();
            test.scroll_region_up(region, 1).unwrap();
        } else {
            ansi.scroll_region_down(region.clone(), 1).unwrap();
            test.scroll_region_down(region, 1).unwrap();
        }

        let mut emulator = vt100::Parser::new(4, 8, 10);
        emulator.process(&tap.0.lock().unwrap());
        let shown = everything(&mut emulator);
        let (scrollback, screen) = shown.split_at(shown.len() - 4);
        let expected: Vec<String> = (0..4)
            .map(|y| test.under().buffer()[(0, y)].symbol().trim().to_string())
            .collect();
        (screen.to_vec(), expected, scrollback.to_vec())
    }

    /// The row ratatui borrows above a pane as tall as the screen is a
    /// region of one row, which no terminal takes as a region. That row
    /// goes into the scrollback, and every other row stays where it was.
    #[test]
    fn a_region_of_one_row_at_the_top_scrolls_that_row_alone_into_the_scrollback() {
        let (screen, expected, scrollback) = one_row(0..1, true);

        assert_eq!(screen, ["", "B", "C", "D"]);
        assert_eq!(screen, expected, "as the test backend has it");
        assert_eq!(scrollback, ["A"], "the top row is in the scrollback");
    }

    /// A region of one row below the top takes no row from anywhere: up or
    /// down, the row is blank after it and the others stay.
    #[test]
    fn a_region_of_one_row_below_the_top_is_erased() {
        for up in [true, false] {
            let (screen, expected, scrollback) = one_row(2..3, up);

            assert_eq!(screen, ["A", "B", "", "D"], "up: {up}");
            assert_eq!(screen, expected, "up: {up}");
            assert!(scrollback.is_empty(), "up: {up}");
        }
    }

    /// The daemon's host draws the pane as the CLI's does: a terminal that
    /// reads the bytes shows the rows ratatui's own test backend holds. Both
    /// renders fold every event under the same `now`, so the turn's clock
    /// reads the same on each side regardless of when the test itself runs.
    #[tokio::test(start_paused = true)]
    async fn the_ansi_backend_shows_the_rows_the_test_backend_shows_through_a_turn() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let now = chrono::Utc::now();
        let mut ansi = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut test = pane();
        let mut on_ansi = under_way_at(&mut ansi, now);
        let mut on_test = under_way_at(&mut test, now);
        let test_screen = |test: &Terminal<Anchored<TestBackend>>| -> Vec<String> {
            rows(test.backend().under().buffer())
                .split('\n')
                .map(str::to_string)
                .collect()
        };

        for written in 1..=50 {
            on_ansi.apply_at(&row(written), now);
            on_ansi.show(&mut ansi).unwrap();
            on_test.apply_at(&row(written), now);
            on_test.show(&mut test).unwrap();
        }
        let written = test_screen(&test);
        assert_eq!(
            written[0], "  • row-016",
            "the block in work takes every row: {written:#?}"
        );
        assert_eq!(tap.screen(), written, "while the block is written");

        on_ansi.apply_at(&prompt("next"), now);
        on_ansi.show(&mut ansi).unwrap();
        on_test.apply_at(&prompt("next"), now);
        on_test.show(&mut test).unwrap();
        let committed = test_screen(&test);
        assert_eq!(
            committed[34], "▌❯ next",
            "the block is committed and the prompt is over the status row: {committed:#?}"
        );
        assert_eq!(tap.screen(), committed, "once the block is committed");
    }

    /// A finished block leaves the pane through the row ratatui borrows at
    /// its top: the bytes scroll that row into the scrollback and paint no
    /// row of the pane again, the pinned rows among them.
    #[tokio::test(start_paused = true)]
    async fn a_committed_block_is_scrolled_away_and_the_pane_is_not_painted_again() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let now = chrono::Utc::now();
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = under_way_at(&mut terminal, now);
        console.apply_at(
            &event("agent_message", "done", json!({"text": "done"})),
            now,
        );
        let mut emulator = vt100::Parser::new(40, 72, 200);
        emulator.process(tap.take().as_bytes());

        // The next block of the same turn: the one before it is finished.
        console.apply_at(
            &event("agent_message_chunk", "more", json!({"text": "more"})),
            now,
        );
        console.show(&mut terminal).unwrap();
        let written = tap.take();

        assert!(
            written.contains("\x1b[40;1H\n\x1b[1;40r\x1b[1T\x1b[r"),
            "the borrowed row is scrolled into the scrollback, and the rows \
             under it back where they were: {written:?}"
        );
        for clear in ["\x1b[2J", "\x1b[3J", "\x1b[J"] {
            assert!(
                !written.contains(clear),
                "nothing is erased whole ({clear:?}): {written:?}"
            );
        }
        for pinned in ["author", "Tell the agent what to do", "shift+enter newline"] {
            assert!(
                !written.contains(pinned),
                "the pinned rows are not painted again ({pinned}): {written:?}"
            );
        }
        emulator.process(written.as_bytes());
        let shown = everything(&mut emulator);
        assert_eq!(
            shown.iter().filter(|row| row.as_str() == "● done").count(),
            1,
            "the block is in the scrollback once: {shown:#?}"
        );
        assert_eq!(
            shown.iter().filter(|row| row.contains("filler-")).count(),
            60,
            "and the blocks before it are still there: {shown:#?}"
        );
        let screen: Vec<&str> = shown[shown.len() - 40..]
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(screen[34], "● more", "{screen:#?}");
        assert_pinned(&screen, " enter send", "after the block is committed");
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

    /// A terminal made narrower re-wraps its rows, the scrollback's too. The
    /// desktop app's console clears the screen and the scrollback once the
    /// resize settles and draws the whole transcript again: each block once
    /// in the scrollback at the new width, and one status row.
    #[tokio::test(start_paused = true)]
    async fn a_console_that_redraws_whole_draws_the_transcript_again_once_a_resize_settles() {
        let written = resized(Console::new(header()).redraws_whole_on_resize()).await;

        let clear = b"\x1b[2J\x1b[3J";
        let at = written
            .windows(clear.len())
            .rposition(|bytes| bytes == clear)
            .expect("the resize cleared the terminal, scrollback included");
        let mut emulator = vt100::Parser::new(40, 60, 200);
        emulator.process(&written[at..]);
        let shown = everything(&mut emulator).join("\n");
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
    /// resize draws the pane again and clears nothing above it.
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

        let written = tap.take();
        assert!(
            !written.contains(&" ".repeat(20)),
            "no row is padded with spaces: {written:?}"
        );
        let mut emulator = vt100::Parser::new(40, 72, 200);
        emulator.process(written.as_bytes());
        let shown = everything(&mut emulator);
        assert_eq!(
            shown[..4],
            ["▌❯ go", "", "● done", ""],
            "and the blocks read as they did: {shown:#?}"
        );
    }

    /// The pane owns the screen, so each resize erases its rows and draws
    /// it again: whatever the terminal did with the old rows, no stale row
    /// is left and the status row is drawn once. The blocks in the
    /// scrollback stay there, once each.
    #[test]
    fn a_resize_either_way_leaves_no_stale_row_and_one_status_row() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut test = pane();
        let mut on_ansi = Console::new(header());
        let mut on_test = Console::new(header());
        let events = [
            prompt("go"),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
            prompt("next"),
            event("agent_message_chunk", "writing", json!({"text": "writing"})),
        ];
        on_ansi.snapshot(&events);
        on_test.snapshot(&events);
        on_ansi.show(&mut terminal).unwrap();
        on_test.show(&mut test).unwrap();
        let mut emulator = vt100::Parser::new(40, 72, 200);
        emulator.process(tap.take().as_bytes());

        for (width, height, what) in [
            (72, 24, "shorter"),
            (72, 40, "taller"),
            (50, 40, "narrower"),
            (100, 40, "wider"),
        ] {
            window.set(width, height);
            emulator.screen_mut().set_size(height, width);
            test.backend_mut().under_mut().resize(width, height);
            on_ansi.show(&mut terminal).unwrap();
            on_test.show(&mut test).unwrap();
            emulator.process(tap.take().as_bytes());

            let expected = rows(test.backend().under().buffer());
            let screen: Vec<String> = emulator
                .screen()
                .rows(0, width)
                .map(|row| row.trim_end().to_string())
                .collect();
            assert_eq!(
                screen.join("\n"),
                expected,
                "{what}: the emulator shows the pane and nothing else"
            );
            let screen: Vec<&str> = screen.iter().map(String::as_str).collect();
            assert_eq!(
                screen
                    .iter()
                    .filter(|row| row.starts_with(" author · "))
                    .count(),
                1,
                "{what}: one status row: {screen:#?}"
            );
            assert_eq!(
                screen[usize::from(height - PINNED - 1)],
                "● writing",
                "{what}"
            );
            assert_pinned(&screen, " enter send", what);
            let shown = everything(&mut emulator);
            for once in ["▌❯ go", "● done", "▌❯ next", "● writing"] {
                assert_eq!(
                    shown.iter().filter(|row| row.as_str() == once).count(),
                    1,
                    "{what}: {once} once: {shown:#?}"
                );
            }
        }
    }

    /// tmux takes an erase from the top-left corner to the end of the screen
    /// for a clear of the whole screen, and keeps a copy of what it erased
    /// in its scrollback: the pane, which is on the top row, each time it is
    /// opened again.
    #[test]
    fn a_pane_on_the_top_row_is_erased_row_by_row_and_never_from_the_corner() {
        let tap = Tap::default();
        let window = Window::new(72, 24);
        let mut terminal = super::open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = under_way(&mut terminal);
        tap.take();

        window.set(60, 24);
        console.show(&mut terminal).unwrap();
        for text in ["a", "b", "c"] {
            type_into(&mut console, text);
            console.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
            console.show(&mut terminal).unwrap();
        }

        let written = tap.take();
        assert!(
            written.contains("\x1b[24;1H\x1b[K"),
            "the resize erased the pane's rows, the last one too: {written:?}"
        );
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

    /// The way out wipes the pane, and the shell comes back on the top row
    /// of the screen: right under the last block, which is the last row of
    /// the scrollback, and not on the row the input box had the cursor on.
    #[test]
    fn the_shell_comes_back_right_under_the_last_block_in_the_scrollback() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("go"),
            event("agent_message", "done", json!({"text": "done"})),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);
        console.show(&mut terminal).unwrap();

        console.close(&mut terminal).unwrap();

        let scrollback = rows(terminal.backend().under().scrollback());
        assert_eq!(scrollback, "▌❯ go\n\n● done\n", "the transcript");
        let screen = rows(terminal.backend().under().buffer());
        assert!(
            screen.lines().all(str::is_empty),
            "the pane is wiped: {screen}"
        );
        assert_eq!(
            terminal
                .backend_mut()
                .under_mut()
                .get_cursor_position()
                .unwrap(),
            Position::ORIGIN,
            "the cursor is on the row after the scrollback's last: {screen}"
        );
    }

    /// A block still being written at the close goes to the terminal as it
    /// stands, on the top rows, and the shell comes back under it.
    #[test]
    fn the_shell_comes_back_under_a_block_the_close_leaves_on_the_screen() {
        let mut terminal = pane();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("go"),
            event("agent_message_chunk", "half", json!({"text": "half"})),
        ]);
        console.show(&mut terminal).unwrap();

        console.close(&mut terminal).unwrap();

        let shown = history(&terminal);
        assert_eq!(
            shown.trim_end(),
            "▌❯ go\n\n● half",
            "the block is under the scrollback, and the pane is wiped: {shown}"
        );
        assert_eq!(
            terminal
                .backend_mut()
                .under_mut()
                .get_cursor_position()
                .unwrap(),
            Position { x: 0, y: 2 },
            "the cursor is under the separator of the last block: {shown}"
        );
    }

    /// The picker folds its diff to the room it is given (rule 27): the
    /// terminal's height less the pinned rows.
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
            "the question is the first row the pane draws: {shown}"
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
            event("agent_message_chunk", "second", json!({"text": "second"})),
        ]);

        console.show(&mut terminal).unwrap();

        let scrollback = rows(terminal.backend().under().0.scrollback());
        assert!(
            scrollback.ends_with("▌❯ first\n"),
            "the finished prompt is in the scrollback: {scrollback}"
        );
        let screen = rows(terminal.backend().under().0.buffer());
        assert_eq!(
            row_of(&screen, "● second"),
            Some(40 - usize::from(PINNED) - 1),
            "and the block in work is over the pinned rows: {screen}"
        );
    }

    /// The terminal answered where its cursor was at the open, and answers
    /// nothing after: the console answers every later query itself rather
    /// than end on its first finished block.
    #[test]
    fn a_finished_block_reaches_the_scrollback_when_only_the_first_cursor_query_is_answered() {
        let mut terminal = super::open(Mute::answering_once).unwrap();
        let mut console = Console::new(header());
        console.snapshot(&[
            prompt("first"),
            event("agent_message_chunk", "second", json!({"text": "second"})),
        ]);

        console
            .show(&mut terminal)
            .expect("the block is inserted without asking the terminal again");

        let scrollback = rows(terminal.backend().under().0.scrollback());
        assert_eq!(scrollback, "▌❯ first\n", "the finished prompt");
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
        terminal.backend_mut().under_mut().0.resize(72, 30);
        let error = console
            .show(&mut terminal)
            .expect_err("the pane cannot be resized on a terminal that went away");

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
