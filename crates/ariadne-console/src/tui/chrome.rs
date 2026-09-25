//! What is drawn around the transcript: the layout of the viewport, the live
//! area, the status row with its spinner and clock, and the footer with the
//! key hints and the tokens spent.
//!
//! Neither row is ever cut. Each is made of parts, and a row too wide for the
//! pane drops whole parts, the least important first (008).

use std::time::Duration;

use ratatui::Frame as Draw;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use ariadne_api::events::AgentEventDto;
use ariadne_api::usage::TokenUsageDto;

use crate::theme::{AGENT, DIM, GAP, SEPARATOR, SPINNER, TOKENS_IN, TOKENS_OUT, TOOL};

use super::blocks::{block, queued};
use super::picker::picker;
use super::{Console, Link, Turn};

/// The status row, over the box.
const STATUS_ROWS: u16 = 1;
/// The footer, under the box.
const FOOTER_ROWS: u16 = 1;
/// The blank column at each end of both rows.
const MARGIN: &str = " ";

/// One piece of a row, drawn whole or not at all.
struct Part {
    /// How long the part stays as the row narrows: the lowest goes first.
    keep: u8,
    /// What stands between the part and the one before it.
    lead: &'static str,
    spans: Vec<Span<'static>>,
}

impl Part {
    fn new(keep: u8, lead: &'static str, spans: Vec<Span<'static>>) -> Self {
        Self { keep, lead, spans }
    }

    fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }
}

/// How wide the parts are drawn one after another, by display width: the
/// first one's lead is not drawn.
fn width<'a>(parts: impl IntoIterator<Item = &'a Part>) -> usize {
    parts
        .into_iter()
        .enumerate()
        .map(|(at, part)| {
            part.width()
                + if at == 0 {
                    0
                } else {
                    Span::raw(part.lead).width()
                }
        })
        .sum()
}

/// Drop the least important part, the later of two that tie, until `fits`.
fn fit(parts: &mut Vec<Part>, fits: impl Fn(&[Part]) -> bool) {
    while !parts.is_empty() && !fits(parts) {
        let (at, _) = parts
            .iter()
            .enumerate()
            .min_by_key(|(at, part)| (part.keep, std::cmp::Reverse(*at)))
            .expect("parts is not empty");
        parts.remove(at);
    }
}

/// The parts drawn one after another, each after its lead.
fn spans(parts: Vec<Part>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (at, part) in parts.into_iter().enumerate() {
        if at > 0 {
            spans.push(Span::styled(part.lead, DIM));
        }
        spans.extend(part.spans);
    }
    spans
}

impl Console {
    /// The rows under the live area: the status row, the input box with its
    /// borders, and the footer. They grow only as the box does.
    pub fn pinned_rows(&self, width: u16) -> u16 {
        STATUS_ROWS + self.input.height(width) + FOOTER_ROWS
    }

