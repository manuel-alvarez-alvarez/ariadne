//! A ratatui backend for a terminal the process cannot see.
//!
//! `CrosstermBackend` draws by writing escape sequences to stdout, and asks
//! the process terminal what it cannot know from that: the cursor position
//! and the size. The daemon hosts the console for a terminal emulator at the
//! far end of a socket — xterm.js in the desktop app — and has no process
//! terminal to ask. [`AnsiBackend`] writes the same sequences a terminal
//! reads, into whatever `Write` the host gives it, and answers both
//! questions itself: the cursor from what it wrote, and the size from what
//! the client last said it was, through the [`Window`] the host keeps a
//! handle on. No query ever goes to a terminal, so [`Viewport::Inline`] and
//! [`Terminal::insert_before`] work as they do on the CLI, and the
//! scrollback lives in the emulator.
//!
//! [`Viewport::Inline`]: ratatui::Viewport::Inline
//! [`Terminal::insert_before`]: ratatui::Terminal::insert_before

use std::io::{self, Write};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

/// The size of the terminal at the far end, as the client last reported it.
///
/// Shared between the backend, which answers ratatui's `size` from it, and
/// the host, which sets it when the client says the terminal was resized —
/// before it hands the console the resize event that redraws at the new
/// size.
#[derive(Debug, Clone)]
pub struct Window(Arc<Mutex<Size>>);

impl Window {
    pub fn new(width: u16, height: u16) -> Self {
        Self(Arc::new(Mutex::new(Size { width, height })))
    }

    pub fn set(&self, width: u16, height: u16) {
        *self.0.lock().expect("window size lock") = Size { width, height };
    }

    pub fn size(&self) -> Size {
        *self.0.lock().expect("window size lock")
    }
}

/// A backend that writes ANSI escape sequences to `out` and never reads a
/// terminal.
pub struct AnsiBackend<W: Write> {
    out: W,
    window: Window,
    /// Where the cursor is, as far as what was written moved it.
    cursor: Position,
}

impl<W: Write> AnsiBackend<W> {
    /// A backend writing to `out`, on a terminal of `window`'s size, with the
    /// cursor at the top left — where a fresh emulator has it.
    pub fn new(out: W, window: Window) -> Self {
        Self {
            out,
            window,
            cursor: Position::ORIGIN,
        }
    }

    fn move_to(&mut self, at: Position) -> io::Result<()> {
        write!(self.out, "\x1b[{};{}H", at.y + 1, at.x + 1)?;
        self.cursor = at;
        Ok(())
    }

    /// Scroll the rows of `region` by `lines` inside a `DECSTBM` region, with
    /// `SU` or `SD` as `direction` says, and reset the region after. Both
    /// `DECSTBM`s put the cursor on the screen's first cell.
    fn scroll_region(&mut self, region: Range<u16>, lines: u16, direction: char) -> io::Result<()> {
        if lines == 0 || region.is_empty() {
            return Ok(());
        }
        write!(
            self.out,
            "\x1b[{};{}r\x1b[{lines}{direction}\x1b[r",
            region.start + 1,
            region.end
        )?;
        self.cursor = Position::ORIGIN;
        self.out.flush()
    }
}

