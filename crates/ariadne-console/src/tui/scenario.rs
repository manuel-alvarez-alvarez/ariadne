//! The whole pane at once: every kind of block, the picker, a queued prompt,
//! a refused post, a dropped stream, a cancelled turn and the session's end,
//! drawn at three sizes on ratatui's test backend and, byte for byte, on the
//! ANSI backend read back by a terminal emulator.

use std::sync::{Arc, Mutex};

use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::{Position, Rect};
use serde_json::json;
use unicode_width::UnicodeWidthStr;

use crate::ansi::{AnsiBackend, Window};
use crate::tui::testing::*;
use crate::tui::*;

/// The bytes the ANSI backend wrote since they were last read.
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

/// One console drawn on both backends from the same events and keys.
struct Pair {
    test: Terminal<Anchored<TestBackend>>,
    ansi: Terminal<Anchored<AnsiBackend<Tap>>>,
    on_test: Console,
    on_ansi: Console,
    tap: Tap,
    window: Window,
    emulator: vt100::Parser,
    /// Every event so far, which is what a reconnect's snapshot holds.
    events: Vec<AgentEventDto>,
    width: u16,
}

impl Pair {
    fn new(width: u16, height: u16) -> Self {
        let tap = Tap::default();
        let window = Window::new(width, height);
        let ansi = open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let test = open(|| TestBackend::new(width, height)).unwrap();
        let mut pair = Self {
            test,
            ansi,
            on_test: Console::new(header()),
            on_ansi: Console::new(header()),
            tap,
            window,
            emulator: vt100::Parser::new(height, width, 0),
            events: Vec::new(),
            width,
        };
        pair.on_test.banner(&mut pair.test).unwrap();
        pair.on_ansi.banner(&mut pair.ansi).unwrap();
        pair.show();
        pair
    }

    /// Draw both, and check that the emulator shows the pane the test
    /// backend holds, the cursor on the same cell.
    fn show(&mut self) {
        self.on_test.show(&mut self.test).unwrap();
        self.on_ansi.show(&mut self.ansi).unwrap();
        let bytes = std::mem::take(&mut *self.tap.0.lock().unwrap());
        self.emulator.process(&bytes);

        let pane = self.test.get_frame().area();
        let buffer = self.test.backend().under().buffer();
        let expected: Vec<String> = rows(buffer)
            .lines()
            .skip(usize::from(pane.y))
            .take(usize::from(pane.height))
            .map(str::to_string)
            .collect();
        let screen = self.emulator.screen();
        let emulated: Vec<String> = screen
            .rows(0, self.width)
            .skip(usize::from(pane.y))
            .take(usize::from(pane.height))
            .map(|row| row.trim_end().to_string())
            .collect();
        assert_eq!(emulated, expected, "the emulator shows the pane");
        let size = self.test.size().unwrap();
        assert_eq!(pane, Rect::from(size), "the pane is the whole screen");
        let status = usize::from(size.height - self.on_test.pinned_rows(self.width));
        let last = expected.len() - 1;
        assert!(
            expected[status].starts_with(" author · ")
                && expected[status + 1].starts_with('─')
                && expected[last - 1].starts_with('─')
                && expected[last].starts_with(' ')
                && !expected[last].trim().is_empty(),
            "the status row, the box between its rules and the footer are the last rows: \
             {expected:#?}"
        );
        let cursor = self
            .test
            .backend_mut()
            .under_mut()
            .get_cursor_position()
            .unwrap();
        let (row, column) = screen.cursor_position();
        assert_eq!(
            Position { x: column, y: row },
            cursor,
            "the cursor is on the same cell"
        );
        assert!(
            expected
                .iter()
                .all(|row| row.width() <= usize::from(self.width)),
            "{expected:?}"
        );
    }

    /// One event, as the stream hands it over, with the id the daemon's
    /// monotonic generator gives it.
    fn event(&mut self, kind: &str, summary: &str, payload: serde_json::Value) {
        let id = u32::try_from(self.events.len() + 1).unwrap();
        // Dated after this clock: the turn counts from now on both consoles.
        let event = numbered(event_at(kind, summary, payload, "2999-01-01T00:00:00Z"), id);
        self.on_test.apply(&event);
        self.on_ansi.apply(&event);
        self.events.push(event);
        self.show();
    }

    fn key(&mut self, key: KeyEvent) -> Action {
        let action = self.on_test.key(key);
        assert_eq!(self.on_ansi.key(key), action);
        self.show();
        action
    }

