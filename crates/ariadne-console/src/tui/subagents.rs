//! Subagents in the pane (008): a bar over the status row that lists the
//! session's own view and one view per child the agent announced, the keys
//! that switch between them, and the line the parent view gives each
//! announcement and each end.
//!
//! The daemon stores a child's events on the parent session, each with a
//! `subagent {id, title}` field, and every announcement and state change as
//! one `subagent_update` (021). A child's events go to the child's view and
//! never to the parent's. A child's permission question is the exception:
//! the session answers it, so it is asked in the parent view, and the bar
//! marks the child that asks.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use serde_json::Value;

use ariadne_api::events::AgentEventDto;

use crate::theme::{AGENT, ASK, DIM, FAIL, SEPARATOR, TOOL};
use crate::transcript::{ItemMeta, TranscriptItem};

use super::{Action, Console, absorb, chrome};

/// The stored kind of every announcement and state change.
pub(super) const SUBAGENT_UPDATE: &str = "subagent_update";

/// What the bar calls the session's own view.
const MAIN: &str = "main";

/// One child the agent announced, in the order it was announced.
#[derive(Debug, Clone)]
pub(super) struct Subagent {
    id: String,
    title: String,
    /// `running`, `completed`, `failed` or `cancelled`, or none where the
    /// agent said none.
    pub(super) state: Option<String>,
    /// When it was announced: what an end counts its time from.
    announced_at: String,
    /// The child's own transcript. Empty while it is the view on the
    /// screen, whose blocks are the console's own `items`.
    pub(super) items: Vec<TranscriptItem>,
}

impl Subagent {
    fn running(&self) -> bool {
        self.state.as_deref() == Some("running")
    }
}

/// The child id an event's payload names, where it names one.
pub(super) fn child_of(event: &AgentEventDto) -> Option<&str> {
    event
        .payload
        .pointer("/subagent/id")
        .and_then(Value::as_str)
}

/// The child `event` is from, by its place in `subagents`: one the console
/// has not heard announced is added, under the title the event gives it.
pub(super) fn child_at(subagents: &mut Vec<Subagent>, event: &AgentEventDto, id: &str) -> usize {
    if let Some(at) = subagents.iter().position(|child| child.id == id) {
        return at;
    }
    subagents.push(Subagent {
        id: id.to_string(),
        title: title_of(&event.payload, id),
        state: None,
        announced_at: event.created_at.clone(),
        items: Vec::new(),
    });
    subagents.len() - 1
}

fn title_of(payload: &Value, id: &str) -> String {
    payload
        .pointer("/subagent/title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// Fold one `subagent_update` into the children, and into `parent` the line
/// it is worth there: one for an announcement, with the title and the task,
/// and one for an end, with the time since the announcement.
pub(super) fn follow(
    subagents: &mut Vec<Subagent>,
    parent: &mut Vec<TranscriptItem>,
    event: &AgentEventDto,
) {
    let Some(id) = child_of(event) else {
        return;
    };
    let known = subagents.iter().any(|child| child.id == id);
    let at = child_at(subagents, event, id);
    let child = &mut subagents[at];
    child.title = title_of(&event.payload, id);
    let state = event
        .payload
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_string);
    let note = |text: String| TranscriptItem::SystemNote {
        meta: ItemMeta::from_event(event),
        text,
    };
    if !known {
        let task = event
            .payload
            .pointer("/subagent/description")
            .and_then(Value::as_str)
            .filter(|task| !task.is_empty());
        parent.push(note(match task {
            Some(task) => format!("{} subagent {}{SEPARATOR}{task}", glyph(None), child.title),
            None => format!("{} subagent {}", glyph(None), child.title),
        }));
    }
    // A first update can carry an end already: it is the announcement and
    // the end at once, and gets both lines.
    if state != child.state
        && matches!(state.as_deref(), Some("completed" | "failed" | "cancelled"))
    {
        let mut text = format!(
            "{} subagent {} {}",
            glyph(state.as_deref()),
            child.title,
            state.as_deref().unwrap_or_default()
        );
        if let Some(elapsed) = elapsed(&child.announced_at, &event.created_at) {
            text.push_str(SEPARATOR);
            text.push_str(&chrome::clock(elapsed));
        }
        parent.push(note(text));
    }
    child.state = state;
}

/// The time from one stored instant to a later one, where both read.
fn elapsed(from: &str, to: &str) -> Option<std::time::Duration> {
    let from = chrono::DateTime::parse_from_rfc3339(from).ok()?;
    let to = chrono::DateTime::parse_from_rfc3339(to).ok()?;
    (to - from).to_std().ok()
}

/// The mark a child's state reads as, in the bar and in the parent's lines.
fn glyph(state: Option<&str>) -> &'static str {
    match state {
        Some("completed") => "✓",
        Some("failed") => "✗",
        Some("cancelled") => "⊘",
        Some("running") => "●",
        _ => "◇",
    }
}