impl<W: Write> Backend for AnsiBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let width = self.window.size().width;
        let mut fg = Color::Reset;
        let mut bg = Color::Reset;
        let mut modifier = Modifier::empty();
        let mut last: Option<Position> = None;
        for (x, y, cell) in content {
            let at = Position { x, y };
            // The cursor is where the last symbol left it only when this
            // cell is the very next one on the same row.
            if !matches!(last, Some(p) if x == p.x + 1 && y == p.y) {
                self.move_to(at)?;
            }
            last = Some(at);
            if cell.modifier != modifier {
                modifiers(&mut self.out, modifier, cell.modifier)?;
                modifier = cell.modifier;
            }
            if cell.fg != fg {
                color(&mut self.out, cell.fg, 30)?;
                fg = cell.fg;
            }
            if cell.bg != bg {
                color(&mut self.out, cell.bg, 40)?;
                bg = cell.bg;
            }
            let symbol = cell.symbol();
            self.out.write_all(symbol.as_bytes())?;
            // A wide symbol moves the cursor two columns; the cells it
            // covers are not in `content`, so the next one is a move.
            let advanced = u16::try_from(symbol.width()).unwrap_or(u16::MAX);
            self.cursor = Position {
                x: x.saturating_add(advanced).min(width),
                y,
            };
        }
        self.out.write_all(b"\x1b[0m")
    }

    /// `n` line feeds: the cursor moves down, keeping its column, and the
    /// terminal scrolls its contents into the scrollback where it has no
    /// rows left. This is how ratatui makes room under an inline viewport.
    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        for _ in 0..n {
            self.out.write_all(b"\n")?;
        }
        let height = self.window.size().height;
        self.cursor.y = self
            .cursor
            .y
            .saturating_add(n)
            .min(height.saturating_sub(1));
        self.out.flush()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.out.write_all(b"\x1b[?25l")
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.out.write_all(b"\x1b[?25h")
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.move_to(position.into())
    }

    /// A clear is whole: the screen, the scrollback, and the cursor home.
    /// The emulator at the far end holds this console and nothing else, so
    /// the one thing that clears it is a redraw of the whole transcript
    /// ([`crate::Console::redraws_whole_on_resize`]), which would otherwise
    /// leave the old copy in the scrollback above the new one.
    fn clear(&mut self) -> io::Result<()> {
        self.out.write_all(b"\x1b[2J\x1b[3J")?;
        self.move_to(Position::ORIGIN)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        // Erasing moves no cursor, on any terminal.
        self.out.write_all(match clear_type {
            ClearType::All => b"\x1b[2J",
            ClearType::AfterCursor => b"\x1b[J",
            ClearType::BeforeCursor => b"\x1b[1J",
            ClearType::CurrentLine => b"\x1b[2K",
            ClearType::UntilNewLine => b"\x1b[K",
        })
    }

    /// Scroll the rows of `region` up by `lines`, as crossterm's backend does
    /// for the CLI: `DECSTBM` sets the region, `SU` scrolls it, and the
    /// region is reset to the whole screen after. The rows that leave a
    /// region that starts on the top row go into the scrollback. A region of
    /// one row is not one a terminal takes, and the pane's own backend,
    /// `Anchored`, never asks for one.
    fn scroll_region_up(&mut self, region: Range<u16>, lines: u16) -> io::Result<()> {
        self.scroll_region(region, lines, 'S')
    }

    /// Scroll the rows of `region` down by `lines`, with `SD` where `SU`
    /// scrolls up. Nothing reaches the scrollback: the rows that leave the
    /// bottom of the region are gone, and the rows freed at its top are
    /// blank.
    fn scroll_region_down(&mut self, region: Range<u16>, lines: u16) -> io::Result<()> {
        self.scroll_region(region, lines, 'T')
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.window.size())
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        // The client reports cells, never pixels.
        Ok(WindowSize {
            columns_rows: self.window.size(),
            pixels: Size::default(),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// The SGR sequence that takes the terminal's attributes from `from` to
/// `to`: what crossterm writes for the same change, so a terminal that reads
/// the CLI's console reads this one the same.
fn modifiers(out: &mut impl Write, from: Modifier, to: Modifier) -> io::Result<()> {
    let removed = from - to;
    if removed.contains(Modifier::REVERSED) {
        out.write_all(b"\x1b[27m")?;
    }
    if removed.contains(Modifier::BOLD) {
        // Normal intensity ends dim with bold; a dim that stays is said again.
        out.write_all(b"\x1b[22m")?;
        if to.contains(Modifier::DIM) {
            out.write_all(b"\x1b[2m")?;
        }
    }
    if removed.contains(Modifier::ITALIC) {
        out.write_all(b"\x1b[23m")?;
    }
    if removed.contains(Modifier::UNDERLINED) {
        out.write_all(b"\x1b[24m")?;
    }
    if removed.contains(Modifier::DIM) {
        // Normal intensity ends bold with dim; a bold that stays is said again.
        out.write_all(b"\x1b[22m")?;
        if to.contains(Modifier::BOLD) {
            out.write_all(b"\x1b[1m")?;
        }
    }
    if removed.contains(Modifier::CROSSED_OUT) {
        out.write_all(b"\x1b[29m")?;
    }
    if removed.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        out.write_all(b"\x1b[25m")?;
    }
    if removed.contains(Modifier::HIDDEN) {
        out.write_all(b"\x1b[28m")?;
    }

    let added = to - from;
    if added.contains(Modifier::REVERSED) {
        out.write_all(b"\x1b[7m")?;
    }
    if added.contains(Modifier::BOLD) {
        out.write_all(b"\x1b[1m")?;
    }
    if added.contains(Modifier::ITALIC) {
        out.write_all(b"\x1b[3m")?;
    }
    if added.contains(Modifier::UNDERLINED) {
        out.write_all(b"\x1b[4m")?;
    }
    if added.contains(Modifier::DIM) {
        out.write_all(b"\x1b[2m")?;
    }
    if added.contains(Modifier::CROSSED_OUT) {
        out.write_all(b"\x1b[9m")?;
    }
    if added.contains(Modifier::SLOW_BLINK) {
        out.write_all(b"\x1b[5m")?;
    }
    if added.contains(Modifier::RAPID_BLINK) {
        out.write_all(b"\x1b[6m")?;
    }
    if added.contains(Modifier::HIDDEN) {
        out.write_all(b"\x1b[8m")?;
    }
    Ok(())
}

/// The SGR sequence for one colour. `base` is 30 for the foreground and 40
/// for the background: the named colours count up from it, the bright ones
/// from 60 above it, and the palette and RGB forms hang off `base + 8`.
fn color(out: &mut impl Write, color: Color, base: u8) -> io::Result<()> {
    let code = match color {
        Color::Reset => base + 9,
        Color::Black => base,
        Color::Red => base + 1,
        Color::Green => base + 2,
        Color::Yellow => base + 3,
        Color::Blue => base + 4,
        Color::Magenta => base + 5,
        Color::Cyan => base + 6,
        Color::Gray => base + 7,
        Color::DarkGray => base + 60,
        Color::LightRed => base + 61,
        Color::LightGreen => base + 62,
        Color::LightYellow => base + 63,
        Color::LightBlue => base + 64,
        Color::LightMagenta => base + 65,
        Color::LightCyan => base + 66,
        Color::White => base + 67,
        Color::Rgb(r, g, b) => return write!(out, "\x1b[{};2;{r};{g};{b}m", base + 8),
        Color::Indexed(n) => return write!(out, "\x1b[{};5;{n}m", base + 8),
    };
    write!(out, "\x1b[{code}m")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::Style;
    use ratatui::text::Line;
    use ratatui::widgets::{Block, Paragraph, Widget};
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use crate::tui::Anchored;

    use super::*;

    /// The bytes the backend wrote, readable while the terminal still owns
    /// the backend.
    #[derive(Clone, Default)]
    struct Tap(Arc<Mutex<Vec<u8>>>);

    impl Write for Tap {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Tap {
        /// What a terminal of `window`'s size shows after reading the bytes,
        /// scrolled `back` rows into its scrollback: one trimmed string per
        /// row.
        fn screen(&self, window: &Window, back: usize) -> Vec<String> {
            let size = window.size();
            let mut parser = vt100::Parser::new(size.height, size.width, back);
            parser.process(&self.0.lock().unwrap());
            parser.screen_mut().set_scrollback(back);
            parser
                .screen()
                .contents()
                .lines()
                .map(|row| row.trim_end().to_string())
                .collect()
        }

        fn cursor(&self, window: &Window) -> Position {
            let size = window.size();
            let mut parser = vt100::Parser::new(size.height, size.width, 0);
            parser.process(&self.0.lock().unwrap());
            let (y, x) = parser.screen().cursor_position();
            Position { x, y }
        }
    }

    /// An inline viewport of 3 rows on the backend, under the pane's own
    /// backend as every host has it ([`crate::tui::open`]).
    fn inline(window: &Window, tap: &Tap) -> Terminal<Anchored<AnsiBackend<Tap>>> {
        Terminal::with_options(
            Anchored::over(AnsiBackend::new(tap.clone(), window.clone())),
            TerminalOptions {
                viewport: Viewport::Inline(3),
            },
        )
        .unwrap()
    }

    fn draw(terminal: &mut Terminal<impl Backend>, lines: &[&str]) {
        let text: Vec<Line> = lines.iter().map(|line| Line::from(*line)).collect();
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(text), frame.area());
                frame.set_cursor_position((1, 1));
            })
            .unwrap();
    }

    /// A terminal reading the bytes shows what ratatui's own test backend
    /// shows of the same drawing: the styled cells, in place, and nothing
    /// the process terminal would have had to answer.
    #[test]
    fn the_bytes_draw_what_the_test_backend_draws() {
        let window = Window::new(20, 4);
        let tap = Tap::default();
        let mut ansi = Terminal::new(AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut test = Terminal::new(TestBackend::new(20, 4)).unwrap();
        let widget = |frame: &mut ratatui::Frame| {
            let styled = Paragraph::new(vec![
                Line::styled("bold 你好", Style::new().add_modifier(Modifier::BOLD)),
                Line::styled("green", Style::new().fg(Color::Green)),
            ])
            .block(Block::bordered());
            frame.render_widget(styled, frame.area());
        };

        ansi.draw(widget).unwrap();
        test.draw(widget).unwrap();

        let expected: Vec<String> = (0..4)
            .map(|y| {
                let buffer = test.backend().buffer();
                let mut row = String::new();
                let mut x = 0;
                while x < 20 {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    x += u16::try_from(symbol.width().max(1)).unwrap();
                }
                row.trim_end().to_string()
            })
            .collect();
        assert_eq!(tap.screen(&window, 0), expected);
        let bytes = tap.0.lock().unwrap().clone();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("\x1b[1mbold"),
            "bold is said in SGR: {text:?}"
        );
        assert!(
            text.contains("\x1b[32mgreen"),
            "green is said in SGR: {text:?}"
        );
    }

    /// The attributes a terminal ends up with are the cells': the one SGR
    /// that ends dim ends bold too, so a bold that stays is said again — and
    /// the same for a dim that stays when bold ends.
    #[test]
    fn a_cell_keeps_its_bold_when_the_dim_beside_it_ends() {
        let window = Window::new(8, 2);
        let tap = Tap::default();
        let mut backend = AnsiBackend::new(tap.clone(), window.clone());
        let styled = |modifier: Modifier| {
            let mut cell = Cell::default();
            cell.set_symbol("x");
            cell.modifier = modifier;
            cell
        };
        let both = styled(Modifier::BOLD | Modifier::DIM);
        let bold = styled(Modifier::BOLD);
        let dim = styled(Modifier::DIM);
        let plain = styled(Modifier::empty());

        backend
            .draw(
                [
                    (0u16, 0u16, &both),
                    (1, 0, &bold),
                    (2, 0, &both),
                    (3, 0, &dim),
                    (4, 0, &plain),
                ]
                .into_iter(),
            )
            .unwrap();

        // vt100 keeps one intensity per cell, the last one said, so the
        // cells set to both read as dim; the cells after them are the claim.
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process(&tap.0.lock().unwrap());
        let screen = parser.screen();
        let attributes: Vec<(bool, bool)> = (1..5)
            .map(|x| {
                let cell = screen.cell(0, x).unwrap();
                (cell.bold(), cell.dim())
            })
            .collect();
        assert_eq!(
            attributes,
            [(true, false), (false, true), (false, true), (false, false)],
            "(bold, dim) of the cells after the first: bold kept, both, dim kept, plain"
        );
    }

    /// The cursor position and the size are answered from what was written
    /// and what the client said, and agree with a terminal that read the
    /// bytes.
    #[test]
    fn the_cursor_and_the_size_are_known_without_a_query() {
        let window = Window::new(10, 5);
        let tap = Tap::default();
        let mut backend = AnsiBackend::new(tap.clone(), window.clone());

        assert_eq!(backend.get_cursor_position().unwrap(), Position::ORIGIN);
        backend
            .set_cursor_position(Position { x: 3, y: 1 })
            .unwrap();
        assert_eq!(
            backend.get_cursor_position().unwrap(),
            Position { x: 3, y: 1 }
        );
        assert_eq!(tap.cursor(&window), Position { x: 3, y: 1 });

        // Line feeds keep the column and stop at the bottom row.
        backend.append_lines(2).unwrap();
        assert_eq!(
            backend.get_cursor_position().unwrap(),
            Position { x: 3, y: 3 }
        );
        assert_eq!(tap.cursor(&window), Position { x: 3, y: 3 });
        backend.append_lines(5).unwrap();
        assert_eq!(
            backend.get_cursor_position().unwrap(),
            Position { x: 3, y: 4 }
        );
        assert_eq!(tap.cursor(&window), Position { x: 3, y: 4 });

        // Drawing leaves the cursor after the last symbol, a wide one
        // counted at its width.
        let mut cell = Cell::default();
        cell.set_symbol("你");
        backend
            .draw([(2u16, 0u16, &cell), (4, 0, &cell)].into_iter())
            .unwrap();
        assert_eq!(
            backend.get_cursor_position().unwrap(),
            Position { x: 6, y: 0 }
        );
        assert_eq!(tap.cursor(&window), Position { x: 6, y: 0 });

        assert_eq!(backend.size().unwrap(), Size::new(10, 5));
        window.set(30, 8);
        assert_eq!(backend.size().unwrap(), Size::new(30, 8));
    }

    /// An inline viewport on this backend behaves as on a terminal: a block
    /// inserted before it lands above, the viewport moves down, and what runs
    /// off the top is in the emulator's scrollback.
    #[test]
    fn an_inline_viewport_pushes_inserted_lines_into_the_scrollback() {
        let window = Window::new(12, 4);
        let tap = Tap::default();
        let mut terminal = inline(&window, &tap);

        draw(&mut terminal, &["status", "[input]"]);
        terminal
            .insert_before(2, |buffer| {
                Paragraph::new(vec![Line::from("first"), Line::from("second")])
                    .render(buffer.area, buffer);
            })
            .unwrap();
        draw(&mut terminal, &["status", "[input]"]);
        terminal
            .insert_before(1, |buffer| {
                Paragraph::new("third").render(buffer.area, buffer);
            })
            .unwrap();
        draw(&mut terminal, &["status", "[input]"]);

        assert_eq!(tap.screen(&window, 0), ["third", "status", "[input]"]);
        assert_eq!(
            tap.screen(&window, 2),
            ["first", "second", "third", "status"],
            "the two rows that ran off the top are in the scrollback"
        );
    }

    /// The rows `A` to `D` on a terminal of 4 rows, drawn on the backend and
    /// on ratatui's test backend, which is what says how a scrolling region
    /// scrolls: one call made on each, then what a terminal that read the
    /// bytes shows, what the test backend holds, what the terminal's
    /// scrollback holds and the bytes the call wrote.
    fn scrolled(region: Range<u16>, up: bool) -> (Vec<String>, Vec<String>, Vec<String>, String) {
        let window = Window::new(8, 4);
        let tap = Tap::default();
        let mut ansi = AnsiBackend::new(tap.clone(), window.clone());
        let mut test = TestBackend::new(8, 4);
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
        let before = tap.0.lock().unwrap().len();

        if up {
            ansi.scroll_region_up(region.clone(), 1).unwrap();
            test.scroll_region_up(region, 1).unwrap();
        } else {
            ansi.scroll_region_down(region.clone(), 1).unwrap();
            test.scroll_region_down(region, 1).unwrap();
        }

        let mut parser = vt100::Parser::new(4, 8, 10);
        parser.process(&tap.0.lock().unwrap());
        let screen: Vec<String> = parser
            .screen()
            .rows(0, 8)
            .map(|row| row.trim_end().to_string())
            .collect();
        parser.screen_mut().set_scrollback(10);
        let back = parser.screen().scrollback();
        let scrollback: Vec<String> = parser
            .screen()
            .rows(0, 8)
            .take(back)
            .map(|row| row.trim_end().to_string())
            .collect();
        let expected: Vec<String> = (0..4)
            .map(|y| test.buffer()[(0, y)].symbol().trim().to_string())
            .collect();
        let bytes = String::from_utf8_lossy(&tap.0.lock().unwrap()[before..]).to_string();
        (screen, expected, scrollback, bytes)
    }

    #[test]
    fn a_region_of_several_rows_scrolls_up_inside_a_decstbm_region() {
        let (screen, expected, _, bytes) = scrolled(0..3, true);

        assert_eq!(screen, ["B", "C", "", "D"], "{bytes:?}");
        assert_eq!(screen, expected, "as the test backend has it");
        assert_eq!(bytes, "\x1b[1;3r\x1b[1S\x1b[r");
    }

    #[test]
    fn a_region_scrolls_down_inside_a_decstbm_region() {
        let (screen, expected, scrollback, bytes) = scrolled(1..4, false);

        assert_eq!(screen, ["A", "", "B", "C"], "{bytes:?}");
        assert_eq!(screen, expected, "as the test backend has it");
        assert!(scrollback.is_empty(), "nothing reaches the scrollback");
        assert_eq!(bytes, "\x1b[2;4r\x1b[1T\x1b[r");
    }

    /// Told a new size, the terminal redraws its viewport at that size.
    #[test]
    fn a_resize_redraws_the_viewport_at_the_new_size() {
        let window = Window::new(12, 6);
        let tap = Tap::default();
        let mut terminal = inline(&window, &tap);
        let boxed = |frame: &mut ratatui::Frame| {
            frame.render_widget(Block::bordered(), frame.area());
        };
        terminal.draw(boxed).unwrap();
        assert!(
            tap.screen(&window, 0)
                .iter()
                .any(|row| row == "┌──────────┐"),
            "{:?}",
            tap.screen(&window, 0)
        );

        window.set(6, 6);
        terminal.resize(Rect::new(0, 0, 6, 6)).unwrap();
        terminal.draw(boxed).unwrap();

        assert!(
            tap.screen(&window, 0).iter().any(|row| row == "┌────┐"),
            "{:?}",
            tap.screen(&window, 0)
        );
    }
}