    /// Draw the viewport: what is being written, the status row, the box and
    /// the footer.
    pub fn render(&self, frame: &mut Draw) {
        let area = frame.area();
        let [live, pinned] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(self.pinned_rows(area.width)),
        ])
        .areas(area);
        let [status, input, footer] = Layout::vertical([
            Constraint::Length(STATUS_ROWS),
            Constraint::Min(0),
            Constraint::Length(FOOTER_ROWS),
        ])
        .areas(pinned);

        self.draw_live(frame, live);
        self.draw_status(frame, status);
        self.input.draw(frame, input);
        self.draw_footer(frame, footer);
    }

    /// Every line of the live area, `width` columns wide: the blocks from
    /// `from` on — the ones not yet in the scrollback — and the picker of a
    /// question waiting for its answer, folded to `height` rows. One blank
    /// line separates two blocks, as in the scrollback. The head of a long
    /// block is in it too: the area draws the tail. The pane is as tall as
    /// these lines (008), so what is drawn and what is counted are the one
    /// list.
    pub fn live_lines(&self, from: usize, width: u16, height: u16) -> Vec<Line<'static>> {
        let width = usize::from(width);
        let height = usize::from(height);
        let asking = self.question();
        let mut blocks = Vec::new();
        for (at, item) in self.items.iter().enumerate().skip(from) {
            if Some(at) == asking {
                continue;
            }
            blocks.push(match self.pending.contains(&at) {
                true => queued(item, width, self.whole),
                false => block(item, width, None, self.whole),
            });
        }
        // The picker is what the keys act on, so it is drawn last, above the
        // box, whatever came after it — a snapshot taken mid-turn ends on the
        // text so far (008), which would otherwise push it off the top — and
        // its command or diff is folded to the room its question and options
        // leave, so both are on the screen however long the diff.
        if let Some(at) = asking {
            blocks.push(picker(&self.items[at], self.picked, width, height));
        }
        let mut lines = Vec::new();
        for block in blocks.into_iter().filter(|block| !block.is_empty()) {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.extend(block);
        }
        lines
    }

    /// The blocks not yet in the scrollback, and the picker of a question
    /// waiting for its answer.
    fn draw_live(&self, frame: &mut Draw, area: Rect) {
        let lines = self.live_lines(self.committed, area.width, area.height);
        // The tail is what is happening now; the head of a long block has
        // scrolled past, exactly as it would have in the scrollback.
        let skip = lines.len().saturating_sub(usize::from(area.height));
        frame.render_widget(Paragraph::new(Text::from(lines[skip..].to_vec())), area);
    }

    fn draw_status(&self, frame: &mut Draw, area: Rect) {
        let room = usize::from(area.width).saturating_sub(2 * MARGIN.len());
        let mut parts = self.status();
        fit(&mut parts, |parts| width(parts) <= room);
        let mut line = vec![Span::raw(MARGIN)];
        line.extend(spans(parts));
        frame.render_widget(Paragraph::new(Line::from(line)), area);
    }

    /// The status row: who is answering, on what, and what it is doing. The
    /// session's status goes last as the row narrows, then what the turn is
    /// doing, its clock, the seat and the model.
    fn status(&self) -> Vec<Part> {
        let mut parts = vec![
            Part::new(
                2,
                SEPARATOR,
                vec![Span::styled(self.header.seat.clone(), AGENT)],
            ),
            Part::new(
                1,
                SEPARATOR,
                vec![Span::styled(
                    self.header.model.as_deref().unwrap_or("-").to_string(),
                    DIM,
                )],
            ),
            Part::new(
                5,
                SEPARATOR,
                vec![match self.link {
                    Link::Reconnecting => Span::styled("reconnecting", TOOL),
                    Link::Live => Span::raw(self.header.status.clone()),
                }],
            ),
        ];
        let doing = match &self.turn {
            Turn::Idle => None,
            Turn::Thinking => Some((AGENT, "thinking".to_string())),
            Turn::Running(tool) => Some((TOOL, format!("running {tool}"))),
        };
        if let Some((style, doing)) = doing {
            parts.push(Part::new(
                4,
                GAP,
                vec![
                    Span::styled(format!("{} ", self.spinner()), style),
                    Span::styled(doing, DIM),
                ],
            ));
        }
        if let Some(since) = self.since {
            parts.push(Part::new(
                3,
                " ",
                vec![Span::styled(clock(since.elapsed()), DIM)],
            ));
        }
        parts
    }

    fn draw_footer(&self, frame: &mut Draw, area: Rect) {
        let room = usize::from(area.width).saturating_sub(2 * MARGIN.len());
        let gap = Span::raw(GAP).width();
        let mut parts = self.hints();
        parts.extend(self.tokens());
        let right = |part: &Part| part.lead.is_empty();
        fit(&mut parts, |parts| {
            let (tokens, hints): (Vec<&Part>, Vec<&Part>) =
                parts.iter().partition(|part| right(part));
            let (tokens, hints) = (width(tokens), width(hints));
            hints + tokens + if hints > 0 && tokens > 0 { gap } else { 0 } <= room
        });
        let (tokens, hints): (Vec<_>, Vec<_>) = parts.into_iter().partition(|part| right(part));
        let mut left = vec![Span::raw(MARGIN)];
        left.extend(spans(hints));
        frame.render_widget(Paragraph::new(Line::from(left)), area);
        let mut right = spans(tokens);
        right.push(Span::raw(MARGIN));
        frame.render_widget(Paragraph::new(Line::from(right)).right_aligned(), area);
    }

    /// The keys of the state the console is in, the first one the last to
    /// go as the footer narrows, the others from the last. The fold state —
    /// `ctrl-o unfold` or `ctrl-o fold` — is one more hint, after every
    /// other, so it is the first dropped on a narrow pane.
    fn hints(&self) -> Vec<Part> {
        let base: &[&str] = match (self.armed, self.question().is_some(), self.turn.running()) {
            (true, _, _) => &["ctrl-c again to leave"],
            (_, true, _) => &["up/down or 1-9 choose", "enter answer"],
            (_, _, true) => &[
                "enter send",
                "shift+enter newline",
                "esc cancel",
                "ctrl-c quit",
            ],
            _ => &["enter send", "shift+enter newline", "ctrl-c quit"],
        };
        let fold = if self.whole {
            "ctrl-o fold"
        } else {
            "ctrl-o unfold"
        };
        base.iter()
            .copied()
            .chain(std::iter::once(fold))
            .enumerate()
            .map(|(at, hint)| {
                let keep = if at == 0 {
                    9
                } else {
                    7 - u8::try_from(at).unwrap_or(7).min(6)
                };
                Part::new(keep, SEPARATOR, vec![Span::styled(hint, DIM)])
            })
            .collect()
    }

    /// The tokens the session has spent, `↑ 12.4k ↓ 3.1k`, on the right of
    /// the footer: nothing before the session has spent any. It goes after
    /// every hint but the first. Its lead is empty, which is what puts it on
    /// the right.
    fn tokens(&self) -> Option<Part> {
        let usage = self.usage();
        (usage.input_tokens > 0 || usage.output_tokens > 0).then(|| {
            Part::new(
                8,
                "",
                vec![Span::styled(
                    format!(
                        "{TOKENS_IN}{} {TOKENS_OUT}{}",
                        compact(usage.input_tokens),
                        compact(usage.output_tokens)
                    ),
                    DIM,
                )],
            )
        })
    }

    /// What the session has spent: what the daemon summed at attach, or the
    /// sum of the last totals of each launch since, whichever is more.
    ///
    /// The session's usage is the sum over its launches of each one's last
    /// totals, and a `stop` carries its launch's totals so far, not the
    /// turn's: the console sums them the same way. The row at attach can
    /// hold a launch's figure read mid-turn, ahead of its last stop, so the
    /// sum of the stops alone can be behind it until the next stop.
    fn usage(&self) -> TokenUsageDto {
        let mut sum = TokenUsageDto::default();
        for launch in self.launches.values() {
            sum.input_tokens += launch.input_tokens;
            sum.cached_input_tokens += launch.cached_input_tokens;
            sum.output_tokens += launch.output_tokens;
        }
        let attach = self.header.usage;
        TokenUsageDto {
            input_tokens: sum.input_tokens.max(attach.input_tokens),
            cached_input_tokens: sum.cached_input_tokens.max(attach.cached_input_tokens),
            output_tokens: sum.output_tokens.max(attach.output_tokens),
        }
    }

    /// Keep the totals an event reports for its launch, as the daemon keeps
    /// them (021): the latest replaces the one before.
    pub(super) fn follow_usage(&mut self, event: &AgentEventDto) {
        let Some(reported) = event.payload.get("ariadne_usage") else {
            return;
        };
        let counter = |key: &str| reported.get(key).and_then(serde_json::Value::as_u64);
        if let (Some(source), Some(input_tokens), Some(cached_input_tokens), Some(output_tokens)) = (
            reported.get("source").and_then(|source| source.as_str()),
            counter("input_tokens"),
            counter("cached_input_tokens"),
            counter("output_tokens"),
        ) {
            self.launches.insert(
                source.to_string(),
                TokenUsageDto {
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                },
            );
        }
    }

    fn spinner(&self) -> &'static str {
        SPINNER[self.tick % SPINNER.len()]
    }
}