    fn typed(&mut self, text: &str) {
        for character in text.chars() {
            self.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.test.backend_mut().under_mut().resize(width, height);
        self.window.set(width, height);
        self.emulator.screen_mut().set_size(height, width);
        self.width = width;
        self.show();
    }

    fn refused(&mut self, reason: &str) {
        self.on_test.failed(reason);
        self.on_ansi.failed(reason);
        self.show();
    }

    fn reconnect(&mut self) {
        self.on_test.dropped();
        self.on_ansi.dropped();
        self.show();
        let status = self.pane_text();
        assert!(status.contains("reconnecting"), "{status}");
        let events = self.events.clone();
        for console in [&mut self.on_test, &mut self.on_ansi] {
            console.live();
            console.snapshot(&events);
        }
        self.show();
    }

    /// The rows of the pane, on the test backend.
    fn pane_text(&mut self) -> String {
        let pane = self.test.get_frame().area();
        rows(self.test.backend().under().buffer())
            .lines()
            .skip(usize::from(pane.y))
            .take(usize::from(pane.height))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn pane(&mut self) -> Rect {
        self.test.get_frame().area()
    }

    fn close(&mut self) -> String {
        self.on_test.close(&mut self.test).unwrap();
        self.on_ansi.close(&mut self.ansi).unwrap();
        let backend = self.test.backend().under();
        format!("{}\n{}", rows(backend.scrollback()), rows(backend.buffer()))
    }
}

fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

const MARKDOWN: &str = "## Findings\n\n\
The pane has **three** parts; see [the spec](https://example.com/specs/008) for them.\n\n\
| Part | Rows | Notes |\n|---|---:|---|\n| status | 1 | seat and model |\n| input | 3 | two rules and `❯` |\n\n\
```rust\nfn main() {\n    println!(\"a line of code long enough to wrap in a pane of sixty columns\");\n}\n```\n\n\
- top item\n  - nested item\n    - third level\n- [x] a done task\n- [ ] an open task\n";

fn call(pair: &mut Pair, id: &str, kind: &str, input: serde_json::Value, end: serde_json::Value) {
    let mut acp = json!({"toolCallId": id, "title": kind, "kind": kind, "rawInput": input});
    pair.event(
        "pre_tool_use",
        id,
        json!({"tool_name": kind, "acp": acp.clone()}),
    );
    for (key, value) in end.as_object().unwrap() {
        acp[key] = value.clone();
    }
    pair.event(
        "post_tool_use",
        &format!("{id}-end"),
        json!({"tool_name": kind, "acp": acp}),
    );
}

/// Draw the whole scenario on a terminal of `width` by `height`, and answer
/// with everything the terminal has shown once the console closed.
fn scenario(width: u16, height: u16) -> String {
    let mut pair = Pair::new(width, height);

    pair.event("session_start", "session started", json!({}));
    let briefing: String = (1..=10).map(|n| format!("briefing line {n}\n")).collect();
    pair.event(
        "user_prompt_submit",
        "briefing",
        json!({"text": briefing, "source": "daemon"}),
    );
    pair.event("stop", "stop", json!({"stop_reason": "end_turn"}));

    // A typed prompt, and the turn it starts.
    pair.typed("Check the pane");
    assert_eq!(
        pair.key(with(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Send("Check the pane".into())
    );
    pair.event(
        "user_prompt_submit",
        "Check the pane",
        json!({"text": "Check the pane", "source": "console"}),
    );
    pair.event(
        "agent_thought_chunk",
        "thought",
        json!({"text": "I read the spec first, then I run the tests at each size."}),
    );
    pair.event(
        "plan",
        "plan",
        json!({"entries": [
            {"content": "Read spec 008", "status": "completed"},
            {"content": "Run the tests at three sizes and compare what each shows", "status": "in_progress"},
            {"content": "Write the report", "status": "pending"}
        ]}),
    );
    call(
        &mut pair,
        "run",
        "execute",
        json!({"command": "cargo nextest run -p ariadne-console"}),
        json!({"status": "completed", "rawOutput": {"stdout": "one\ntwo\nthree\nfour\nfive\nsix\n"}}),
    );
    call(
        &mut pair,
        "read",
        "read",
        json!({"file_path": "src/theme.rs", "line": 10}),
        json!({"status": "completed", "rawOutput": "pub const USER: Style"}),
    );
    call(
        &mut pair,
        "edit",
        "edit",
        json!({"file_path": "src/picker.rs"}),
        json!({"status": "completed", "content": [{"type": "diff", "path": "src/picker.rs",
               "oldText": "let a = 1;\nlet b = 2;\n", "newText": "let a = 1;\nlet b = 20;\n"}]}),
    );
    call(
        &mut pair,
        "search",
        "search",
        json!({"pattern": "fn picker", "path": "src"}),
        json!({"status": "completed", "rawOutput": "src/picker.rs:18"}),
    );
    call(
        &mut pair,
        "fetch",
        "fetch",
        json!({"url": "https://example.com/a/long/path/index.html"}),
        json!({"status": "completed", "rawOutput": "200 OK"}),
    );
    call(
        &mut pair,
        "delete",
        "delete",
        json!({"path": "tmp/old.txt"}),
        json!({"status": "completed"}),
    );
    call(
        &mut pair,
        "move",
        "move",
        json!({"path": "a.txt", "new_path": "b.txt"}),
        json!({"status": "completed"}),
    );
    call(
        &mut pair,
        "think",
        "think",
        json!({}),
        json!({"status": "completed"}),
    );
    call(
        &mut pair,
        "switch",
        "switch_mode",
        json!({}),
        json!({"status": "completed"}),
    );
    call(
        &mut pair,
        "other",
        "other",
        json!({}),
        json!({"status": "completed"}),
    );
    call(
        &mut pair,
        "fails",
        "execute",
        json!({"command": "false"}),
        json!({"status": "failed", "rawOutput": {"stdout": "", "stderr": "exit status 1"}}),
    );
    pair.event("agent_message_chunk", "text", json!({"text": MARKDOWN}));
    pair.event("agent_message", "whole", json!({"text": MARKDOWN}));

    // A permission question, and its answer.
    pair.event(
        "permission_request",
        "Allow this edit?",
        json!({"tool_name": "Allow this edit?",
               "acp": {"toolCallId": "ask", "kind": "edit",
                       "rawInput": {"file_path": "src/main.rs"},
                       "content": [{"type": "diff", "path": "src/main.rs",
                                    "oldText": "fn a() {}\n", "newText": "fn b() {}\n"}]},
               "options": [{"optionId": "once", "name": "Allow once"},
                           {"optionId": "always", "name": "Allow always"},
                           {"optionId": "no", "name": "Reject"}]}),
    );
    let asking = pair.pane_text();
    for shown in [
        "permission",
        "Allow this edit?",
        "1. Allow once",
        "2. Allow always",
        "3. Reject",
    ] {
        assert!(asking.contains(shown), "{shown} is on the screen: {asking}");
    }
    pair.key(with(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        pair.key(with(KeyCode::Enter, KeyModifiers::NONE)),
        Action::Send("always".into())
    );
    pair.event(
        "permission.replied",
        "answered",
        json!({"option_id": "always"}),
    );
    pair.event("stop", "stop", json!({"stop_reason": "end_turn"}));

    // A turn that runs a long call: a prompt typed meanwhile is queued, the
    // terminal is resized both ways, and the turn is cancelled.
    pair.typed("Run the long task");
    pair.key(with(KeyCode::Enter, KeyModifiers::NONE));
    pair.event(
        "user_prompt_submit",
        "Run the long task",
        json!({"text": "Run the long task", "source": "console"}),
    );
    pair.event(
        "pre_tool_use",
        "long",
        json!({"tool_name": "Bash", "acp": {"toolCallId": "long", "title": "Bash",
               "kind": "execute", "status": "in_progress", "rawInput": {"command": "sleep 600"}}}),
    );
    pair.typed("then this");
    pair.key(with(KeyCode::Enter, KeyModifiers::NONE));
    let queued = pair.pane_text();
    assert!(queued.contains("▌❯ then this  queued"), "{queued}");
    assert!(queued.contains("running Bash"), "{queued}");
    pair.resize(width + 20, height + 10);
    pair.resize(width, height);
    assert_eq!(
        pair.key(with(KeyCode::Esc, KeyModifiers::NONE)),
        Action::Cancel
    );
    pair.event("stop", "cancelled", json!({"stop_reason": "cancelled"}));
    pair.event(
        "user_prompt_submit",
        "then this",
        json!({"text": "then this", "source": "console"}),
    );
    pair.event("agent_message", "done", json!({"text": "Done."}));
    pair.event("stop", "stop", json!({"stop_reason": "end_turn"}));

    // The newline keys add a line each and post nothing, and the cursor
    // is after the last character typed.
    pair.typed("a");
    for key in [
        with(KeyCode::Enter, KeyModifiers::SHIFT),
        with(KeyCode::Enter, KeyModifiers::ALT),
        with(KeyCode::Char('j'), KeyModifiers::CONTROL),
    ] {
        assert_eq!(pair.key(key), Action::None);
        pair.typed("b");
    }
    pair.typed("\\");
    assert_eq!(
        pair.key(with(KeyCode::Enter, KeyModifiers::NONE)),
        Action::None
    );
    pair.typed("cd");
    let pane = pair.pane();
    let boxed: Vec<String> = pair
        .pane_text()
        .lines()
        .skip(usize::from(pane.height) - 6)
        .take(4)
        .map(str::to_string)
        .collect();
    assert_eq!(
        boxed,
        ["  b", "  b", "  b", "  cd"],
        "the box scrolled to its last rows"
    );
    let cursor = pair
        .test
        .backend_mut()
        .under_mut()
        .get_cursor_position()
        .unwrap();
    assert_eq!(
        cursor,
        Position {
            x: 4,
            y: pane.bottom() - 3
        },
        "the cursor is after `cd`"
    );
    for _ in 0..5 {
        pair.key(ctrl('u'));
        pair.key(with(KeyCode::Backspace, KeyModifiers::NONE));
    }

    // A post the daemon refuses, a dropped stream and the session's end.
    pair.typed("one more");
    pair.key(with(KeyCode::Enter, KeyModifiers::NONE));
    pair.refused("409 Conflict: the session takes no more input");
    pair.reconnect();
    pair.event("session_end", "session ended", json!({}));
    pair.close()
}

/// Where each block starts, in the order the scenario writes them.
const HEADS: &[&str] = &[
    "  session started",
    "» daemon",
    "▌❯ Check the pane",
    "· I read the spec",
    "plan 1/3",
    "✓ $ cargo nextest run -p ariadne-console",
    "✓ ≡ src/theme.rs:10",
    "✓ ✎ src/picker.rs",
    "✓ ⌕ fn picker in src",
    "✓ ⇣ https://example.com/a/long/path/index.html",
    "✓ ⌫ tmp/old.txt",
    "✓ → a.txt",
    "✓ ∴ think",
    "✓ ⇄ switch_mode",
    "✓ ◇ other",
    "✗ $ false",
    "● ## Findings",
    "Allow this edit?",
    "▌❯ Run the long task",
    "◐ $ sleep 600",
    "  turn cancelled",
    "▌❯ then this",
    "● Done.",
    "▌❯ one more",
    "✗ 409 Conflict",
    "  session ended",
];

/// What a row of the scrollback may start with at its first column: a mark
/// that starts a block, the banner's frame, the question of a picker, and
/// the answer under it. Every other row is under a mark, two columns in.
const MARKS: &[&str] = &[
    "▌",
    "●",
    "·",
    "»",
    "✓",
    "✗",
    "◐",
    "○",
    "plan ",
    "Allow this edit?",
    "↳ ",
    "╭",
    "│",
    "╰",
];

fn check(width: u16, height: u16) {
    let shown = scenario(width, height);
    // The rows the closed pane left blank under the last block.
    let shown = shown.trim_matches('\n');
    let rows: Vec<&str> = shown.lines().collect();

    let mut last = 0;
    for head in HEADS {
        let at: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.starts_with(head))
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            at.len(),
            1,
            "{head:?} is in the scrollback once at {width}x{height}:\n{shown}"
        );
        assert!(
            at[0] > last,
            "{head:?} comes in order at {width}x{height}:\n{shown}"
        );
        assert_eq!(
            rows[at[0] - 1],
            "",
            "one blank line comes before {head:?} at {width}x{height}:\n{shown}"
        );
        last = at[0];
    }
    for row in &rows {
        assert!(
            row.is_empty()
                || row.starts_with("  ")
                || MARKS.iter().any(|mark| row.starts_with(mark)),
            "{row:?} starts at a mark or two columns in at {width}x{height}:\n{shown}"
        );
        assert!(row.width() <= usize::from(width), "{row:?} fits {width}");
    }
    for gone in ["queued", "permission ─", "reconnecting", "enter send"] {
        assert!(
            !shown.contains(gone),
            "{gone:?} left no cell behind at {width}x{height}:\n{shown}"
        );
    }
    assert!(
        rows.windows(2)
            .all(|pair| !(pair[0].is_empty() && pair[1].is_empty())),
        "no two blank rows follow each other at {width}x{height}:\n{shown}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_whole_pane_draws_at_60_by_20() {
    check(60, 20);
}

#[tokio::test(start_paused = true)]
async fn the_whole_pane_draws_at_80_by_24() {
    check(80, 24);
}

#[tokio::test(start_paused = true)]
async fn the_whole_pane_draws_at_120_by_40() {
    check(120, 40);
}