impl Console {
    /// Whether a child's view is on the screen.
    pub(super) fn in_child(&self) -> bool {
        self.view > 0
    }

    /// How many children are running now.
    pub(super) fn running_subagents(&self) -> usize {
        self.subagents
            .iter()
            .filter(|child| child.running())
            .count()
    }

    /// Whether the view on the screen is being written: the parent's turn
    /// runs, or the child shown runs.
    pub(super) fn view_running(&self) -> bool {
        match self.view {
            0 => self.turn.running(),
            view => self.subagents[view - 1].running(),
        }
    }

    /// Fold one child's event into that child's transcript.
    pub(super) fn follow_child(&mut self, event: &AgentEventDto, id: &str) {
        let at = child_at(&mut self.subagents, event, id);
        let items = match self.view == at + 1 {
            true => &mut self.items,
            false => &mut self.subagents[at].items,
        };
        absorb(items, event);
    }

    /// Fold one `subagent_update`, and the line it gives the parent view.
    pub(super) fn follow_subagent(&mut self, event: &AgentEventDto) {
        let parent = match self.view {
            0 => &mut self.items,
            _ => &mut self.parked,
        };
        follow(&mut self.subagents, parent, event);
    }

    /// Track which child a pending permission question is from: the
    /// session has one at a time, and its answer ends it.
    pub(super) fn follow_asking(&mut self, event: &AgentEventDto) {
        match event.kind.as_str() {
            "permission_request" => self.asking_child = child_of(event).map(str::to_string),
            "permission.replied" => self.asking_child = None,
            _ => {}
        }
    }