/// A count of tokens in at most five columns: `950`, `12.4k`, `1.2M`.
fn compact(count: u64) -> String {
    let tenths = |unit: u64| (count + unit / 20) / (unit / 10);
    let (tenths, suffix) = match count {
        0..1_000 => return count.to_string(),
        _ if tenths(1_000) < 10_000 => (tenths(1_000), "k"),
        _ => (tenths(1_000_000), "M"),
    };
    match tenths % 10 {
        0 => format!("{}{suffix}", tenths / 10),
        tenth => format!("{}.{tenth}{suffix}", tenths / 10),
    }
}

/// How long the turn has run, in whole seconds: `12s`, or `1m 04s` past a
/// minute.
fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {

    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::{TerminalOptions, Viewport};
    use serde_json::json;
    use unicode_width::UnicodeWidthStr;

    use ariadne_api::usage::TokenUsageDto;

    use super::compact;
    use crate::tui::viewport::VIEWPORT;

    use crate::tui::testing::*;
    use crate::tui::*;

    /// An event dated this many seconds ago on the wall clock, as the
    /// daemon dates the ones it stores. Zero is an event happening now.
    fn ago(seconds: i64, kind: &str, summary: &str, payload: serde_json::Value) -> AgentEventDto {
        let at = chrono::Utc::now() - chrono::Duration::seconds(seconds);
        event_at(kind, summary, payload, &at.to_rfc3339())
    }

    /// The status line names the session's status: what the row said at
    /// attach, then what the events move it to, the way the daemon moves the
    /// row on them — running on a prompt, idle on a stop, exited at the end.
    #[test]
    fn the_status_line_follows_the_sessions_status_from_its_events() {
        let mut console = Console::new(Header {
            status: "idle".into(),
            ..header()
        });
        let mut terminal = terminal();
        let status = |console: &Console, terminal: &mut Terminal<TestBackend>| {
            terminal.draw(|frame| console.render(frame)).unwrap();
            let shown = screen(terminal);
            shown
                .lines()
                .find(|line| line.starts_with(" author · claude:opus · "))
                .map(|line| {
                    line.trim_start_matches(" author · claude:opus · ")
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string()
                })
                .unwrap_or_else(|| shown.clone())
        };

        assert_eq!(status(&console, &mut terminal), "idle", "as the row said");
        console.apply(&event(
            "user_prompt_submit",
            "go",
            json!({"text": "go", "source": "console"}),
        ));
        assert_eq!(status(&console, &mut terminal), "running", "a prompt");
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
        assert_eq!(status(&console, &mut terminal), "idle", "a stop");
        console.apply(&ended());
        assert_eq!(status(&console, &mut terminal), "exited", "the end");
    }

    /// A live row's snapshot can hold an earlier launch's end before this
    /// launch's start: a session resumed after it exited. The status line
    /// follows both, and reads the turns since as the daemon does.
    #[test]
    fn a_live_session_resumed_after_its_end_follows_its_new_launch_not_the_old_end() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.snapshot(&[
            ended(),
            event("session_start", "started", json!({})),
            event(
                "user_prompt_submit",
                "go",
                json!({"text": "go", "source": "console"}),
            ),
            event("stop", "stop", json!({"stop_reason": "end_turn"})),
        ]);

        terminal.draw(|frame| console.render(frame)).unwrap();
        let shown = screen(&terminal);

        assert!(
            shown.contains(" author · claude:opus · idle"),
            "the row is live and its last turn stopped: {shown}"
        );
    }

    /// A row the daemon's sweep marked exited has no session_end stored, and
    /// its last event maps to idle or running. The daemon moves a live row
    /// only, and so does the status line: an end is not undone by the events
    /// replayed under it.
    #[test]
    fn a_header_that_says_exited_or_failed_is_not_revived_by_the_events_replayed() {
        for ended in ["exited", "failed"] {
            let mut console = Console::new(Header {
                status: ended.into(),
                ..header()
            });
            let mut terminal = terminal();
            console.snapshot(&[
                event(
                    "user_prompt_submit",
                    "go",
                    json!({"text": "go", "source": "console"}),
                ),
                event("stop", "stop", json!({"stop_reason": "end_turn"})),
            ]);

            terminal.draw(|frame| console.render(frame)).unwrap();
            let shown = screen(&terminal);

            assert!(
                shown.contains(&format!(" author · claude:opus · {ended}")),
                "the status line still says {ended}: {shown}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_attach_during_a_turn_counts_from_the_prompt_that_began_it() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.snapshot(&[ago(
            45,
            "user_prompt_submit",
            "first",
            json!({"text": "first", "source": "console"}),
        )]);
        let (status, _) = rows_of(&mut terminal, &console);

        assert!(
            status.ends_with("thinking 45s"),
            "the turn began 45 seconds before the attach: {status}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_reconnect_keeps_the_clock_of_the_turn_still_running() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let events = vec![
            ago(
                40,
                "user_prompt_submit",
                "first",
                json!({"text": "first", "source": "console"}),
            ),
            ago(35, "stop", "stopped", json!({"stop_reason": "end_turn"})),
            ago(
                30,
                "user_prompt_submit",
                "second",
                json!({"text": "second", "source": "console"}),
            ),
        ];
        console.snapshot(&events);

        console.dropped();
        console.snapshot(&events);
        console.live();
        let (status, _) = rows_of(&mut terminal, &console);

        assert!(
            status.ends_with("thinking 30s"),
            "the replay did not restart the clock: {status}"
        );
    }

    /// The stream is down while one turn ends and the next begins: the
    /// replay brings both, and the clock is the new turn's.
    #[tokio::test(start_paused = true)]
    async fn a_turn_begun_while_the_stream_was_down_counts_from_its_own_prompt() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let first = ago(
            30,
            "user_prompt_submit",
            "first",
            json!({"text": "first", "source": "console"}),
        );
        console.snapshot(std::slice::from_ref(&first));
        let (status, _) = rows_of(&mut terminal, &console);
        assert!(status.ends_with("thinking 30s"), "{status}");

        console.dropped();
        console.snapshot(&[
            first,
            ago(20, "stop", "stopped", json!({"stop_reason": "end_turn"})),
            ago(
                5,
                "user_prompt_submit",
                "second",
                json!({"text": "second", "source": "console"}),
            ),
        ]);
        console.live();
        let (status, _) = rows_of(&mut terminal, &console);

        assert!(
            status.ends_with("thinking 5s"),
            "the clock is the new turn's, not the old one's: {status}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_status_line_counts_the_running_turn_and_stops_between_turns() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        let prompt = |text: &str| {
            ago(
                0,
                "user_prompt_submit",
                text,
                json!({"text": text, "source": "console"}),
            )
        };
        let stopped = ago(0, "stop", "stopped", json!({"stop_reason": "end_turn"}));
        let status_line =
            |terminal: &mut Terminal<TestBackend>, console: &Console| rows_of(terminal, console).0;

        console.apply(&prompt("first"));
        tokio::time::advance(Duration::from_secs(64)).await;
        let status = status_line(&mut terminal, &console);
        assert!(status.ends_with("thinking 1m 04s"), "{status}");

        console.apply(&stopped);
        let status = status_line(&mut terminal, &console);
        assert!(
            !status.contains("1m 04s"),
            "the clock is off between turns: {status}"
        );

        console.apply(&prompt("second"));
        tokio::time::advance(Duration::from_secs(5)).await;
        let status = status_line(&mut terminal, &console);
        assert!(
            status.ends_with("thinking 5s"),
            "the next turn starts from zero: {status}"
        );
    }

    #[test]
    fn a_prompt_typed_during_a_running_turn_is_queued_until_the_daemon_takes_it() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&event(
            "user_prompt_submit",
            "one",
            json!({"text": "one", "source": "console"}),
        ));
        type_into(&mut console, "two");
        enter(&mut console);

        terminal.draw(|frame| console.render(frame)).unwrap();
        let waiting = screen(&terminal);
        console.apply(&event("stop", "stop", json!({"stop_reason": "end_turn"})));
        console.apply(&event(
            "user_prompt_submit",
            "two",
            json!({"text": "two", "source": "console"}),
        ));
        terminal.draw(|frame| console.render(frame)).unwrap();
        let taken = screen(&terminal);

        assert!(waiting.contains("▌❯ two  queued"), "{waiting}");
        assert!(!waiting.contains("one  queued"), "{waiting}");
        assert!(!taken.contains("queued"), "{taken}");
        assert_eq!(taken.matches("❯ two").count(), 1, "{taken}");
    }

    /// The scrollback puts one blank line after each block, and the live
    /// area one between its blocks: a block that moves from one to the other
    /// keeps the one blank line above the next.
    #[test]
    fn two_live_blocks_have_one_blank_line_between_them_as_in_the_scrollback() {
        let mut console = Console::new(header());
        let mut terminal = terminal();
        console.apply(&event("agent_message", "first", json!({"text": "first"})));
        console.apply(&event("agent_message", "second", json!({"text": "second"})));

        terminal.draw(|frame| console.render(frame)).unwrap();
        let live = screen(&terminal);
        console.commit(&mut terminal).unwrap();
        terminal.draw(|frame| console.render(frame)).unwrap();
        let committed = screen(&terminal);

        assert!(live.contains("● first\n\n● second"), "{live}");
        assert!(committed.contains("● first\n\n● second"), "{committed}");
        assert_eq!(
            console.live_lines(console.committed, 72, 30).len(),
            1,
            "only the open block is live"
        );
    }

    #[test]
    fn a_dropped_stream_says_reconnecting_on_the_status_line() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.dropped();
        let (status, _) = rows_of(&mut terminal, &console);

        assert_eq!(status, " author · claude:opus · reconnecting");
    }

    /// The status row and the footer as `terminal` draws `console` now: the
    /// row over the box and the row under it, wherever the viewport is.
    fn rows_of(terminal: &mut Terminal<TestBackend>, console: &Console) -> (String, String) {
        let mut area = Rect::default();
        terminal
            .draw(|frame| {
                area = frame.area();
                console.render(frame);
            })
            .unwrap();
        let shown = screen(terminal);
        let lines: Vec<&str> = shown.lines().collect();
        let bottom = usize::from(area.bottom());
        let pinned = usize::from(console.pinned_rows(area.width));
        (
            lines[bottom - pinned].to_string(),
            lines[bottom - 1].to_string(),
        )
    }

    /// The footer's arrows count the tokens read and written; the keys of a
    /// question are named in words, so no arrow means a key there too.
    #[test]
    fn the_footer_uses_its_arrows_for_the_tokens_alone() {
        let mut console = Console::new(spent(12_400, 3_100));
        console.apply(&asked());

        let (_, footer) = rows_of(&mut wide(120), &console);

        assert!(footer.contains("or 1-9 choose"), "{footer:?}");
        assert_eq!(footer.matches('↑').count(), 1, "{footer:?}");
        assert_eq!(footer.matches('↓').count(), 1, "{footer:?}");
    }

    fn wide(width: u16) -> Terminal<TestBackend> {
        Terminal::with_options(
            TestBackend::new(width, 40),
            TerminalOptions {
                viewport: Viewport::Inline(VIEWPORT),
            },
        )
        .unwrap()
    }

    fn spent(input_tokens: u64, output_tokens: u64) -> Header {
        Header {
            usage: TokenUsageDto {
                input_tokens,
                cached_input_tokens: 0,
                output_tokens,
            },
            ..header()
        }
    }

    fn stopped(source: &str, input_tokens: u64, output_tokens: u64) -> AgentEventDto {
        event(
            "stop",
            &format!("stopped {source} {input_tokens}"),
            json!({"stop_reason": "end_turn", "ariadne_usage": {
                "source": source,
                "input_tokens": input_tokens,
                "cached_input_tokens": 0,
                "output_tokens": output_tokens,
            }}),
        )
    }

    const LONG_TOOL: &str = "mcp__ariadne__request_review_of_the_whole_branch";
    const TOKENS: &str = "↑ 12.4k ↓ 3.1k";

    /// One state of the console, and every part of its two rows as the
    /// spec orders them: the status row's with the lead each is drawn after,
    /// and the footer's hints.
    struct State {
        name: &'static str,
        console: Console,
        status: Vec<(&'static str, String)>,
        hints: Vec<&'static str>,
    }

    fn states() -> Vec<State> {
        let at_rest = |status: &str| Header {
            status: status.into(),
            ..spent(12_400, 3_100)
        };
        let prompt = ago(
            0,
            "user_prompt_submit",
            "go",
            json!({"text": "go", "source": "console"}),
        );
        let seat = |status: &str| {
            vec![
                ("", "author".to_string()),
                (" · ", "claude:opus".to_string()),
                (" · ", status.to_string()),
            ]
        };
        let idle = vec![
            "enter send",
            "shift+enter newline",
            "ctrl-c quit",
            "ctrl-o unfold",
        ];

        let mut thinking = Console::new(at_rest("idle"));
        thinking.apply(&prompt);
        let mut running = Console::new(at_rest("idle"));
        running.apply(&prompt);
        running.apply(&ago(
            0,
            "pre_tool_use",
            LONG_TOOL,
            json!({"tool_name": LONG_TOOL,
                   "acp": {"toolCallId": "call-1", "status": "in_progress"}}),
        ));
        let mut reconnecting = Console::new(at_rest("idle"));
        reconnecting.dropped();
        let mut armed = Console::new(at_rest("idle"));
        armed.key(ctrl('c'));
        let mut asking = Console::new(at_rest("idle"));
        asking.snapshot(&[prompt.clone(), asked()]);

        let turn = |doing: String| [("   ", doing), (" ", "0s".to_string())];
        vec![
            State {
                name: "idle",
                console: Console::new(at_rest("idle")),
                status: seat("idle"),
                hints: idle.clone(),
            },
            State {
                name: "thinking",
                console: thinking,
                status: [seat("running"), turn("⠋ thinking".into()).to_vec()].concat(),
                hints: vec![
                    "enter send",
                    "shift+enter newline",
                    "esc cancel",
                    "ctrl-c quit",
                    "ctrl-o unfold",
                ],
            },
            State {
                name: "running a tool",
                console: running,
                status: [
                    seat("running"),
                    turn(format!("⠋ running {LONG_TOOL}")).to_vec(),
                ]
                .concat(),
                hints: vec![
                    "enter send",
                    "shift+enter newline",
                    "esc cancel",
                    "ctrl-c quit",
                    "ctrl-o unfold",
                ],
            },
            State {
                name: "reconnecting",
                console: reconnecting,
                status: seat("reconnecting"),
                hints: idle,
            },
            State {
                name: "armed",
                console: armed,
                status: seat("idle"),
                hints: vec!["ctrl-c again to leave", "ctrl-o unfold"],
            },
            State {
                name: "pending question",
                console: asking,
                status: [seat("running"), turn("⠋ thinking".into()).to_vec()].concat(),
                hints: vec!["up/down or 1-9 choose", "enter answer", "ctrl-o unfold"],
            },
        ]
    }

    /// Every way to draw some of `parts` one after another, in their order,
    /// the first drawn without its lead: every row the parts can make where
    /// none is cut.
    fn whole(parts: &[(&str, String)]) -> Vec<String> {
        (0..1_u32 << parts.len())
            .map(|kept| {
                let mut row = String::new();
                for (at, (lead, part)) in parts.iter().enumerate() {
                    if kept & (1 << at) != 0 {
                        if !row.is_empty() {
                            row.push_str(lead);
                        }
                        row.push_str(part);
                    }
                }
                row
            })
            .collect()
    }

    fn hint_parts(hints: &[&str]) -> Vec<(&'static str, String)> {
        hints
            .iter()
            .map(|hint| (" · ", (*hint).to_string()))
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn each_row_drops_whole_parts_and_never_cuts_one_at_any_width() {
        for state in states() {
            for width in [40, 60, 80, 120] {
                let (status, footer) = rows_of(&mut wide(width), &state.console);
                let name = state.name;

                let drawn = status.strip_prefix(' ').unwrap_or(&status);
                assert!(
                    whole(&state.status).contains(&drawn.to_string()),
                    "{name} at {width}: the status row ends on a whole part: {status:?}"
                );
                let hints = footer
                    .strip_suffix(TOKENS)
                    .unwrap_or(&footer)
                    .trim_end()
                    .strip_prefix(' ')
                    .unwrap_or_default();
                assert!(
                    whole(&hint_parts(&state.hints)).contains(&hints.to_string()),
                    "{name} at {width}: the footer ends on a whole part: {footer:?}"
                );
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn at_120_columns_every_part_of_every_state_shows() {
        for state in states() {
            let (status, footer) = rows_of(&mut wide(120), &state.console);

            let every = |parts: &[(&str, String)]| whole(parts).pop().unwrap();
            assert_eq!(
                status,
                format!(" {}", every(&state.status)),
                "{}",
                state.name
            );
            assert!(
                footer.starts_with(&format!(" {}", every(&hint_parts(&state.hints))))
                    && footer.ends_with(TOKENS),
                "{}: {footer:?}",
                state.name
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn at_40_columns_the_status_and_the_first_hint_still_show() {
        for state in states() {
            let (status, footer) = rows_of(&mut wide(40), &state.console);

            assert!(
                status.contains(&state.status[2].1),
                "{}: {status:?}",
                state.name
            );
            assert!(
                footer.starts_with(&format!(" {}", state.hints[0])),
                "{}: {footer:?}",
                state.name
            );
        }
    }

    /// Twelve characters that take 24 columns: the row with them in it is 70
    /// columns, and 58 by a count of characters, which would fit the 58 a
    /// 60-column pane leaves and have the clock cut off the end.
    #[tokio::test(start_paused = true)]
    async fn a_tool_name_of_wide_characters_is_measured_by_the_columns_it_takes() {
        let tool = "読み込み読み込み読み込み";
        let mut console = Console::new(header());
        console.apply(&ago(
            0,
            "pre_tool_use",
            tool,
            json!({"tool_name": tool, "acp": {"toolCallId": "call-1", "status": "in_progress"}}),
        ));

        let (status, _) = rows_of(&mut wide(60), &console);

        assert_eq!(status, format!(" author · running   ⠋ running {tool} 0s"));
    }

    #[test]
    fn the_tokens_spent_are_drawn_compact_at_the_right_end_of_the_footer() {
        let console = Console::new(spent(12_400, 3_100));

        let (_, footer) = rows_of(&mut terminal(), &console);

        assert!(footer.ends_with("   ↑ 12.4k ↓ 3.1k"), "{footer:?}");
        assert_eq!(footer.width(), 72 - 1, "one blank column after it");
    }

    #[test]
    fn counts_are_compact_by_the_thousand_and_the_million() {
        let drawn = [950, 1_000, 12_400, 999_960, 1_200_000].map(compact);

        assert_eq!(drawn, ["950", "1k", "12.4k", "1M", "1.2M"]);
    }

    /// A stop carries its launch's totals so far, and the footer sums the
    /// last totals of each launch, as the daemon sums the session's.
    #[test]
    fn a_stop_that_reports_usage_moves_the_footer() {
        let mut console = Console::new(spent(12_400, 3_100));
        console.snapshot(&[stopped("launch-1", 12_400, 3_100)]);

        console.apply(&stopped("launch-1", 20_000, 5_000));
        let (_, first) = rows_of(&mut terminal(), &console);
        console.apply(&stopped("launch-2", 1_000, 200));
        let (_, second) = rows_of(&mut terminal(), &console);

        assert!(
            first.ends_with("↑ 20k ↓ 5k"),
            "the launch's new totals: {first:?}"
        );
        assert!(
            second.ends_with("↑ 21k ↓ 5.2k"),
            "and the next launch's: {second:?}"
        );
    }

    #[test]
    fn a_session_that_spent_nothing_draws_no_tokens() {
        let console = Console::new(header());

        let (_, footer) = rows_of(&mut terminal(), &console);

        assert_eq!(
            footer,
            " enter send · shift+enter newline · ctrl-c quit · ctrl-o unfold"
        );
    }

    #[test]
    fn the_hints_are_on_the_footer_and_not_on_the_status_row() {
        let console = Console::new(Header {
            status: "idle".into(),
            ..header()
        });

        let (status, footer) = rows_of(&mut terminal(), &console);

        assert_eq!(status, " author · claude:opus · idle");
        assert!(footer.starts_with(" enter send"), "{footer:?}");
    }
}
