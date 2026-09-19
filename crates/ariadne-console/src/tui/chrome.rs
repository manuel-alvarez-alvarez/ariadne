//! What is drawn around the transcript: the layout of the viewport, the live
//! area, and the status line with its spinner and clock.

use std::time::Duration;

use ratatui::Frame as Draw;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::theme::{AGENT, DIM, SPINNER, TOOL};

use super::blocks::block;
use super::picker::picker;
use super::{Console, Link, Turn};

/// The status line.
const STATUS_ROWS: u16 = 1;

impl Console {
    /// The rows under the live area: the status line, and the input box with
    /// its borders. They grow only as the box does.
    pub fn pinned_rows(&self, width: u16) -> u16 {
        STATUS_ROWS + self.input.height(width)
    }

    /// Draw the viewport: what is being written, the status line, the box.
    pub fn render(&self, frame: &mut Draw) {
        let area = frame.area();
        let [live, pinned] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(self.pinned_rows(area.width)),
        ])
        .areas(area);
        let [status, input] =
            Layout::vertical([Constraint::Length(STATUS_ROWS), Constraint::Min(0)]).areas(pinned);

        self.draw_live(frame, live);
        self.draw_status(frame, status);
        self.input.draw(frame, input);
    }

    /// The blocks not yet in the scrollback, and the picker of a question
    /// waiting for its answer.
    fn draw_live(&self, frame: &mut Draw, area: Rect) {
        let width = usize::from(area.width);
        let height = usize::from(area.height);
        let asking = self.question();
        let mut lines = Vec::new();
        for (at, item) in self.items.iter().enumerate().skip(self.committed) {
            if Some(at) != asking {
                lines.extend(block(item, width, None));
            }
        }
        // The picker is what the keys act on, so it is drawn last, above the
        // box, whatever came after it — a snapshot taken mid-turn ends on the
        // text so far (008), which would otherwise push it off the top — and
        // its command or diff is folded to the room its question and options
        // leave, so both are on the screen however long the diff.
        if let Some(at) = asking {
            lines.extend(picker(&self.items[at], self.picked, width, height));
        }
        // The tail is what is happening now; the head of a long block has
        // scrolled past, exactly as it would have in the scrollback.
        let skip = lines.len().saturating_sub(height);
        frame.render_widget(Paragraph::new(Text::from(lines[skip..].to_vec())), area);
    }

    fn draw_status(&self, frame: &mut Draw, area: Rect) {
        frame.render_widget(Paragraph::new(self.status()), area);
    }

    /// The status line: who is answering, on what, and what it is doing.
    fn status(&self) -> Line<'static> {
        let mut spans = vec![
            Span::styled(format!("{} ", self.header.seat), AGENT),
            Span::styled(self.header.model.clone(), DIM),
            Span::styled(" · ", DIM),
        ];
        match self.link {
            Link::Reconnecting => spans.push(Span::styled("reconnecting", TOOL)),
            Link::Live => spans.push(Span::raw(self.header.status.clone())),
        }
        match &self.turn {
            Turn::Idle => {}
            Turn::Thinking => {
                spans.push(Span::styled(format!("  {} ", self.spinner()), AGENT));
                spans.push(Span::styled("thinking", DIM));
            }
            Turn::Running(tool) => {
                spans.push(Span::styled(format!("  {} ", self.spinner()), TOOL));
                spans.push(Span::styled(format!("running {tool}"), DIM));
            }
        }
        if let Some(since) = self.since {
            spans.push(Span::styled(format!(" {}", clock(since.elapsed())), DIM));
        }
        spans.push(Span::styled(
            match (self.armed, self.question().is_some(), self.turn.running()) {
                (true, _, _) => "   ctrl-c again to leave".to_string(),
                (_, true, _) => "   ↑↓ choose · enter answer".to_string(),
                (_, _, true) => "   enter send · esc cancel · ctrl-c quit".to_string(),
                _ => "   enter send · shift+enter newline · ctrl-c quit".to_string(),
            },
            DIM,
        ));
        Line::from(spans)
    }

    fn spinner(&self) -> &'static str {
        SPINNER[self.tick % SPINNER.len()]
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
    use serde_json::json;

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
            seat: "author".into(),
            model: "claude:opus".into(),
            status: "idle".into(),
        });
        let mut terminal = terminal();
        let status = |console: &Console, terminal: &mut Terminal<TestBackend>| {
            terminal.draw(|frame| console.render(frame)).unwrap();
            let shown = screen(terminal);
            shown
                .lines()
                .find(|line| line.starts_with("author claude:opus · "))
                .map(|line| {
                    line.trim_start_matches("author claude:opus · ")
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
            shown.contains("author claude:opus · idle"),
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
                seat: "author".into(),
                model: "claude:opus".into(),
                status: ended.into(),
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
                shown.contains(&format!("author claude:opus · {ended}")),
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
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 45s"),
            "the turn began 45 seconds before the attach: {}",
            screen(&terminal)
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
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 30s"),
            "the replay did not restart the clock: {}",
            screen(&terminal)
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
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            screen(&terminal).contains("thinking 30s"),
            "{}",
            screen(&terminal)
        );

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
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("thinking 5s"),
            "the clock is the new turn's, not the old one's: {}",
            screen(&terminal)
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
        let status_line = |terminal: &Terminal<TestBackend>| {
            screen(terminal)
                .lines()
                .find(|line| line.starts_with("author"))
                .unwrap()
                .to_string()
        };

        console.apply(&prompt("first"));
        tokio::time::advance(Duration::from_secs(64)).await;
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            status_line(&terminal).contains("thinking 1m 04s"),
            "{}",
            screen(&terminal)
        );

        console.apply(&stopped);
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            !status_line(&terminal).contains("1m 04s"),
            "the clock is off between turns: {}",
            screen(&terminal)
        );

        console.apply(&prompt("second"));
        tokio::time::advance(Duration::from_secs(5)).await;
        terminal.draw(|frame| console.render(frame)).unwrap();
        assert!(
            status_line(&terminal).contains("thinking 5s"),
            "the next turn starts from zero: {}",
            screen(&terminal)
        );
    }

    #[test]
    fn a_dropped_stream_says_reconnecting_on_the_status_line() {
        let mut console = Console::new(header());
        let mut terminal = terminal();

        console.dropped();
        terminal.draw(|frame| console.render(frame)).unwrap();

        assert!(
            screen(&terminal).contains("reconnecting"),
            "{}",
            screen(&terminal)
        );
    }
}