    /// The keys of the bar: Tab to the next view and Shift-Tab to the one
    /// before, in bar order, and Esc from a child's view to the parent's.
    /// `None` for any other key, and while the session has no child.
    pub(super) fn view_key(&mut self, key: crossterm::event::KeyEvent) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};
        if self.subagents.is_empty() {
            return None;
        }
        let views = self.subagents.len() + 1;
        let back = key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT));
        let to = match key.code {
            KeyCode::Tab | KeyCode::BackTab if back => (self.view + views - 1) % views,
            KeyCode::Tab => (self.view + 1) % views,
            KeyCode::Esc if self.in_child() => 0,
            _ => return None,
        };
        self.switch(to);
        Some(Action::Switch)
    }

    /// Put view `to` on the screen: the one shown goes back where it is
    /// kept, and `to`'s blocks become the console's own. The loop then
    /// clears the terminal and draws it whole.
    pub(super) fn switch(&mut self, to: usize) {
        if to == self.view {
            return;
        }
        let shown = std::mem::take(&mut self.items);
        match self.view {
            0 => self.parked = shown,
            view => self.subagents[view - 1].items = shown,
        }
        self.items = match to {
            0 => std::mem::take(&mut self.parked),
            view => std::mem::take(&mut self.subagents[view - 1].items),
        };
        self.view = to;
        self.committed = 0;
        self.head.clear();
        self.invalidate_blocks();
    }

    /// The bar: the parent and each child in the order they were announced,
    /// a child with the mark of its state and `?` while its question waits,
    /// the view on the screen reversed. None while the session has no child.
    pub(super) fn bar(&self) -> Option<Line<'static>> {
        if self.subagents.is_empty() {
            return None;
        }
        let tab = |at: usize, spans: Vec<Span<'static>>| {
            let shown = at == self.view;
            spans
                .into_iter()
                .map(move |span| match shown {
                    true => span.patch_style(
                        ratatui::style::Style::new()
                            .add_modifier(Modifier::REVERSED | Modifier::BOLD),
                    ),
                    false => span,
                })
                .collect::<Vec<_>>()
        };
        let mut spans = vec![Span::raw(" ")];
        spans.extend(tab(0, vec![Span::raw(format!(" {MAIN} "))]));
        for (at, child) in self.subagents.iter().enumerate() {
            let style = match child.state.as_deref() {
                Some("running") => AGENT,
                Some("failed") => FAIL,
                Some("completed" | "cancelled") => DIM,
                _ => TOOL,
            };
            let mut parts = vec![
                Span::raw(" "),
                Span::styled(glyph(child.state.as_deref()), style),
                Span::raw(format!(" {}", child.title)),
            ];
            if self.asking_child.as_deref() == Some(child.id.as_str()) {
                parts.push(Span::styled(" ?", ASK));
            }
            parts.push(Span::raw(" "));
            spans.push(Span::styled(" ", DIM));
            spans.extend(tab(at + 1, parts));
        }
        Some(Line::from(spans))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use futures_util::stream;
    use serde_json::json;

    use crate::ansi::{AnsiBackend, Window};
    use crate::tui::testing::*;
    use crate::tui::*;

    /// A `subagent_update` as the daemon stores it (021), at `at`.
    fn update(id: &str, title: &str, task: &str, state: &str, at: &str) -> AgentEventDto {
        event_at(
            "subagent_update",
            &format!("{id} {state}"),
            json!({"session_id": "stub-session",
                   "subagent": {"id": id, "title": title, "description": task},
                   "state": state, "capabilities": {"cancel": false}}),
            at,
        )
    }

    /// An event of the child `id`, as the daemon stores it.
    fn of_child(id: &str, title: &str, mut event: AgentEventDto) -> AgentEventDto {
        event.payload["subagent"] = json!({"id": id, "title": title});
        event.id = format!("{}-{id}", event.id);
        event
    }

    fn said(text: &str) -> AgentEventDto {
        event("agent_message", text, json!({"text": text}))
    }

    const AT: &str = "2026-09-12T00:00:00Z";

    /// A session whose agent started two children: `explorer` still runs
    /// and asks a question, `reviewer` ended a minute and four seconds in.
    fn two_children() -> Vec<AgentEventDto> {
        vec![
            said("parent words"),
            update("c1", "explorer", "Find the config loader", "running", AT),
            of_child("c1", "explorer", said("explorer words")),
            update("c2", "reviewer", "Review the diff", "running", AT),
            of_child("c2", "reviewer", said("reviewer words")),
            update(
                "c2",
                "reviewer",
                "Review the diff",
                "completed",
                "2026-09-12T00:01:04Z",
            ),
            of_child(
                "c1",
                "explorer",
                event(
                    "permission_request",
                    "ask",
                    json!({"tool_name": "Bash", "options": [
                        {"optionId": "no", "name": "Reject"},
                        {"optionId": "yes", "name": "Allow"}]}),
                ),
            ),
        ]
    }

    /// Everything a fresh pane shows of `console`: what went to its
    /// scrollback, then its screen.
    fn drawn(console: &mut Console) -> String {
        let mut terminal = pane();
        console.redraw(&mut terminal).unwrap();
        shown(&terminal)
    }

    fn tab(console: &mut Console) -> Action {
        console.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
    }

    fn back_tab(console: &mut Console) -> Action {
        console.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
    }

    fn esc(console: &mut Console) -> Action {
        console.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
    }

    /// The row right over the status row.
    fn over_status(shown: &str) -> String {
        let rows: Vec<&str> = shown.lines().collect();
        let status = rows
            .iter()
            .position(|row| row.starts_with(" author · "))
            .unwrap_or_else(|| panic!("a status row: {shown}"));
        rows[status - 1].to_string()
    }

    /// The bar is over the status row: the parent, then each child in the
    /// order it was announced, with the mark of its state, and `?` on the
    /// child whose question waits.
    #[test]
    fn the_bar_lists_the_parent_and_each_child_in_announcement_order() {
        let mut console = Console::new(header());
        console.snapshot(&two_children());

        let bar = over_status(&drawn(&mut console));

        assert_eq!(bar, "  main   ● explorer ?   ✓ reviewer");
    }

    /// A session whose agent announces no child, as OpenCode's, has no bar,
    /// and Tab goes to the box as it always did.
    #[test]
    fn a_session_without_a_child_has_no_bar_and_tab_switches_nothing() {
        let mut console = Console::new(header());
        console.snapshot(&[said("parent words")]);
        let before = drawn(&mut console);

        assert_eq!(tab(&mut console), Action::None);
        let shown = drawn(&mut console);

        assert!(!shown.contains("main"), "{shown}");
        assert_eq!(shown, before);
        assert_eq!(
            console.pinned_rows(72),
            Console::new(header()).pinned_rows(72)
        );
    }

    /// Tab moves to the next view and Shift-Tab to the one before, in bar
    /// order, and both wrap around.
    #[test]
    fn tab_and_shift_tab_walk_the_views_in_bar_order() {
        let mut console = Console::new(header());
        console.snapshot(&two_children());
        let shown_text = |console: &mut Console| {
            let shown = drawn(console);
            ["parent words", "explorer words", "reviewer words"]
                .into_iter()
                .filter(|words| shown.contains(words))
                .collect::<Vec<_>>()
        };

        assert_eq!(shown_text(&mut console), ["parent words"]);
        assert_eq!(tab(&mut console), Action::Switch);
        assert_eq!(shown_text(&mut console), ["explorer words"]);
        assert_eq!(tab(&mut console), Action::Switch);
        assert_eq!(shown_text(&mut console), ["reviewer words"]);
        assert_eq!(tab(&mut console), Action::Switch);
        assert_eq!(shown_text(&mut console), ["parent words"]);
        assert_eq!(back_tab(&mut console), Action::Switch);
        assert_eq!(shown_text(&mut console), ["reviewer words"]);
    }

    /// While the slash-command suggestion list is open, Tab completes the
    /// pick, as it does in a session without a child.
    #[test]
    fn tab_completes_while_the_suggestion_list_is_open() {
        let mut console = Console::new(header());
        let mut events = two_children();
        events.truncate(6);
        events.push(event(
            "available_commands_update",
            "commands",
            json!({"available_commands": [{"name": "review", "description": "Review"}]}),
        ));
        console.snapshot(&events);
        type_into(&mut console, "/re");

        assert_eq!(tab(&mut console), Action::None);
        let shown = drawn(&mut console);

        assert!(shown.contains("❯ /review"), "{shown}");
        assert!(shown.contains("parent words"), "{shown}");
    }

    /// A child's view shows the child's events alone, has no box, says
    /// read-only, takes no typing, folds with Ctrl-O, and Esc leaves it for
    /// the parent's view.
    #[test]
    fn a_childs_view_is_read_only_and_esc_returns_to_the_parent() {
        let mut console = Console::new(header());
        console.snapshot(&two_children());
        tab(&mut console);

        type_into(&mut console, "hello");
        assert_eq!(enter(&mut console), Action::None);
        let shown = drawn(&mut console);

        assert!(shown.contains("explorer words"), "{shown}");
        assert!(!shown.contains("parent words"), "{shown}");
        assert!(!shown.contains("Tell the agent"), "no box: {shown}");
        assert!(!shown.contains("hello"), "{shown}");
        let footer = shown.lines().last().unwrap_or_default().to_string();
        assert!(footer.starts_with(" read-only"), "{footer:?}");
        assert!(footer.contains("ctrl-o unfold"), "{footer:?}");

        console.key(ctrl('o'));
        let folded = drawn(&mut console);
        assert!(
            folded.lines().last().unwrap().contains("ctrl-o fold"),
            "{folded}"
        );

        assert_eq!(esc(&mut console), Action::Switch);
        let shown = drawn(&mut console);
        assert!(shown.contains("parent words"), "{shown}");
        assert!(shown.contains("Tell the agent"), "{shown}");
    }

    /// The parent's view gives each child one line as it is announced,
    /// with its title and task, and one as it ends, with the time it took.
    /// The child's own words are not among them, and its question is asked
    /// here, naming the child.
    #[test]
    fn the_parent_view_has_a_line_per_announcement_and_per_end() {
        let mut console = Console::new(header());
        console.snapshot(&two_children());

        let shown = drawn(&mut console);

        assert!(
            shown.contains("◇ subagent explorer · Find the config loader"),
            "{shown}"
        );
        assert!(
            shown.contains("◇ subagent reviewer · Review the diff"),
            "{shown}"
        );
        assert!(
            shown.contains("✓ subagent reviewer completed · 1m 04s"),
            "{shown}"
        );
        assert!(!shown.contains("explorer words"), "{shown}");
        assert!(!shown.contains("reviewer words"), "{shown}");
        assert!(
            shown.contains("Bash · asked by subagent explorer"),
            "{shown}"
        );
    }

    /// A first update can already carry an end, as the draft shape lets
    /// it: the parent view gives that child its announcement and its end.
    #[test]
    fn a_child_first_heard_of_at_its_end_has_both_lines() {
        let mut console = Console::new(header());
        console.snapshot(&[update("c3", "linter", "Lint the tree", "failed", AT)]);
        let mut live = Console::new(header());
        live.apply(&update("c3", "linter", "Lint the tree", "cancelled", AT));

        let shown = drawn(&mut console);
        assert!(
            shown.contains("◇ subagent linter · Lint the tree"),
            "{shown}"
        );
        assert!(shown.contains("✗ subagent linter failed · 0s"), "{shown}");
        let shown = drawn(&mut live);
        assert!(
            shown.contains("◇ subagent linter · Lint the tree"),
            "{shown}"
        );
        assert!(
            shown.contains("⊘ subagent linter cancelled · 0s"),
            "{shown}"
        );
    }

    /// A child's slash commands are not the session's: the parent's list
    /// stays the one the suggestion list offers.
    #[test]
    fn a_childs_commands_leave_the_parents_list_alone() {
        let listed = |name: &str| {
            event(
                "available_commands_update",
                name,
                json!({"available_commands": [{"name": name, "description": name}]}),
            )
        };
        let mut console = Console::new(header());
        console.snapshot(&[
            update("c1", "explorer", "", "running", AT),
            listed("review"),
            of_child("c1", "explorer", listed("explore")),
        ]);
        console.apply(&of_child("c1", "explorer", listed("explore")));
        type_into(&mut console, "/");

        let shown = drawn(&mut console);
        assert!(shown.contains("/review"), "{shown}");
        assert!(!shown.contains("/explore"), "{shown}");
    }

    /// The status row counts the children that run.
    #[test]
    fn the_status_row_counts_the_running_subagents() {
        let mut console = Console::new(header());
        let mut events = two_children();
        events.truncate(4);
        console.snapshot(&events);
        assert!(drawn(&mut console).contains("running 2 subagents"));

        console.apply(&update("c1", "explorer", "", "failed", AT));
        assert!(drawn(&mut console).contains("running 1 subagent"));

        console.apply(&update("c2", "reviewer", "", "cancelled", AT));
        let shown = drawn(&mut console);
        assert!(
            !shown.contains("running 1") && !shown.contains("running 2"),
            "{shown}"
        );
    }

    /// Events that arrive while a child's view is on the screen land in
    /// their own views: the parent's text in the parent's, a child's in
    /// that child's.
    #[test]
    fn live_events_land_in_their_own_views_whichever_is_shown() {
        let mut console = Console::new(header());
        console.snapshot(&two_children());
        tab(&mut console);

        console.apply(&said("more parent words"));
        console.apply(&of_child("c1", "explorer", said("more explorer words")));
        console.apply(&of_child("c2", "reviewer", said("more reviewer words")));
        let child = drawn(&mut console);
        esc(&mut console);
        let parent = drawn(&mut console);

        assert!(child.contains("more explorer words"), "{child}");
        assert!(!child.contains("more parent words"), "{child}");
        assert!(!child.contains("more reviewer words"), "{child}");
        assert!(parent.contains("more parent words"), "{parent}");
        assert!(!parent.contains("more explorer words"), "{parent}");
    }

    /// A switch clears the terminal, its scrollback too where it can, and
    /// draws the chosen view whole: the first thing written after the key
    /// is the clear, and what follows is that view alone.
    #[tokio::test(start_paused = true)]
    async fn a_switch_clears_the_terminal_and_draws_the_view_whole() {
        let tap = Tap::default();
        let window = Window::new(72, 40);
        let mut terminal = open(|| AnsiBackend::new(tap.clone(), window.clone())).unwrap();
        let mut console = Console::new(header());
        let mut stub = Stub::new(two_children());
        let source = stub.source();
        let (typing, typed) = tokio::sync::mpsc::unbounded_channel();
        let keys = stream::unfold(typed, |mut typed| async move {
            typed.recv().await.map(|key| (Ok(key), typed))
        });
        let after = |written: &[u8]| {
            let clear = b"\x1b[2J\x1b[3J";
            let at = written
                .windows(clear.len())
                .rposition(|bytes| bytes == clear)
                .expect("the switch cleared the terminal, scrollback included");
            let mut emulator = vt100::Parser::new(40, 72, 200);
            emulator.process(&written[at..]);
            emulator.screen().contents()
        };
        let script = async {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            tap.take();
            typing.send(key(KeyCode::Tab)).unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let child = after(&tap.0.lock().unwrap());
            typing.send(key(KeyCode::Esc)).unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let parent = after(&tap.0.lock().unwrap());
            typing.send(TermEvent::Key(ctrl('c'))).unwrap();
            typing.send(TermEvent::Key(ctrl('c'))).unwrap();
            (child, parent)
        };
        let (outcome, (child, parent)) = tokio::join!(
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

        assert!(child.contains("explorer words"), "{child}");
        assert!(!child.contains("parent words"), "{child}");
        assert!(child.contains("read-only"), "{child}");
        assert!(parent.contains("parent words"), "{parent}");
        assert!(!parent.contains("explorer words"), "{parent}");
    }
}
