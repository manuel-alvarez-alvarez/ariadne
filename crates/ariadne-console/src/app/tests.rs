//! The full-screen console at its public seams: frames into [`App::frame`],
//! terminal events into [`App::event`] and [`drive`], and what
//! [`App::render`] draws on ratatui's test backend and, byte for byte, on the
//! ANSI backend read back by a terminal emulator.

use std::sync::{Arc, Mutex};

use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use futures_util::{StreamExt, stream};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use serde_json::{Value, json};

use ariadne_api::sessions::{AnswerQuestionRequest, QuestionAction, QuestionDto};
use ariadne_api::trajectory::{StepDto, ToolCallDetailDto};

use super::*;
use crate::ansi::{AnsiBackend, Window};
use crate::app::draw::KEYS;
use crate::testing::{Tap, emulator, resize};
use crate::trajectory::fixtures::{snapshot, step, turn, update};

const WIDTH: u16 = 60;
const HEIGHT: u16 = 24;

fn key(code: KeyCode) -> TermEvent {
    TermEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(character: char) -> TermEvent {
    TermEvent::Key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::CONTROL,
    ))
}

fn click(row: u16) -> TermEvent {
    TermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn wheel(kind: MouseEventKind) -> TermEvent {
    TermEvent::Mouse(MouseEvent {
        kind,
        column: 4,
        row: 2,
        modifiers: KeyModifiers::NONE,
    })
}

fn typed(app: &mut App, text: &str) {
    for character in text.chars() {
        app.event(key(KeyCode::Char(character)));
    }
}

/// A completed turn with `prompt` and `answer`, `steps` steps and one tool
/// call that took two seconds.
fn done(id: &str, prompt: &str, answer: &str) -> Value {
    turn(
        id,
        "completed",
        json!({
            "prompt": {"id": id, "source": "console", "prompt_kind": null,
                       "content": [{"type": "text", "text": prompt}]},
            "ended_at": "2026-11-27T10:00:02Z",
            "activity": {"steps": 2, "tool_calls": 1, "files_changed": 0, "failed": 0,
                         "cancelled": 0, "thoughts": 0, "subagents": 0},
            "answer": [{"type": "text", "text": answer}]
        }),
    )
}

fn running(id: &str, prompt: &str, steps: Vec<Value>) -> Value {
    turn(
        id,
        "running",
        json!({
            "prompt": {"id": id, "source": "console", "prompt_kind": null,
                       "content": [{"type": "text", "text": prompt}]},
            "steps": steps,
        }),
    )
}

fn call(id: &str, turn: &str, title: &str, status: &str) -> Value {
    step(
        "tool_call",
        id,
        turn,
        json!({"title": title, "kind": "execute", "status": status,
               "subject": format!("{title} subject"),
               "started_at": "2026-11-27T10:00:00Z",
               "ended_at": if status == "running" { Value::Null } else { json!("2026-11-27T10:00:02Z") }}),
    )
}

fn snap(rev: u64, turns: Vec<Value>) -> Frame {
    Frame::Snapshot(Box::new(snapshot(rev, turns)), None)
}

fn app_with(turns: Vec<Value>) -> App {
    let mut app = App::new("s1");
    app.frame(snap(1, turns));
    app
}

fn draw(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    rows(terminal.backend().buffer())
}

fn rows(buffer: &Buffer) -> Vec<String> {
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            let mut row = String::new();
            let mut x = 0;
            while x < area.width {
                let cell = &buffer[(x, y)];
                row.push_str(cell.symbol());
                x += u16::try_from(unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1))
                    .unwrap();
            }
            row.trim_end().to_string()
        })
        .collect()
}

fn count(screen: &[String], text: &str) -> usize {
    screen.iter().filter(|row| row.contains(text)).count()
}

/// The steps the stub host reads back, by turn.
type Steps = Vec<(String, Vec<StepDto>)>;

/// The host, in memory: what was sent, answered, cancelled and copied, and
/// the steps and questions it reads back.
#[derive(Clone, Default)]
struct Stub {
    sent: Arc<Mutex<Vec<String>>>,
    answers: Arc<Mutex<Vec<(String, AnswerQuestionRequest)>>>,
    cancels: Arc<Mutex<usize>>,
    copies: Arc<Mutex<Vec<String>>>,
    steps: Arc<Mutex<Steps>>,
    /// The turns whose steps were read.
    step_reads: Arc<Mutex<Vec<String>>>,
    /// The steps of each subagent, by turn and subagent id.
    lanes: Vec<((String, String), Vec<StepDto>)>,
    /// The detail of each step, by step id.
    details: Vec<(String, ToolCallDetailDto)>,
    /// Each read of a detail: the step id and whether it asked for raw.
    detail_reads: Arc<Mutex<Vec<(String, bool)>>>,
    questions: Vec<QuestionDto>,
    refuses: bool,
}

impl Host for Stub {
    async fn send(&mut self, text: String) -> Result<(), String> {
        self.sent.lock().unwrap().push(text);
        match self.refuses {
            true => Err("the session takes no more input".into()),
            false => Ok(()),
        }
    }

    async fn answer(
        &mut self,
        question_id: &str,
        answer: AnswerQuestionRequest,
    ) -> Result<(), String> {
        self.answers
            .lock()
            .unwrap()
            .push((question_id.to_string(), answer));
        Ok(())
    }

    async fn cancel(&mut self) {
        *self.cancels.lock().unwrap() += 1;
    }

    async fn subagent_steps(
        &mut self,
        turn_id: &str,
        subagent_id: &str,
    ) -> Result<Vec<StepDto>, String> {
        self.lanes
            .iter()
            .find(|((turn, lane), _)| turn == turn_id && lane == subagent_id)
            .map(|(_, steps)| steps.clone())
            .ok_or_else(|| format!("no steps for {subagent_id}"))
    }

    async fn detail(&mut self, step_id: &str, raw: bool) -> Result<ToolCallDetailDto, String> {
        self.detail_reads
            .lock()
            .unwrap()
            .push((step_id.to_string(), raw));
        let mut detail = self
            .details
            .iter()
            .find(|(id, _)| id == step_id)
            .map(|(_, detail)| detail.clone())
            .ok_or_else(|| format!("no detail for {step_id}"))?;
        if !raw {
            detail.raw = None;
        }
        Ok(detail)
    }

    async fn steps(&mut self, turn_id: &str) -> Result<Vec<StepDto>, String> {
        self.step_reads.lock().unwrap().push(turn_id.to_string());
        self.steps
            .lock()
            .unwrap()
            .iter()
            .find(|(id, _)| id == turn_id)
            .map(|(_, steps)| steps.clone())
            .ok_or_else(|| format!("no steps for {turn_id}"))
    }

    async fn questions(&mut self) -> Result<Vec<QuestionDto>, String> {
        Ok(self.questions.clone())
    }

    fn copy(&mut self, text: String) {
        self.copies.lock().unwrap().push(text);
    }
}

/// Drive `app` on a test backend from `frames`, then `events`, then two
/// Ctrl-C, and give back what the last draw showed.
async fn run(
    app: &mut App,
    stub: &mut Stub,
    frames: Vec<Frame>,
    events: Vec<TermEvent>,
) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    let source = stream::iter(frames.into_iter().map(Ok)).chain(stream::pending());
    let events = stream::iter(events.into_iter().chain([ctrl('c'), ctrl('c')]).map(Ok))
        .chain(stream::pending());
    drive(&mut terminal, source, stub, Box::pin(events), app)
        .await
        .unwrap();
    rows(terminal.backend().buffer())
}

#[test]
fn a_turn_shows_its_prompt_one_activity_line_and_its_answer() {
    let app = app_with(vec![done("t1", "Run the tests", "All **tests** pass.")]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[0], "❯ Run the tests");
    assert_eq!(screen[1], "  ▸ ✓ 2 steps · 1 tool call · 2s");
    assert_eq!(screen[2], "  All tests pass.");
    assert_eq!(screen[3], "");
}

#[test]
fn a_running_turn_shows_its_last_steps_live() {
    let mut app = app_with(vec![running(
        "t1",
        "Build it",
        vec![
            call("c1", "t1", "First", "completed"),
            call("c2", "t1", "Second", "completed"),
            call("c3", "t1", "Third", "completed"),
            call("c4", "t1", "Fourth", "running"),
        ],
    )]);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "First"), 0, "{screen:#?}");
    assert!(
        screen[2].starts_with("    ✓ $ Second · Second subject  2s"),
        "{screen:#?}"
    );
    assert!(
        screen[4].starts_with("    ◐ $ Fourth · Fourth subject"),
        "{screen:#?}"
    );

    app.frame(Frame::Update(update(
        2,
        json!([{"op": "item_upsert", "step": call("c5", "t1", "Fifth", "running")}]),
    )));

    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "Second"), 0, "{screen:#?}");
    assert!(screen[4].contains("Fifth"), "{screen:#?}");
}

#[test]
fn a_running_turn_draws_the_whole_answer_and_live_steps() {
    let mut turn = running("t1", "Build it", vec![call("c1", "t1", "Work", "running")]);
    turn["answer"] = json!([{"type": "text", "text": "First paragraph.\n\nSecond paragraph."}]);
    let screen = draw(&app_with(vec![turn]), WIDTH, HEIGHT);
    assert_eq!(count(&screen, "First paragraph."), 1);
    assert_eq!(count(&screen, "Second paragraph."), 1);
    assert_eq!(count(&screen, "Work subject"), 1);
}

fn streaming_answer() -> Value {
    let mut turn = running(
        "t2",
        "Two",
        vec![step(
            "message",
            "m2",
            "t2",
            json!({"content": [{"type": "text", "text": "second answer"}], "streaming": true}),
        )],
    );
    turn["answer"] = json!([{"type": "text", "text": "second answer"}]);
    turn
}

#[test]
fn a_streamed_append_updates_the_live_answer() {
    let mut app = app_with(vec![streaming_answer()]);
    app.frame(Frame::Update(update(
        2,
        json!([{
            "op": "text_append", "turn_id": "t2", "item_id": "m2",
            "text": "\n\nAnother paragraph."
        }]),
    )));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "Another paragraph."), 1, "{screen:#?}");
}

#[tokio::test]
async fn an_open_running_turn_reads_all_steps_after_it_finishes() {
    let latest = call("c2", "t1", "Latest", "completed");
    let mut app = app_with(vec![running("t1", "Build it", vec![latest.clone()])]);
    app.event(ctrl('o'));
    let mut stub = Stub::default();
    stub.steps.lock().unwrap().push((
        "t1".into(),
        vec![
            serde_json::from_value(call("c1", "t1", "Earlier", "completed")).unwrap(),
            serde_json::from_value(latest).unwrap(),
        ],
    ));
    let closed = Frame::Update(update(
        2,
        json!([{
            "op": "turn_upsert", "turn": done("t1", "Build it", "Done.")
        }]),
    ));
    let screen = run(&mut app, &mut stub, vec![closed], vec![]).await;
    assert_eq!(count(&screen, "Earlier subject"), 1);
    assert_eq!(count(&screen, "Latest subject"), 1);
}

#[tokio::test]
async fn an_open_turn_shows_a_row_per_step_with_its_status_kind_subject_and_duration() {
    let mut app = App::new("s1");
    let mut stub = Stub::default();
    let steps: Vec<StepDto> = [
        call("c1", "t1", "Bash", "completed"),
        call("c2", "t1", "Edit", "failed"),
        step(
            "message",
            "m1",
            "t1",
            json!({"content": [{"type": "text", "text": "Done.\nMore."}]}),
        ),
    ]
    .into_iter()
    .map(|value| serde_json::from_value(value).unwrap())
    .collect();
    stub.steps.lock().unwrap().push(("t1".into(), steps));

    // Ctrl-O opens the newest turn: a closed one, whose steps the host reads.
    let screen = run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![done("t1", "Fix it", "Fixed.")])],
        vec![ctrl('o')],
    )
    .await;

    assert_eq!(screen[1], "  ▾ ✓ 2 steps · 1 tool call · 2s");
    assert_eq!(screen[2], "    ✓ $ Bash · Bash subject  2s");
    assert_eq!(screen[3], "    ✗ $ Edit · Edit subject  2s");
    assert_eq!(screen[4], "    ✓ message    Done.");
    assert_eq!(screen[5], "  Fixed.");
}

#[test]
fn ctrl_o_opens_and_closes_the_newest_turn() {
    let mut app = app_with(vec![
        done("t1", "Old", "a"),
        running("t2", "New", vec![call("c1", "t2", "Bash", "completed")]),
    ]);

    app.event(ctrl('o'));
    let open = draw(&app, WIDTH, HEIGHT);
    app.event(ctrl('o'));
    let closed = draw(&app, WIDTH, HEIGHT);

    assert!(open.iter().any(|row| row.starts_with("  ▾ ")), "{open:#?}");
    assert!(
        open.iter().any(|row| row.starts_with("  ▸ ✓")),
        "the old turn stays closed: {open:#?}"
    );
    assert!(
        !closed.iter().any(|row| row.starts_with("  ▾ ")),
        "{closed:#?}"
    );
}

#[test]
fn enter_sends_the_prompt_and_it_shows_queued_until_a_turn_holds_it() {
    let mut app = app_with(vec![done("t1", "First", "a")]);
    typed(&mut app, "Second");

    let action = app.event(key(KeyCode::Enter));
    let queued = draw(&app, WIDTH, HEIGHT);
    app.frame(Frame::Update(update(
        2,
        json!([{"op": "turn_upsert", "turn": running("t2", "Second", vec![])}]),
    )));
    let held = draw(&app, WIDTH, HEIGHT);

    assert_eq!(action, Action::Send("Second".into()));
    assert_eq!(count(&queued, "❯ Second"), 1, "{queued:#?}");
    assert_eq!(count(&queued, "queued"), 1, "{queued:#?}");
    assert_eq!(count(&held, "❯ Second"), 1, "{held:#?}");
    assert_eq!(count(&held, "queued"), 0, "{held:#?}");
}

#[tokio::test]
async fn a_refused_prompt_is_said_under_the_transcript_and_is_no_longer_queued() {
    let mut app = App::new("s1");
    let mut stub = Stub {
        refuses: true,
        ..Stub::default()
    };
    let mut events: Vec<TermEvent> = "Again".chars().map(|c| key(KeyCode::Char(c))).collect();
    events.push(key(KeyCode::Enter));

    let screen = run(&mut app, &mut stub, vec![snap(1, vec![])], events).await;

    assert_eq!(*stub.sent.lock().unwrap(), ["Again"]);
    assert_eq!(
        count(&screen, "✗ the session takes no more input"),
        1,
        "{screen:#?}"
    );
    assert_eq!(count(&screen, "queued"), 0, "{screen:#?}");
}

#[test]
fn up_and_down_move_through_the_rows_then_through_the_prompt_history() {
    let mut app = app_with(vec![
        done("t1", "first prompt", "a"),
        done("t2", "second prompt", "b"),
    ]);
    // Two rows in the box: Up goes to the first row before the history.
    typed(&mut app, "top");
    app.event(TermEvent::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::ALT,
    )));
    typed(&mut app, "bottom");

    app.event(key(KeyCode::Up));
    assert_eq!(
        app.input.joined(),
        "top\nbottom",
        "the first Up moves a row"
    );
    app.event(key(KeyCode::Up));
    assert_eq!(app.input.joined(), "second prompt");
    app.event(key(KeyCode::Up));
    assert_eq!(app.input.joined(), "first prompt");
    app.event(key(KeyCode::Down));
    app.event(key(KeyCode::Down));
    assert_eq!(app.input.joined(), "top\nbottom", "the draft comes back");
}

#[test]
fn tab_gives_the_keys_to_the_transcript_and_tab_escape_or_a_typed_character_gives_them_back() {
    let mut app = app_with(vec![done("t1", "One", "a")]);

    for back in [
        key(KeyCode::Tab),
        key(KeyCode::Esc),
        key(KeyCode::Char('x')),
    ] {
        app.event(key(KeyCode::Tab));
        assert_eq!(app.zone(), Zone::Transcript);
        let screen = draw(&app, WIDTH, HEIGHT);
        assert!(!screen.last().unwrap().contains("copy"), "{screen:#?}");
        app.event(back);
        assert_eq!(app.zone(), Zone::Input);
    }
    assert_eq!(app.input.joined(), "x", "the typed character is in the box");
}

#[test]
fn the_cursor_walks_turns_and_steps_with_the_arrows_and_j_and_k() {
    let mut app = app_with(vec![
        done("t1", "One", "a"),
        running(
            "t2",
            "Two",
            vec![
                call("c1", "t2", "Bash", "completed"),
                call("c2", "t2", "Read", "running"),
            ],
        ),
    ]);
    app.event(ctrl('o'));
    app.event(key(KeyCode::Tab));
    let at = |app: &App| app.cursor.clone();

    assert_eq!(at(&app), Some(Item::Turn("t2".into())));
    app.event(key(KeyCode::Down));
    assert_eq!(
        at(&app),
        Some(Item::Step {
            turn: "t2".into(),
            step: "c1".into()
        })
    );
    app.event(key(KeyCode::Char('j')));
    assert_eq!(
        at(&app),
        Some(Item::Step {
            turn: "t2".into(),
            step: "c2".into()
        })
    );
    app.event(key(KeyCode::Char('k')));
    app.event(key(KeyCode::Up));
    app.event(key(KeyCode::Up));
    assert_eq!(at(&app), Some(Item::Turn("t1".into())));

    // The cursor marks its line, reversed, in the transcript zone.
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer[(4, 1)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert!(
        !buffer[(4, 0)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
}

#[test]
fn enter_and_right_open_the_item_and_left_closes_it_or_goes_to_its_turn() {
    let mut app = app_with(vec![running(
        "t1",
        "One",
        vec![call("c1", "t1", "Bash", "completed")],
    )]);
    app.event(key(KeyCode::Tab));

    app.event(key(KeyCode::Enter));
    assert!(app.open.contains(&Item::Turn("t1".into())));
    app.event(key(KeyCode::Down));
    app.event(key(KeyCode::Right));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen.iter().any(|row| row == "      reading the detail…"),
        "an open call reads its detail: {screen:#?}"
    );
    // A host that reads no detail leaves the call with what its step says.
    app.detail_read("c1".into(), Err("no detail here".into()));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen.iter().any(|row| row == "      Bash · Bash subject"),
        "an open step shows what it says whole under its row: {screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      no detail here"),
        "{screen:#?}"
    );

    app.event(key(KeyCode::Left));
    assert!(!app.open.contains(&Item::Step {
        turn: "t1".into(),
        step: "c1".into()
    }));
    app.event(key(KeyCode::Left));
    assert_eq!(app.cursor, Some(Item::Turn("t1".into())));
    app.event(key(KeyCode::Left));
    assert!(!app.open.contains(&Item::Turn("t1".into())));
}

#[test]
fn home_and_end_go_to_the_first_and_the_newest_turn() {
    let mut app = app_with(vec![
        done("t1", "One", "a"),
        done("t2", "Two", "b"),
        done("t3", "Three", "c"),
    ]);
    app.event(key(KeyCode::Tab));

    app.event(key(KeyCode::Home));
    assert_eq!(app.cursor, Some(Item::Turn("t1".into())));
    app.event(key(KeyCode::End));
    assert_eq!(app.cursor, Some(Item::Turn("t3".into())));
}

#[tokio::test]
async fn y_copies_the_item_at_the_cursor_to_the_clipboard() {
    let mut app = App::new("s1");
    let mut stub = Stub::default();

    run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![done("t1", "Run the tests", "All pass.")])],
        vec![key(KeyCode::Tab), key(KeyCode::Char('y'))],
    )
    .await;

    let copies = stub.copies.lock().unwrap();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].contains("USER  Run the tests"), "{copies:?}");
    assert!(copies[0].contains("AGENT  All pass."), "{copies:?}");
}

#[tokio::test]
async fn escape_cancels_the_running_turn() {
    let mut app = App::new("s1");
    let mut stub = Stub::default();

    run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![running("t1", "Work", vec![])])],
        vec![key(KeyCode::Esc)],
    )
    .await;

    assert_eq!(*stub.cancels.lock().unwrap(), 1);
}

/// Twenty turns: more than a screen of 24 rows holds.
fn many() -> Vec<Value> {
    (1..=20)
        .map(|n| {
            done(
                &format!("t{n:02}"),
                &format!("prompt {n}"),
                &format!("answer {n}"),
            )
        })
        .collect()
}

#[test]
fn page_up_and_page_down_scroll_the_transcript_and_the_end_follows() {
    let mut app = app_with(many());
    let end = draw(&app, WIDTH, HEIGHT);
    assert!(end.iter().any(|row| row == "❯ prompt 20"), "{end:#?}");

    app.event(key(KeyCode::PageUp));
    let up = draw(&app, WIDTH, HEIGHT);
    assert!(!up.iter().any(|row| row == "❯ prompt 20"), "{up:#?}");
    assert!(up.iter().any(|row| row == "❯ prompt 15"), "{up:#?}");

    app.event(key(KeyCode::PageDown));
    app.event(key(KeyCode::PageDown));
    assert_eq!(draw(&app, WIDTH, HEIGHT), end);
    app.frame(Frame::Update(update(
        2,
        json!([{"op": "turn_upsert", "turn": done("t21", "prompt 21", "answer 21")}]),
    )));
    let followed = draw(&app, WIDTH, HEIGHT);
    assert!(
        followed.iter().any(|row| row == "  answer 21"),
        "{followed:#?}"
    );
}

#[test]
fn the_wheel_scrolls_in_both_zones_and_a_click_opens_or_closes_an_item() {
    let mut app = app_with(many());
    let end = draw(&app, WIDTH, HEIGHT);
    for zone in [Zone::Input, Zone::Transcript] {
        app.zone = zone;
        app.event(wheel(MouseEventKind::ScrollUp));
        let up = draw(&app, WIDTH, HEIGHT);
        // The transcript zone: the screen less the box and the bottom row.
        let zone_rows = usize::from(HEIGHT) - 4;
        assert_eq!(
            up[3..zone_rows],
            end[..zone_rows - 3],
            "three lines up in {zone:?}"
        );
        app.event(wheel(MouseEventKind::ScrollDown));
        assert_eq!(
            draw(&app, WIDTH, HEIGHT)[..zone_rows],
            end[..zone_rows],
            "back at the end in {zone:?}"
        );
    }

    let row = end.iter().position(|row| row == "❯ prompt 20").unwrap() as u16;
    app.zone = Zone::Input;
    app.event(click(row));
    assert!(app.open.contains(&Item::Turn("t20".into())));
    assert_eq!(app.cursor, Some(Item::Turn("t20".into())));
    app.event(click(row));
    assert!(!app.open.contains(&Item::Turn("t20".into())));
}

fn permission() -> Value {
    step(
        "permission",
        "p1",
        "t1",
        json!({"tool": {"title": "Bash", "kind": "execute",
                                                    "subject": "cargo test"}}),
    )
}

#[tokio::test]
async fn a_pending_permission_shows_above_the_input_and_enter_sends_the_picked_option() {
    let mut app = App::new("s1");
    let mut stub = Stub::default();
    let asked = turn(
        "t1",
        "running",
        json!({"attention": [permission()], "steps": [permission()]}),
    );

    let screen = run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![asked])],
        vec![key(KeyCode::Down), key(KeyCode::Enter)],
    )
    .await;

    assert_eq!(*stub.sent.lock().unwrap(), ["no"]);
    let rule = screen
        .iter()
        .position(|row| row.starts_with("─ permission ─"))
        .unwrap();
    assert_eq!(
        screen[rule + 1..rule + 5],
        [
            "❯ Allow Bash?",
            "  Bash · cargo test",
            "    1. Allow",
            "  ❯ 2. Reject"
        ]
    );
    let input = screen
        .iter()
        .rposition(|row| row.starts_with("❯ Tell the agent"))
        .unwrap();
    assert!(rule < input, "{screen:#?}");
}

#[tokio::test]
async fn the_selected_permission_option_stays_visible_after_resize_and_answers() {
    let mut permission = permission();
    permission["options"] = json!([
        {"id": "yes", "name": "Allow"},
        {"id": "no", "name": "Reject"},
        {"id": "always", "name": "Always allow"},
        {"id": "never", "name": "Always reject"}
    ]);
    let snapshot = snap(
        1,
        vec![turn("t1", "running", json!({"attention": [permission]}))],
    );
    let mut app = App::new("s1");
    app.frame(snapshot.clone());
    assert_eq!(count(&draw(&app, WIDTH, HEIGHT), "❯ 1. Allow"), 1);

    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 12)).unwrap();
    let source = stream::iter([Ok(snapshot)]).chain(stream::pending());
    let keys = stream::iter([
        Ok(TermEvent::Resize(WIDTH, 12)),
        Ok(key(KeyCode::Enter)),
        Ok(ctrl('c')),
        Ok(ctrl('c')),
    ]);
    let mut stub = Stub::default();
    drive(&mut terminal, source, &mut stub, Box::pin(keys), &mut app)
        .await
        .unwrap();
    let small = rows(terminal.backend().buffer());
    assert_eq!(count(&small, "❯ 1. Allow"), 1, "{small:#?}");
    assert_eq!(*stub.sent.lock().unwrap(), ["yes"]);
}

fn form() -> QuestionDto {
    serde_json::from_value(json!({
        "id": "q1", "session_id": "s1", "message": "Codex needs your input.",
        "created_at": "2026-11-27T10:00:00Z",
        "schema": {"type": "object", "required": ["base"], "properties": {
            "base": {"type": "string", "title": "Which base branch?", "oneOf": [
                {"const": "main", "title": "Main"},
                {"const": "next", "title": "Next"}
            ]}
        }}
    }))
    .unwrap()
}

fn asking() -> Frame {
    let question = step(
        "question",
        "q1",
        "t1",
        json!({"question": "Codex needs your input."}),
    );
    snap(
        1,
        vec![turn(
            "t1",
            "running",
            json!({"attention": [question.clone()], "steps": [question]}),
        )],
    )
}

#[test]
fn question_reads_retry_after_failure_and_reconnect() {
    let mut app = App::new("s1");
    app.frame(asking());
    assert!(app.wants_questions());
    for failures in 1..=3 {
        app.question_failures = failures;
        app.asked_questions.clear();
        assert_eq!(app.wants_questions(), failures < 3);
    }
    app.frame(Frame::Dropped);
    app.frame(asking());
    assert!(app.wants_questions());
    app.questions_read(vec![form()]);
    assert!(!app.wants_questions());
}

#[tokio::test]
async fn an_open_question_shows_its_form_above_the_input_and_the_keys_answer_it() {
    let mut app = App::new("s1");
    let mut stub = Stub {
        questions: vec![form()],
        ..Stub::default()
    };

    let screen = run(
        &mut app,
        &mut stub,
        vec![asking()],
        vec![key(KeyCode::Down)],
    )
    .await;
    let header = screen
        .iter()
        .position(|row| row.contains("Which base branch?"))
        .unwrap();
    assert_eq!(
        screen[header..header + 4],
        [
            "❯ Which base branch?",
            "Codex needs your input.",
            "    1. Main",
            "  ❯ 2. Next"
        ]
    );

    run(
        &mut app,
        &mut stub,
        vec![asking()],
        vec![key(KeyCode::Char('2'))],
    )
    .await;
    let answers = stub.answers.lock().unwrap();
    assert_eq!(answers[0].0, "q1");
    assert_eq!(answers[0].1.action, QuestionAction::Accept);
    assert_eq!(
        answers[0].1.content.as_ref().unwrap()["base"],
        json!("next")
    );
}

#[tokio::test]
async fn the_active_question_and_choice_stay_visible_after_resize_and_answer() {
    let mut app = App::new("s1");
    let mut question = form();
    question.schema["properties"]["base"]["oneOf"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"const": "third", "title": "Third"}),
            json!({"const": "fourth", "title": "Fourth"}),
        ]);
    let mut stub = Stub {
        questions: vec![question],
        ..Stub::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 12)).unwrap();
    let source = stream::iter([Ok(asking())]).chain(stream::pending());
    let keys = stream::iter([
        Ok(TermEvent::Resize(WIDTH, 12)),
        Ok(key(KeyCode::Enter)),
        Ok(ctrl('c')),
        Ok(ctrl('c')),
    ]);
    drive(&mut terminal, source, &mut stub, Box::pin(keys), &mut app)
        .await
        .unwrap();

    let screen = rows(terminal.backend().buffer());
    assert_eq!(count(&screen, "Which base branch?"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "❯ 1. Main"), 1, "{screen:#?}");
    let answers = stub.answers.lock().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(
        answers[0].1.content.as_ref().unwrap()["base"],
        json!("main")
    );
}

#[tokio::test]
async fn escape_declines_the_open_question() {
    let mut app = App::new("s1");
    let mut stub = Stub {
        questions: vec![form()],
        ..Stub::default()
    };

    run(&mut app, &mut stub, vec![asking()], vec![key(KeyCode::Esc)]).await;

    let answers = stub.answers.lock().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1.action, QuestionAction::Decline);
}

/// The three field shapes each adapter sends: a single-select `string` with
/// `oneOf`, a multi-select `array` with `items.anyOf`, and a plain `string`.
fn three_fields() -> Value {
    json!({"type": "object", "required": ["base"], "properties": {
        "base": {"type": "string", "title": "Which base branch?",
                 "oneOf": [{"const": "main", "title": "Main"},
                           {"const": "next", "title": "Next — the *release* line"}]},
        "checks": {"type": "array", "title": "Which checks?",
                   "items": {"anyOf": [{"const": "fmt", "title": "Format"},
                                       {"const": "clippy", "title": "Clippy"}]}},
        "note": {"type": "string", "title": "Anything else?"},
    }})
}

/// A console with the open question `q1`, whose form has `schema`.
fn asked(schema: Value) -> App {
    let mut app = App::new("s1");
    app.frame(asking());
    let mut question = form();
    question.schema = schema;
    app.questions_read(vec![question]);
    app
}

fn press(app: &mut App, code: KeyCode) -> Action {
    app.key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// What a typed `text` and the Enter after it do.
fn type_and_enter(app: &mut App, text: &str) -> Action {
    typed(app, text);
    press(app, KeyCode::Enter)
}

/// The content of the answer an action sends, which must be an acceptance.
fn accepted(action: Action) -> serde_json::Map<String, Value> {
    let Action::Answer { answer, .. } = action else {
        panic!("an answer, not {action:?}");
    };
    assert_eq!(answer.action, QuestionAction::Accept);
    answer.content.expect("an acceptance carries its content")
}

#[test]
fn a_required_question_left_unanswered_blocks_the_send_until_it_is_answered() {
    for custom in [false, true] {
        let mut schema = three_fields();
        schema["required"] = json!(["base", "note"]);
        if custom {
            let properties = schema["properties"].as_object_mut().unwrap();
            let previous = std::mem::take(properties);
            for (key, value) in previous {
                properties.insert(key.clone(), value);
                if key == "base" {
                    properties.insert(
                        "base_custom".into(),
                        json!({"type": "string", "title": "Other"}),
                    );
                }
            }
        }
        let mut app = asked(schema);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Right);
        assert_eq!(type_and_enter(&mut app, "done"), Action::None);
        let screen = draw(&app, WIDTH, HEIGHT);
        assert_eq!(
            count(&screen, "Which base branch? needs an answer."),
            1,
            "{screen:#?}"
        );
        assert_eq!(count(&screen, "1 of 3"), 1, "{screen:#?}");

        if custom {
            assert_eq!(type_and_enter(&mut app, "release"), Action::None);
        } else {
            assert_eq!(press(&mut app, KeyCode::Char('2')), Action::None);
        }
        press(&mut app, KeyCode::Right);
        let content = accepted(type_and_enter(&mut app, "done"));
        if custom {
            assert_eq!(content["base_custom"], "release");
            assert!(!content.contains_key("base"));
        } else {
            assert_eq!(content["base"], "next");
        }
        assert_eq!(content["note"], "done");
    }
}

#[test]
fn a_required_multi_select_answered_with_text_alone_sends_no_unchecked_choice() {
    let mut app = asked(
        json!({"type": "object", "required": ["question_0"], "properties": {
            "question_0": {"type": "array", "title": "Checks",
                           "items": {"anyOf": [{"const": "fmt", "title": "Format"}]}},
            "question_0_custom": {"type": "string", "title": "Other"}
        }}),
    );

    let content = accepted(type_and_enter(&mut app, "lint"));

    assert_eq!(Value::Object(content), json!({"question_0_custom": "lint"}));
}

#[test]
fn a_number_field_is_one_step_and_sends_its_value() {
    let mut app = asked(json!({"type": "object", "properties": {
        "count": {"type": "integer", "title": "Count"}}}));

    let content = accepted(type_and_enter(&mut app, "12"));

    assert_eq!(Value::Object(content), json!({"count": 12}));
}

#[test]
fn a_number_picks_a_choice_and_moving_between_questions_keeps_the_answers() {
    let mut app = asked(three_fields());
    assert_eq!(press(&mut app, KeyCode::Char('2')), Action::None);
    press(&mut app, KeyCode::Left);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "1 of 3 ✓"), 1, "{screen:#?}");

    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Right);
    let content = accepted(type_and_enter(&mut app, "done"));

    assert_eq!(content["base"], "next");
}

#[test]
fn a_choice_and_its_other_companion_are_one_step_and_the_typed_answer_stands_for_the_choice() {
    let mut app = asked(json!({"type": "object", "properties": {
        "question_0": {"type": "string", "title": "Base", "description": "Which branch?",
            "oneOf": [{"const": "Main (Recommended)", "title": "Main (Recommended)",
                       "description": "Stable branch"},
                      {"const": "Next", "title": "Next"}]},
        "question_0_custom": {"type": "string", "title": "Other"},
        "question_1": {"type": "array", "title": "Checks", "description": "Which checks?",
            "items": {"anyOf": [{"const": "fmt", "title": "Format"}]}},
        "question_1_custom": {"type": "string", "title": "Other"}
    }}));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "1 of 2"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "recommended"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "(Recommended)"), 0, "{screen:#?}");
    assert_eq!(count(&screen, "Which checks?"), 0, "{screen:#?}");

    assert_eq!(type_and_enter(&mut app, "release"), Action::None);
    press(&mut app, KeyCode::Char(' '));
    let content = accepted(type_and_enter(&mut app, "lint"));

    assert_eq!(
        Value::Object(content),
        json!({"question_0_custom": "release", "question_1": ["fmt"], "question_1_custom": "lint"})
    );
}

#[test]
fn the_keys_answer_each_field_and_the_last_enter_sends_the_content() {
    let mut app = asked(three_fields());
    assert_eq!(press(&mut app, KeyCode::Down), Action::None);
    assert_eq!(press(&mut app, KeyCode::Enter), Action::None);
    assert_eq!(press(&mut app, KeyCode::Char(' ')), Action::None);
    assert_eq!(press(&mut app, KeyCode::Down), Action::None);
    assert_eq!(press(&mut app, KeyCode::Char(' ')), Action::None);
    assert_eq!(press(&mut app, KeyCode::Enter), Action::None);

    let content = accepted(type_and_enter(&mut app, "ship it"));

    assert_eq!(
        Value::Object(content),
        json!({"base": "next", "checks": ["fmt", "clippy"], "note": "ship it"})
    );
}

#[test]
fn enter_accepts_a_form_without_fields_with_empty_content() {
    let mut app = asked(json!({"type": "object", "properties": {}}));

    let content = accepted(press(&mut app, KeyCode::Enter));

    assert!(content.is_empty());
}

#[test]
fn text_typed_on_a_choice_field_is_a_prompt_and_answers_nothing() {
    let mut app = asked(three_fields());

    assert_eq!(
        type_and_enter(&mut app, "hello"),
        Action::Send("hello".into())
    );

    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(
        count(&screen, "Which base branch? · 1 of 3"),
        1,
        "{screen:#?}"
    );
}

#[test]
fn left_and_tab_with_text_in_the_box_do_not_change_the_question() {
    let mut schema = three_fields();
    schema["properties"] = json!({
        "a": {"type": "string", "title": "First"},
        "b": {"type": "string", "title": "Second"},
    });
    let mut app = asked(schema);
    typed(&mut app, "helo");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Tab);
    typed(&mut app, "l");

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(count(&screen, "First · 1 of 2"), 1, "{screen:#?}");
}

#[test]
fn a_required_field_with_no_choice_is_not_taken_empty() {
    let mut schema = three_fields();
    schema["properties"]["base"] = json!({"type": "string", "title": "Branch name"});
    let mut app = asked(schema);

    assert_eq!(press(&mut app, KeyCode::Enter), Action::None);

    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(
        count(&screen, "Branch name needs an answer."),
        1,
        "{screen:#?}"
    );
    assert_eq!(count(&screen, "Branch name · 1 of 3"), 1, "{screen:#?}");
}

#[test]
fn a_slash_opens_the_session_commands_and_tab_completes_the_pick() {
    let mut snapshot = snapshot(1, vec![]);
    snapshot.session.commands = serde_json::from_value(json!([
        {"name": "review", "description": "Review the diff", "hint": null},
        {"name": "plan", "description": "Plan the work", "hint": "what to plan"}
    ]))
    .unwrap();
    let mut app = App::new("s1");
    app.frame(Frame::Snapshot(Box::new(snapshot), None));

    typed(&mut app, "/pl");
    let listed = draw(&app, WIDTH, HEIGHT);
    app.event(key(KeyCode::Tab));

    assert!(
        listed
            .iter()
            .any(|row| row.starts_with("❯ /plan") && row.contains("Plan the work")),
        "{listed:#?}"
    );
    assert!(
        !listed.iter().any(|row| row.contains("/review")),
        "{listed:#?}"
    );
    assert_eq!(app.input.joined(), "/plan ");
    assert_eq!(
        app.zone(),
        Zone::Input,
        "Tab completes while the list is open"
    );
    let hinted = draw(&app, WIDTH, HEIGHT);
    assert!(
        hinted.iter().any(|row| row.contains("/plan what to plan")),
        "{hinted:#?}"
    );
}

#[test]
fn one_ctrl_c_keeps_the_console_and_the_second_leaves_it() {
    let mut app = app_with(vec![]);

    assert_eq!(app.event(ctrl('c')), Action::None);
    assert!(
        draw(&app, WIDTH, HEIGHT)
            .last()
            .unwrap()
            .contains("ctrl-c again to leave")
    );
    typed(&mut app, "x");
    assert_eq!(
        app.event(ctrl('c')),
        Action::None,
        "a key between disarms it"
    );
    assert_eq!(app.event(ctrl('c')), Action::Quit);
}

#[test]
fn a_reconnect_shows_no_item_two_times() {
    let mut app = app_with(vec![done("t1", "One", "first answer")]);
    app.event(ctrl('o'));
    app.steps_read(
        "t1".into(),
        vec![serde_json::from_value(call("c1", "t1", "Bash", "completed")).unwrap()],
    );

    app.frame(Frame::Dropped);
    let down = draw(&app, WIDTH, HEIGHT);
    app.frame(snap(
        7,
        vec![done("t1", "One", "first answer"), streaming_answer()],
    ));
    let back = draw(&app, WIDTH, HEIGHT);

    assert!(down.last().unwrap().contains("reconnecting"), "{down:#?}");
    assert!(!back.last().unwrap().contains("reconnecting"), "{back:#?}");
    assert_eq!(count(&back, "◐ message"), 1, "{back:#?}");
    for text in ["❯ One", "first answer", "Bash", "❯ Two", "second answer"] {
        assert_eq!(count(&back, text), 1, "{text}: {back:#?}");
    }
}

#[test]
fn the_farewell_prints_at_most_twenty_folded_turns_and_a_hint_to_the_log() {
    let mut app = app_with(many());
    app.frame(Frame::Update(update(
        2,
        json!([{"op": "turn_upsert", "turn": done("t21", "prompt 21", "answer 21")}]),
    )));

    let said = app.farewell(&Printer::new(chrono::Utc));

    assert!(
        !said.contains("prompt 1\n"),
        "the oldest turn is left out: {said}"
    );
    assert!(
        said.starts_with("10:00:00  USER  prompt 2\n  completed · 2 steps · 1 tool call\n"),
        "{said}"
    );
    assert_eq!(said.matches("USER").count(), 20);
    assert!(
        said.ends_with(
            "every turn of the session, with its steps: ariadne session logs s1 --steps\n"
        ),
        "{said}"
    );
}

#[tokio::test]
async fn a_session_that_has_ended_leaves_after_its_snapshot() {
    let mut ended = snapshot(1, vec![done("t1", "One", "a")]);
    ended.session.status = ariadne_core::SessionStatus::Exited;
    let mut app = App::new("s1");
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();

    drive(
        &mut terminal,
        stream::iter([Ok(Frame::Snapshot(Box::new(ended), None))]).chain(stream::pending()),
        &mut Stub::default(),
        Box::pin(stream::pending()),
        &mut app,
    )
    .await
    .unwrap();

    assert!(app.has_ended());
    assert!(
        app.farewell(&Printer::new(chrono::Utc))
            .contains("USER  One")
    );
}

/// The console drawn on the ANSI backend, read back by an emulator, against
/// the same state drawn fresh on the test backend.
fn same_screen(tap: &Tap, emulator: &mut vt100::Parser, app: &App, width: u16, height: u16) {
    emulator.process(&std::mem::take(&mut *tap.0.lock().unwrap()));
    let emulated: Vec<String> = emulator
        .screen()
        .rows(0, width)
        .map(|row| row.trim_end().to_string())
        .collect();
    assert_eq!(emulated, draw(app, width, height));
}

#[tokio::test]
async fn the_screen_on_an_ansi_terminal_reads_back_as_the_test_backend_draws_it() {
    let tap = Tap::default();
    let window = Window::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(AnsiBackend::new(tap.clone(), window.clone())).unwrap();
    let mut emulator = emulator(HEIGHT, WIDTH, 0);
    let mut app = App::new("s1");
    let frames = vec![snap(
        1,
        vec![
            done(
                "t1",
                "Run the tests — **all** of them",
                "All `tests` pass.\n\n- one\n- two",
            ),
            running(
                "t2",
                "Now the 中文 docs",
                vec![call("c1", "t2", "Bash", "running")],
            ),
        ],
    )];
    let source = stream::iter(frames.into_iter().map(Ok)).chain(stream::pending());
    let events = stream::iter([key(KeyCode::Tab), ctrl('c'), ctrl('c')].map(Ok));

    drive(
        &mut terminal,
        source,
        &mut Stub::default(),
        events.chain(stream::pending()),
        &mut app,
    )
    .await
    .unwrap();

    // The running turn's spinner moves with the clock: stop it where the
    // last draw left it.
    same_screen(&tap, &mut emulator, &app, WIDTH, HEIGHT);
}

#[tokio::test]
async fn a_resize_draws_the_whole_screen_again_and_loses_no_line() {
    let tap = Tap::default();
    let window = Window::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(AnsiBackend::new(tap.clone(), window.clone())).unwrap();
    let mut emulator = emulator(HEIGHT, WIDTH, 0);
    let mut app = App::new("s1");
    let long = "word ".repeat(30);
    let frames = vec![snap(
        1,
        vec![
            done("t1", "Short", &long),
            done("t2", "Second prompt", "Second answer."),
        ],
    )];
    let source = stream::iter(frames.into_iter().map(Ok)).chain(stream::pending());
    let (narrow, tall) = (30, 40);
    let resized = window.clone();
    let events =
        stream::iter([TermEvent::Resize(narrow, tall), ctrl('c'), ctrl('c')]).map(move |event| {
            if let TermEvent::Resize(width, height) = event {
                resized.set(width, height);
            }
            Ok(event)
        });

    drive(
        &mut terminal,
        source,
        &mut Stub::default(),
        events.chain(stream::pending()),
        &mut app,
    )
    .await
    .unwrap();

    resize(&mut emulator, tall, narrow);
    same_screen(&tap, &mut emulator, &app, narrow, tall);
    let screen = draw(&app, narrow, tall);
    let words: usize = screen.iter().map(|row| row.matches("word").count()).sum();
    assert_eq!(
        words, 30,
        "every word of the answer is on the screen: {screen:#?}"
    );
    assert_eq!(count(&screen, "Second answer."), 1);
}

/// A closed turn `t1` whose steps are `steps`, open on the screen with the
/// cursor on its first step, driven with `stub` through `events`.
async fn opened(stub: &mut Stub, steps: Vec<Value>, events: Vec<TermEvent>) -> (App, Vec<String>) {
    let steps: Vec<StepDto> = steps
        .into_iter()
        .map(|value| serde_json::from_value(value).unwrap())
        .collect();
    stub.steps.lock().unwrap().push(("t1".into(), steps));
    let mut app = App::new("s1");
    let mut keys = vec![ctrl('o'), key(KeyCode::Tab), key(KeyCode::Down)];
    keys.extend(events);
    let screen = run(
        &mut app,
        stub,
        vec![snap(1, vec![done("t1", "Fix it", "Fixed.")])],
        keys,
    )
    .await;
    (app, screen)
}

fn detail_of(id: &str, kind: &str, detail: Value) -> (String, ToolCallDetailDto) {
    let value = json!({
        "id": id, "tool_call_id": id, "title": "Run", "kind": kind, "status": "completed",
        "detail_rev": 0, "detail": detail,
        "raw": {"input": {"command": "cargo test"}, "output": {"exit": 0}}
    });
    (id.to_string(), serde_json::from_value(value).unwrap())
}

fn buffer(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    terminal.backend().buffer().clone()
}

/// The style of the first cell of `text` on the screen.
fn style_of(buffer: &Buffer, text: &str) -> ratatui::style::Style {
    let screen = rows(buffer);
    let (y, row) = screen
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains(text))
        .unwrap_or_else(|| panic!("{text} is not on the screen: {screen:#?}"));
    let x = unicode_width::UnicodeWidthStr::width(&row[..row.find(text).unwrap()]);
    buffer[(x as u16, y as u16)].style()
}

#[tokio::test]
async fn an_open_command_shows_the_command_and_its_output_folded_then_whole() {
    let output: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let mut stub = Stub {
        details: vec![detail_of(
            "c1",
            "execute",
            json!({"type": "command", "command": "cargo test", "cwd": "/repo",
                   "output": output, "exit_code": 1, "truncated": false}),
        )],
        ..Stub::default()
    };
    let calls = vec![call("c1", "t1", "Bash", "completed")];

    let (_, screen) = opened(&mut stub, calls.clone(), vec![key(KeyCode::Enter)]).await;
    assert!(
        screen.iter().any(|row| row == "      $ cargo test"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      in /repo"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      … 18 more lines"),
        "{screen:#?}"
    );
    assert_eq!(count(&screen, "line 19"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "line 18"), 0, "{screen:#?}");

    // The second Enter draws the output whole.
    let mut stub = Stub {
        details: stub.details.clone(),
        ..Stub::default()
    };
    let (app, _) = opened(
        &mut stub,
        calls,
        vec![key(KeyCode::Enter), key(KeyCode::Enter)],
    )
    .await;
    let screen = draw(&app, WIDTH, 60);
    assert_eq!(count(&screen, "more lines"), 0, "{screen:#?}");
    assert_eq!(count(&screen, "line 1"), 11, "{screen:#?}");
    assert!(
        screen.iter().any(|row| row == "      exit 1"),
        "{screen:#?}"
    );
}

#[tokio::test]
async fn an_open_edit_shows_its_change_as_a_coloured_diff() {
    let unified = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n";
    let mut stub = Stub {
        details: vec![detail_of(
            "c1",
            "edit",
            json!({"type": "diff", "files": [{"path": "src/main.rs", "old_text": null,
                   "new_text": null, "unified": unified, "language": "rust"}]}),
        )],
        ..Stub::default()
    };
    let (app, screen) = opened(
        &mut stub,
        vec![call("c1", "t1", "Edit", "completed")],
        vec![key(KeyCode::Enter)],
    )
    .await;

    assert!(
        screen.iter().any(|row| row == "      src/main.rs"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      @@ -1,3 +1,3 @@"),
        "{screen:#?}"
    );
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert_eq!(
        style_of(&drawn, "-    old();").fg,
        Some(ratatui::style::Color::Red)
    );
    assert_eq!(
        style_of(&drawn, "+    new();").fg,
        Some(ratatui::style::Color::Green)
    );
}

#[tokio::test]
async fn an_open_read_shows_the_file_in_its_language() {
    let mut stub = Stub {
        details: vec![detail_of(
            "c1",
            "read",
            json!({"type": "file", "path": "src/lib.rs", "line": 3, "language": "rust",
                   "text": "fn main() {}\n"}),
        )],
        ..Stub::default()
    };
    let (app, screen) = opened(
        &mut stub,
        vec![call("c1", "t1", "Read", "completed")],
        vec![key(KeyCode::Enter)],
    )
    .await;

    assert!(
        screen.iter().any(|row| row == "      src/lib.rs:3"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      fn main() {}"),
        "{screen:#?}"
    );
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert_eq!(
        style_of(&drawn, "fn main").fg,
        Some(ratatui::style::Color::Magenta),
        "a keyword of the language is coloured"
    );
}

#[tokio::test]
async fn r_shows_the_raw_input_and_output_of_the_call_at_the_cursor() {
    let mut stub = Stub {
        details: vec![detail_of(
            "c1",
            "execute",
            json!({"type": "command", "command": "cargo test", "cwd": null,
                   "output": "ok", "exit_code": 0, "truncated": false}),
        )],
        ..Stub::default()
    };
    let (_, screen) = opened(
        &mut stub,
        vec![call("c1", "t1", "Bash", "completed")],
        vec![key(KeyCode::Char('r'))],
    )
    .await;

    assert!(
        screen.iter().any(|row| row == "      raw input"),
        "{screen:#?}"
    );
    assert!(
        screen
            .iter()
            .any(|row| row == "        \"command\": \"cargo test\""),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      raw output"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "        \"exit\": 0"),
        "{screen:#?}"
    );
    assert_eq!(
        *stub.detail_reads.lock().unwrap(),
        vec![("c1".to_string(), true)]
    );
}

/// A subagent `id` of the turn `turn`, as the trajectory sends it: it took
/// one minute and four seconds and three steps, unless it still runs.
fn subagent(id: &str, turn: &str, title: &str, state: &str, started: &str) -> Value {
    let ended = match state {
        "running" => Value::Null,
        _ => json!("2026-11-27T10:01:04Z"),
    };
    step(
        "subagent",
        &format!("a-{id}"),
        turn,
        json!({"subagent_id": id, "title": title, "task": format!("{title} task"),
               "state": state, "started_at": started, "ended_at": ended,
               "activity": {"steps": 3, "tool_calls": 3, "files_changed": 0, "failed": 0,
                            "cancelled": 0, "thoughts": 0, "subagents": 0}}),
    )
}

/// A snapshot of `turns` whose session has `subagents`, in that order.
fn with_subagents(rev: u64, turns: Vec<Value>, subagents: &[Value]) -> Frame {
    let mut snapshot = snapshot(rev, turns);
    snapshot.subagents = subagents
        .iter()
        .map(|value| serde_json::from_value(value.clone()).unwrap())
        .collect();
    Frame::Snapshot(Box::new(snapshot), None)
}

/// `Explore`, which completed, and `Review`, which started five seconds
/// later and failed: the session lists `Review` first.
fn explore_and_review() -> [Value; 2] {
    [
        subagent("sub2", "t1", "Review", "failed", "2026-11-27T10:00:05Z"),
        subagent("sub1", "t1", "Explore", "completed", "2026-11-27T10:00:00Z"),
    ]
}

/// A step of the lane `lane`.
fn in_lane(mut step: Value, lane: &str) -> StepDto {
    step["lane"] = json!(lane);
    serde_json::from_value(step).unwrap()
}

/// A host whose turn `t1` holds the two subagents and one call of its own,
/// and whose `Explore` ran one `Grep`.
fn tabbed_stub() -> Stub {
    let [review, explore] = explore_and_review();
    let steps = [explore, review, call("c1", "t1", "Bash", "completed")]
        .into_iter()
        .map(|value| serde_json::from_value(value).unwrap())
        .collect();
    Stub {
        steps: Arc::new(Mutex::new(vec![("t1".into(), steps)])),
        lanes: vec![
            (
                ("t1".into(), "sub1".into()),
                vec![in_lane(call("c9", "t1", "Grep", "completed"), "sub1")],
            ),
            (("t1".into(), "sub2".into()), vec![]),
        ],
        ..Stub::default()
    }
}

/// Drive a console of the session with both subagents, its turn open and
/// the cursor on the first step, through `events`.
async fn tabbed(stub: &mut Stub, events: Vec<TermEvent>) -> (App, Vec<String>) {
    let mut app = App::new("s1");
    let mut keys = vec![ctrl('o'), key(KeyCode::Tab), key(KeyCode::Down)];
    keys.extend(events);
    let frames = vec![with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    )];
    let screen = run(&mut app, stub, frames, keys).await;
    (app, screen)
}

#[test]
fn the_top_row_is_a_tab_bar_of_main_and_each_subagent_in_start_order() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    ));

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[0], "[main] ✓ Explore  ✗ Review", "{screen:#?}");
    assert_eq!(screen[1], "❯ Fix it", "the transcript starts under the bar");
    let buffer = buffer(&app, WIDTH, HEIGHT);
    assert!(
        buffer[(1, 0)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "the active tab is reversed as well as bracketed"
    );
    assert!(
        !buffer[(9, 0)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
}

#[test]
fn a_session_with_no_subagent_shows_no_tab_bar_and_the_transcript_uses_the_row() {
    let app = app_with(vec![done("t1", "Fix it", "Fixed.")]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[0], "❯ Fix it", "{screen:#?}");
    assert_eq!(count(&screen, "main"), 0, "{screen:#?}");
}

#[test]
fn a_narrow_bar_cuts_the_names_keeps_the_marks_and_marks_tabs_off_the_screen() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    ));
    assert_eq!(draw(&app, 22, HEIGHT)[0], "[main] ✓ Exp…  ✗ Rev…");

    let four: Vec<Value> = ["Alpha", "Beta", "Gamma", "Delta"]
        .iter()
        .enumerate()
        .map(|(at, title)| {
            subagent(
                &format!("s{at}"),
                "t1",
                title,
                "completed",
                &format!("2026-11-27T10:00:0{at}Z"),
            )
        })
        .collect();
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &four,
    ));
    assert_eq!(
        draw(&app, 20, HEIGHT)[0],
        " [main] ✓ Alp… ›",
        "the tabs past the right edge are marked"
    );
    for _ in 0..3 {
        app.event(ctrl('n'));
    }
    assert_eq!(
        draw(&app, 20, HEIGHT)[0],
        "‹[✓ Gam…] ✓ Del…",
        "the tabs past the left edge are marked"
    );
}

#[test]
fn from_five_columns_the_active_tab_its_mark_and_the_edge_marks_fit_whole() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    ));
    app.event(ctrl('n'));
    assert_eq!(draw(&app, 8, HEIGHT)[0], "‹[✓ E…]›");
    assert_eq!(draw(&app, 5, HEIGHT)[0], "‹[✓]›");
    app.event(ctrl('p'));
    assert_eq!(draw(&app, 5, HEIGHT)[0], " […]›");

    for (active, mark) in [
        ("main", None),
        ("Explore", Some("✓")),
        ("Review", Some("✗")),
    ] {
        for width in 5..=30u16 {
            let bar = draw(&app, width, HEIGHT)[0].clone();
            let (Some(open), Some(close)) = (bar.find('['), bar.find(']')) else {
                panic!("{active} at {width}: no whole active tab in {bar:?}");
            };
            if let Some(mark) = mark {
                assert!(
                    bar[open..close].contains(mark),
                    "{active} at {width}: {bar:?}"
                );
            }
            if width < 22 {
                assert!(
                    bar.contains('‹') || bar.contains('›'),
                    "{active} at {width}: no mark of the tabs off the screen in {bar:?}"
                );
            }
        }
        app.event(ctrl('n'));
    }
}

#[tokio::test]
async fn a_click_on_a_summary_row_opens_the_tab_of_its_subagent() {
    let mut stub = tabbed_stub();
    let (mut app, _) = tabbed(&mut stub, vec![]).await;
    let screen = draw(&app, WIDTH, HEIGHT);
    let row = screen
        .iter()
        .position(|row| row.contains("↳ Review"))
        .unwrap_or_else(|| panic!("{screen:#?}"));

    app.event(click(u16::try_from(row).unwrap()));

    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(screen[0], " main  ✓ Explore [✗ Review]", "{screen:#?}");
    assert_eq!(count(&screen, "Review task"), 1, "{screen:#?}");
}

#[tokio::test]
async fn a_subagent_in_main_is_one_summary_row_that_does_not_open_in_place() {
    let mut stub = tabbed_stub();
    let (app, _) = tabbed(&mut stub, vec![]).await;

    let screen = draw(&app, WIDTH, HEIGHT);

    assert!(
        screen
            .iter()
            .any(|row| row == "    ✓ ↳ Explore  3 steps · 1m 04s"),
        "{screen:#?}"
    );
    assert!(
        screen
            .iter()
            .any(|row| row == "    ✗ ↳ Review  3 steps · 59s"),
        "{screen:#?}"
    );
    let mut stub = tabbed_stub();
    let (_, screen) = tabbed(&mut stub, vec![key(KeyCode::Enter), key(KeyCode::Esc)]).await;
    assert_eq!(screen[0], "[main] ✓ Explore  ✗ Review", "{screen:#?}");
    assert_eq!(count(&screen, "Grep"), 0, "{screen:#?}");
    assert_eq!(count(&screen, "Explore task"), 0, "{screen:#?}");
}

#[tokio::test]
async fn enter_on_a_summary_row_opens_the_tab_of_its_subagent_with_the_rows_of_main() {
    let mut stub = tabbed_stub();
    let (_, screen) = tabbed(&mut stub, vec![key(KeyCode::Enter)]).await;

    assert_eq!(screen[0], " main [✓ Explore] ✗ Review", "{screen:#?}");
    assert!(
        screen
            .iter()
            .any(|row| row == "    ✓ $ Grep · Grep subject  2s"),
        "the step row of main, with no name of its lane: {screen:#?}"
    );
    assert_eq!(count(&screen, "Fix it"), 0, "{screen:#?}");
    assert_eq!(count(&screen, "Bash"), 0, "{screen:#?}");
}

#[test]
fn ctrl_n_and_ctrl_p_move_between_tabs_and_escape_returns_to_main() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    ));
    let bar = |app: &App| draw(app, WIDTH, HEIGHT)[0].clone();

    app.event(ctrl('n'));
    assert_eq!(bar(&app), " main [✓ Explore] ✗ Review");
    app.event(ctrl('n'));
    assert_eq!(bar(&app), " main  ✓ Explore [✗ Review]");
    app.event(ctrl('n'));
    assert_eq!(
        bar(&app),
        "[main] ✓ Explore  ✗ Review",
        "the last tab wraps"
    );
    app.event(ctrl('p'));
    assert_eq!(
        bar(&app),
        " main  ✓ Explore [✗ Review]",
        "the first wraps back"
    );

    app.event(key(KeyCode::Tab));
    assert_eq!(app.zone(), Zone::Transcript, "Tab still moves the zone");
    app.event(key(KeyCode::Tab));
    assert_eq!(app.zone(), Zone::Input);
    assert_eq!(bar(&app), " main  ✓ Explore [✗ Review]");

    app.event(key(KeyCode::Esc));
    assert_eq!(bar(&app), "[main] ✓ Explore  ✗ Review");
}

#[test]
fn each_tab_keeps_its_own_scroll_position_and_cursor_row() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        many(),
        &[subagent(
            "sub1",
            "t20",
            "Explore",
            "completed",
            "2026-11-27T10:00:00Z",
        )],
    ));
    for event in [key(KeyCode::Tab), key(KeyCode::Up), key(KeyCode::Up)] {
        app.event(event);
    }
    draw(&app, WIDTH, HEIGHT);
    app.event(key(KeyCode::PageUp));
    let main = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&main, "prompt 20"), 0, "{main:#?}");

    app.event(ctrl('n'));
    app.lane_read(
        ("t20".into(), "sub1".into()),
        vec![
            in_lane(call("c8", "t20", "Glob", "completed"), "sub1"),
            in_lane(call("c9", "t20", "Edit", "completed"), "sub1"),
        ],
    );
    app.event(key(KeyCode::Up));
    app.event(key(KeyCode::Up));
    let tab = draw(&app, WIDTH, HEIGHT);
    let cursor = app.cursor.clone();
    assert!(
        matches!(&cursor, Some(Item::Step { step, .. }) if step == "c8"),
        "{cursor:?}"
    );

    app.event(ctrl('p'));
    assert_eq!(draw(&app, WIDTH, HEIGHT), main, "main keeps its scroll");
    assert_eq!(app.cursor, Some(Item::Turn("t18".into())), "and its cursor");

    app.event(ctrl('n'));
    assert_eq!(draw(&app, WIDTH, HEIGHT), tab);
    assert_eq!(app.cursor, cursor, "the tab keeps its cursor");
}

#[test]
fn the_input_sends_to_the_session_from_a_subagent_tab() {
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![done("t1", "Fix it", "Fixed.")],
        &explore_and_review(),
    ));
    app.event(ctrl('n'));
    typed(&mut app, "go on");

    assert_eq!(app.event(key(KeyCode::Enter)), Action::Send("go on".into()));
}

#[test]
fn a_shown_subagent_tab_is_read_again_when_its_turn_completes() {
    let explore = subagent("sub1", "t1", "Explore", "running", "2026-11-27T10:00:00Z");
    let mut app = App::new("s1");
    app.frame(with_subagents(
        1,
        vec![running("t1", "Fix it", vec![explore.clone()])],
        std::slice::from_ref(&explore),
    ));
    assert_eq!(app.next_lane(), None, "main reads no lane");
    app.event(ctrl('n'));
    let lane = app.next_lane().expect("the shown tab is read");
    assert_eq!(app.next_lane(), None);
    app.lane_read(lane.clone(), vec![]);

    let mut ended = explore.clone();
    ended["state"] = json!("completed");
    app.frame(with_subagents(
        2,
        vec![done("t1", "Fix it", "Fixed.")],
        &[ended],
    ));
    assert_eq!(
        app.next_lane(),
        Some(lane),
        "a turn that ends makes the console read the lane again"
    );
}

fn planned(entries: Value) -> Frame {
    let mut snapshot = snapshot(1, vec![done("t1", "Plan it", "Planned.")]);
    snapshot.plans = vec![
        serde_json::from_value(json!({
            "plan_id": "p1", "variant": "entries", "entries": entries, "markdown": null,
            "path": null, "turn_id": "t1", "removed": false,
            "updated_at": "2026-11-27T10:00:00Z"
        }))
        .unwrap(),
    ];
    Frame::Snapshot(Box::new(snapshot), None)
}

#[test]
fn a_plan_with_an_open_entry_shows_one_line_above_the_input_that_opens_to_the_checklist() {
    let mut app = App::new("s1");
    app.frame(planned(json!([
        {"content": "Read the code", "status": "completed", "priority": null},
        {"content": "Write the tests", "status": "in_progress", "priority": null},
        {"content": "Ship it", "status": "pending", "priority": null}
    ])));
    let screen = draw(&app, WIDTH, HEIGHT);
    let rule = screen
        .iter()
        .position(|row| row.starts_with("───"))
        .unwrap();
    assert_eq!(
        screen[rule - 1],
        "▸ plan 1/3 · Write the tests",
        "{screen:#?}"
    );
    assert_eq!(count(&screen, "plan 1/3"), 1);

    app.event(ctrl('t'));
    let screen = draw(&app, WIDTH, HEIGHT);
    let rule = screen
        .iter()
        .position(|row| row.starts_with("───"))
        .unwrap();
    assert_eq!(
        screen[rule - 4..rule],
        [
            "▾ plan 1/3 · Write the tests",
            "  ☑ Read the code",
            "  ◐ Write the tests",
            "  ☐ Ship it",
        ],
        "{screen:#?}"
    );

    let mut done_plan = App::new("s1");
    done_plan.frame(planned(json!([
        {"content": "Read the code", "status": "completed", "priority": null}
    ])));
    assert_eq!(count(&draw(&done_plan, WIDTH, HEIGHT), "plan"), 0);
}

#[tokio::test]
async fn warnings_and_compactions_show_in_their_turn_from_its_notices_without_a_step_read() {
    let mut closed = done("t1", "Work", "Done.");
    closed["notices"] = json!([
        step("compaction", "k1", "t1", json!({})),
        step(
            "note",
            "n1",
            "t1",
            json!({"severity": "warning", "text": "disk almost full"})
        ),
    ]);
    closed["notices_more"] = json!(2);
    let mut app = App::new("s1");
    let mut stub = Stub::default();
    let screen = run(&mut app, &mut stub, vec![snap(1, vec![closed])], vec![]).await;

    assert_eq!(
        screen[2], "    ! note       disk almost full",
        "{screen:#?}"
    );
    assert_eq!(
        screen[3], "    ✓ compaction context compacted",
        "{screen:#?}"
    );
    assert_eq!(
        screen[4], "    2 more notices in the steps of the turn",
        "{screen:#?}"
    );
    assert!(stub.step_reads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_info_note_shows_in_the_open_turn_and_not_in_the_folded_one() {
    let info = step(
        "note",
        "n2",
        "t1",
        json!({"severity": "info", "text": "mode changed"}),
    );
    let mut stub = Stub::default();
    let mut app = App::new("s1");
    let folded = run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![done("t1", "Work", "Done.")])],
        vec![],
    )
    .await;
    assert_eq!(count(&folded, "mode changed"), 0, "{folded:#?}");

    let (_, screen) = opened(&mut Stub::default(), vec![info], vec![]).await;
    assert!(
        screen
            .iter()
            .any(|row| row == "    · note       mode changed"),
        "{screen:#?}"
    );
}

#[tokio::test]
async fn an_answered_permission_is_a_mark_on_the_row_of_its_call() {
    let mut bash = call("c1", "t1", "Bash", "completed");
    bash["permission"] = json!("p1");
    let answered = step(
        "permission",
        "p1",
        "t1",
        json!({"tool_call_id": "c1", "state": "answered",
               "answer": {"option_id": "yes", "label": "Allow", "decided_by": "ai"}}),
    );
    let (app, screen) = opened(&mut Stub::default(), vec![bash, answered], vec![]).await;
    assert_eq!(count(&screen, "permission"), 0, "{screen:#?}");
    let screen = draw(&app, 80, HEIGHT);
    assert!(
        screen
            .iter()
            .any(|row| row == "    ✓ $ Bash · Bash subject  ✓ Allow · by ai  2s"),
        "{screen:#?}"
    );
}

fn with_state() -> App {
    let mut snapshot = snapshot(1, vec![done("t1", "Work", "Done.")]);
    snapshot.session = serde_json::from_value(json!({
        "status": "running", "attention_reason": null, "running_turn_id": null,
        "title": null,
        "mode": {"current_id": "code", "available": [
            {"id": "code", "name": "Code", "description": null}]},
        "config_options": [{"id": "model", "name": "Model", "description": null,
            "category": "model", "current_value": "opus",
            "values": [{"value": "opus", "name": "Opus 5.5", "description": null}]}],
        "context": {"used": 90000, "size": 200000},
        "usage": {"input_tokens": 12400, "cached_input_tokens": 0, "output_tokens": 3100},
        "cost": {"amount": 0.42, "currency": "USD"}, "commands": []
    }))
    .unwrap();
    let mut app = App::new("s1");
    app.frame(Frame::Snapshot(Box::new(snapshot), None));
    app
}

/// The bottom row: the row under the input box's lower rule.
fn bottom_row(screen: &[String]) -> String {
    let rule = screen
        .iter()
        .rposition(|row| row.starts_with("───"))
        .unwrap();
    screen[rule + 1].clone()
}

/// A row of `width` columns: `left` at the start and `? keys` at the end.
fn keyed(left: &str, width: usize) -> String {
    let pad = width - left.chars().count() - "? keys".len() - 1;
    format!("{left}{}? keys", " ".repeat(pad))
}

#[test]
fn the_bottom_row_shows_the_state_mark_model_mode_context_and_cost_and_the_keys_hint() {
    let app = with_state();
    let screen = draw(&app, 80, HEIGHT);
    assert_eq!(
        bottom_row(&screen),
        keyed(" ◐ · Opus 5.5 · Code · context 45% · $0.42", 80)
    );
    // The one row replaces the status row and the footer.
    assert_eq!(screen.last().unwrap(), &bottom_row(&screen), "{screen:#?}");
    let row = bottom_row(&screen);
    for word in ["running", "↑", "tab", "enter", "pgup"] {
        assert!(!row.contains(word), "{word}: {row}");
    }
}

#[test]
fn a_narrow_bottom_row_drops_whole_items_in_rank_order_and_the_keys_hint_last() {
    let app = with_state();
    let at = |width: usize| bottom_row(&draw(&app, u16::try_from(width).unwrap(), HEIGHT));
    assert_eq!(
        at(51),
        keyed(" ◐ · Opus 5.5 · Code · context 45% · $0.42", 51)
    );
    assert_eq!(at(50), keyed(" ◐ · Opus 5.5 · Code · context 45%", 50));
    assert_eq!(at(42), keyed(" ◐ · Opus 5.5 · Code", 42));
    assert_eq!(at(28), keyed(" ◐ · Opus 5.5", 28));
    assert_eq!(at(21), keyed(" ◐", 21));
    assert_eq!(at(10), keyed("", 10));
    assert_eq!(at(9), "");
}

#[test]
fn the_bottom_row_names_the_session_after_the_state_mark() {
    let mut app = with_state();
    app.named("Add the frobnicator · agent · develop");
    assert_eq!(
        bottom_row(&draw(&app, 100, HEIGHT)),
        keyed(
            " ◐ · Add the frobnicator · agent · develop · Opus 5.5 · Code · context 45% · $0.42",
            100
        )
    );
}

#[test]
fn a_narrow_bottom_row_drops_the_name_before_any_other_item() {
    let mut app = with_state();
    app.named("Work");
    let at = |width: usize| bottom_row(&draw(&app, u16::try_from(width).unwrap(), HEIGHT));
    let rest = " ◐ · Opus 5.5 · Code · context 45% · $0.42";
    // The name needs its own room and a separator.
    assert_eq!(
        at(58),
        keyed(" ◐ · Work · Opus 5.5 · Code · context 45% · $0.42", 58)
    );
    // Short of that the name is cut, and short of an ellipsis and a letter
    // it is gone, while every other item is whole.
    assert_eq!(
        at(57),
        keyed(" ◐ · Wo… · Opus 5.5 · Code · context 45% · $0.42", 57)
    );
    assert_eq!(
        at(56),
        keyed(" ◐ · W… · Opus 5.5 · Code · context 45% · $0.42", 56)
    );
    assert_eq!(at(55), keyed(rest, 55));
    assert_eq!(at(51), keyed(rest, 51));
    assert_eq!(at(50), keyed(" ◐ · Opus 5.5 · Code · context 45%", 50));
}

#[test]
fn a_name_longer_than_its_room_is_cut_with_an_ellipsis_and_the_other_items_stay_whole() {
    let mut app = with_state();
    app.named("Add the frobnicator · agent · develop");
    let row = bottom_row(&draw(&app, 70, HEIGHT));
    assert_eq!(
        row,
        keyed(
            " ◐ · Add the frobnic… · Opus 5.5 · Code · context 45% · $0.42",
            70
        )
    );
}

#[test]
fn a_session_is_named_by_its_title_and_its_seat_with_the_step() {
    use ariadne_core::Seat;
    let named = |title, seat, step| session_name(title, seat, step);
    assert_eq!(
        named(Some("Add it"), Some(Seat::Agent), Some("develop")),
        "Add it · agent · develop"
    );
    assert_eq!(
        named(Some("Ship it"), Some(Seat::Orchestrator), Some("develop")),
        "Ship it · orchestrator"
    );
    assert_eq!(named(None, Some(Seat::Reviewer), None), "reviewer");
    assert_eq!(named(None, None, None), "");
}

#[test]
fn reconnecting_and_the_leave_hint_replace_the_left_side_of_the_row_while_they_apply() {
    let mut app = with_state();
    app.frame(Frame::Dropped);
    assert_eq!(
        bottom_row(&draw(&app, 60, HEIGHT)),
        keyed(" reconnecting", 60)
    );
    app.frame(snap(2, vec![done("t1", "Work", "Done.")]));
    assert!(bottom_row(&draw(&app, 60, HEIGHT)).starts_with(" ◐"));
    app.event(ctrl('c'));
    assert_eq!(
        bottom_row(&draw(&app, 60, HEIGHT)),
        keyed(" ctrl-c again to leave", 60)
    );
}

#[test]
fn a_question_mark_opens_an_overlay_of_every_key_in_groups_by_zone() {
    let mut app = app_with(many());
    typed(&mut app, "?");
    let screen = draw(&app, 80, 80);
    let at = |text: &str| {
        screen
            .iter()
            .position(|row| row.contains(text))
            .unwrap_or_else(|| panic!("{text}: {screen:#?}"))
    };
    // A group for each zone, in order, each with its own keys.
    let (input, transcript, tabs, everywhere) =
        (at("Input"), at("Transcript"), at(" Tabs"), at("Everywhere"));
    assert!(
        input < transcript && transcript < tabs && tabs < everywhere,
        "{screen:#?}"
    );
    for (key, group) in [
        ("enter", input),
        ("shift-enter", input),
        ("esc", input),
        ("↑ ↓ j k", transcript),
        ("y", transcript),
        ("home end", transcript),
        ("ctrl-n", tabs),
        ("ctrl-p", tabs),
        ("pgup pgdn", everywhere),
        ("ctrl-o", everywhere),
        ("ctrl-t", everywhere),
        ("ctrl-c ctrl-c", everywhere),
    ] {
        let row = screen
            .iter()
            .position(|row| row.trim_start().starts_with(key))
            .unwrap_or_else(|| panic!("{key}: {screen:#?}"));
        assert!(row > group, "{key} is under its group: {screen:#?}");
    }
    assert!(
        screen.iter().all(|row| !row.contains("prompt 20")),
        "{screen:#?}"
    );
    // The typed question mark did not reach the box.
    assert!(app.input.is_empty());
}

#[test]
fn every_key_of_the_overlay_is_reached_by_scrolling_in_a_60_by_24_terminal() {
    let mut app = app_with(many());
    app.event(key(KeyCode::Char('?')));
    let mut seen = String::new();
    let mut first = None;
    for _ in 0..40 {
        let screen = draw(&app, WIDTH, HEIGHT);
        assert!(screen[0].contains("? or esc closes"), "{screen:#?}");
        assert!(screen.iter().all(|row| row.chars().count() <= 60));
        first.get_or_insert_with(|| screen.clone());
        seen.push_str(&screen.join("\n"));
        app.event(key(KeyCode::Down));
    }
    let first = first.unwrap();
    assert!(first[1].contains("Input"), "{first:#?}");
    // An inventory read from the key handlers, not from the overlay: the
    // chords of the input box (`input.rs`), the form (`form.rs`), the
    // suggestions (`commands.rs`), the transcript and the console (`mod.rs`).
    for chord in [
        "shift-enter",
        "alt-enter",
        "ctrl-j",
        "ctrl-a",
        "ctrl-e",
        "ctrl-u",
        "ctrl-k",
        "ctrl-w",
        "alt-b",
        "alt-f",
        "alt-←",
        "alt-→",
        "backspace",
        "delete",
        "home",
        "end",
        "space",
        "pgup",
        "pgdn",
        "ctrl-o",
        "ctrl-t",
        "ctrl-c",
        "ctrl-d",
        "tab",
        "esc",
        "enter",
        "1-9",
        "wheel",
        "click",
        "j k",
    ] {
        assert!(seen.contains(chord), "{chord} is not in the overlay");
    }
    for zone in [
        "Input",
        "Input box",
        "Permission",
        "Question",
        "Transcript",
        "Everywhere",
    ] {
        assert!(seen.contains(zone), "{zone}");
    }
    app.event(key(KeyCode::Esc));
    assert!(
        draw(&app, WIDTH, HEIGHT)
            .iter()
            .all(|row| !row.contains("? or esc closes"))
    );
}

/// The first column at which `text` starts on `row`, in characters.
fn column_of(row: &str, text: &str) -> Option<usize> {
    row.find(text).map(|at| row[..at].chars().count())
}

#[test]
fn a_wide_overlay_shows_its_groups_whole_in_columns_with_no_scroll() {
    let mut app = app_with(many());
    app.event(key(KeyCode::Char('?')));
    let screen = draw(&app, 160, 50);
    // A heading stands alone in its cell: two spaces or the row's end follow.
    let head = |zone: &str| {
        let text = format!(" {zone}");
        screen
            .iter()
            .enumerate()
            .find_map(|(y, row)| {
                let at = [
                    row.find(&format!("{text}  ")),
                    row.ends_with(&text).then(|| row.len() - text.len()),
                ]
                .into_iter()
                .flatten()
                .next()?;
                Some((row[..at].chars().count(), y))
            })
            .unwrap_or_else(|| panic!("{zone}: {screen:#?}"))
    };
    let heads = KEYS.iter().map(|(zone, _)| head(zone)).collect::<Vec<_>>();
    // Two or more columns, and the groups in order down each, then across.
    assert!(heads.iter().any(|(x, _)| *x > 0), "{screen:#?}");
    for pair in heads.windows(2) {
        let ((x, y), (next_x, next_y)) = (pair[0], pair[1]);
        assert!(next_x > x || (next_x == x && next_y > y), "{heads:?}");
    }
    // Each group is whole in its column: every key of it stands under its
    // heading, at the column of the heading, before the next heading.
    for (at, (_, keys)) in KEYS.iter().enumerate() {
        let (x, y) = heads[at];
        for (key, _) in *keys {
            assert!(
                screen[y + 1..].iter().any(|row| row
                    .chars()
                    .skip(x)
                    .collect::<String>()
                    .trim_start()
                    .starts_with(key)),
                "{key}: {screen:#?}"
            );
        }
    }
    // No scroll reaches anything the first screen does not show.
    app.event(key(KeyCode::Down));
    assert_eq!(draw(&app, 160, 50), screen);
    app.event(key(KeyCode::End));
    assert_eq!(draw(&app, 160, 50), screen);
}

#[test]
fn an_overlay_fills_every_column_the_width_holds() {
    let mut app = app_with(many());
    app.event(key(KeyCode::Char('?')));
    // 348 columns hold six columns of 58.
    let screen = draw(&app, 348, 50);
    let heads = KEYS
        .iter()
        .filter(|(zone, _)| screen[1].contains(&format!(" {zone}")))
        .count();
    assert_eq!(heads, 6, "{screen:#?}");
}

#[test]
fn a_narrow_overlay_shows_one_column() {
    let mut app = app_with(many());
    app.event(key(KeyCode::Char('?')));
    let screen = draw(&app, 100, 80);
    for (zone, _) in KEYS {
        let row = screen
            .iter()
            .find(|row| row.starts_with(&format!(" {zone}")))
            .unwrap_or_else(|| panic!("{zone}: {screen:#?}"));
        assert_eq!(column_of(row, zone), Some(1), "{screen:#?}");
    }
    // One heading to a row: no row holds two.
    assert!(
        screen
            .iter()
            .all(|row| !row.contains("Transcript") || !row.contains("Tabs"))
    );
}

#[test]
fn the_overlay_hides_the_text_cursor_and_closing_it_brings_the_cursor_back() {
    let tap = Tap::default();
    let window = Window::new(WIDTH, HEIGHT);
    let mut terminal = Terminal::new(AnsiBackend::new(tap.clone(), window)).unwrap();
    let mut emulator = emulator(HEIGHT, WIDTH, 0);
    let mut app = app_with(many());
    let mut hidden = |app: &App| {
        terminal.draw(|frame| app.render(frame)).unwrap();
        emulator.process(&std::mem::take(&mut *tap.0.lock().unwrap()));
        emulator.screen().hide_cursor()
    };
    assert!(!hidden(&app));
    app.event(key(KeyCode::Char('?')));
    assert!(hidden(&app));
    app.event(key(KeyCode::Esc));
    assert!(!hidden(&app));
}

#[test]
fn a_question_mark_in_a_non_empty_input_types_the_character() {
    let mut app = app_with(many());
    typed(&mut app, "why?");
    let screen = draw(&app, 80, HEIGHT);
    assert!(screen.iter().any(|row| row == "❯ why?"), "{screen:#?}");
    assert!(screen.iter().all(|row| !row.contains("? or esc closes")));
}

#[test]
fn a_question_mark_or_escape_closes_the_overlay_and_the_screen_stays_as_it_was() {
    for close in [KeyCode::Char('?'), KeyCode::Esc] {
        let mut app = app_with(many());
        app.event(key(KeyCode::PageUp));
        let before = draw(&app, WIDTH, HEIGHT);
        app.event(key(KeyCode::Char('?')));
        // Keys other than the closing ones leave the box and the scroll alone.
        app.event(key(KeyCode::Char('x')));
        app.event(key(KeyCode::PageDown));
        assert!(
            draw(&app, WIDTH, HEIGHT)
                .iter()
                .any(|row| row.contains("? or esc closes"))
        );
        app.event(key(close));
        let after = draw(&app, WIDTH, HEIGHT);
        assert_eq!(after, before, "{close:?}");
        assert!(app.input.is_empty(), "{close:?}");
    }
}

#[test]
fn the_question_mark_opens_the_overlay_from_the_transcript_zone_too() {
    let mut app = app_with(many());
    app.event(key(KeyCode::Tab));
    assert_eq!(app.zone(), Zone::Transcript);
    app.event(key(KeyCode::Char('?')));
    assert!(
        draw(&app, 80, 40)
            .iter()
            .any(|row| row.contains("? or esc closes"))
    );
    app.event(key(KeyCode::Esc));
    assert_eq!(app.zone(), Zone::Transcript);
    assert!(
        draw(&app, 80, 40)
            .iter()
            .all(|row| !row.contains("? or esc closes"))
    );
}

#[test]
fn an_image_audio_resource_and_link_show_as_one_line_with_type_and_name() {
    let mut turn = done("t1", "Show it", "");
    turn["answer"] = json!([
        {"type": "text", "text": "Here."},
        {"type": "image", "mime_type": "image/png", "blob": null, "uri": "file:///tmp/shot.png"},
        {"type": "audio", "mime_type": "audio/wav", "blob": null},
        {"type": "resource", "uri": "file:///src/lib.rs", "mime_type": null, "text": null, "blob": null},
        {"type": "resource_link", "uri": "https://x/spec", "name": "spec", "title": "The spec",
         "mime_type": null, "size": null}
    ]);
    let screen = draw(&app_with(vec![turn]), WIDTH, HEIGHT);
    assert_eq!(
        screen[2..7],
        [
            "  Here.",
            "  [image] shot.png",
            "  [audio] audio/wav",
            "  [resource] lib.rs",
            "  [resource link] The spec",
        ],
        "{screen:#?}"
    );
}

/// A closed turn with a warning, a compaction, and two notices it does not
/// carry.
fn noticed() -> Value {
    let mut closed = done("t1", "Work", "Done.");
    closed["notices"] = json!([
        step("compaction", "k1", "t1", json!({})),
        step(
            "note",
            "n1",
            "t1",
            json!({"severity": "warning", "text": "disk almost full"})
        ),
    ]);
    closed["notices_more"] = json!(2);
    closed
}

#[tokio::test]
async fn an_open_turn_shows_its_notices_and_their_count_once_whether_its_steps_read_or_not() {
    // The host reads no steps of the turn: the notices show all the same.
    let mut app = App::new("s1");
    let mut stub = Stub::default();
    let screen = run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![noticed()])],
        vec![ctrl('o')],
    )
    .await;
    assert_eq!(screen[1], "  ▾ ✓ 2 steps · 1 tool call · 2s");
    assert_eq!(
        screen[2], "    ! note       disk almost full",
        "{screen:#?}"
    );
    assert_eq!(
        screen[3], "    ✓ compaction context compacted",
        "{screen:#?}"
    );
    assert_eq!(
        screen[4], "    2 more notices in the steps of the turn",
        "{screen:#?}"
    );
    assert_eq!(*stub.step_reads.lock().unwrap(), ["t1"]);

    // The steps read: a step that is a notice has no second row.
    let mut stub = Stub::default();
    let steps: Vec<StepDto> = [
        step(
            "note",
            "n1",
            "t1",
            json!({"severity": "warning", "text": "disk almost full"}),
        ),
        call("c1", "t1", "Bash", "completed"),
    ]
    .into_iter()
    .map(|value| serde_json::from_value(value).unwrap())
    .collect();
    stub.steps.lock().unwrap().push(("t1".into(), steps));
    let mut app = App::new("s1");
    let screen = run(
        &mut app,
        &mut stub,
        vec![snap(1, vec![noticed()])],
        vec![ctrl('o')],
    )
    .await;
    assert_eq!(count(&screen, "disk almost full"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "2 more notices"), 1, "{screen:#?}");
    assert_eq!(screen[5], "    ✓ $ Bash · Bash subject  2s", "{screen:#?}");
}

#[test]
fn every_entry_of_a_checklist_taller_than_its_room_is_reached_with_the_wheel() {
    let entries: Vec<Value> = (1..=30)
        .map(|n| json!({"content": format!("Entry {n}"), "status": "pending", "priority": null}))
        .collect();
    let mut app = App::new("s1");
    app.frame(planned(Value::Array(entries)));
    app.event(ctrl('t'));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(screen.iter().any(|row| row == "  ☐ Entry 1"), "{screen:#?}");
    assert_eq!(count(&screen, "Entry 30"), 0, "{screen:#?}");
    let counted = screen
        .iter()
        .find(|row| row.contains("the wheel scrolls the plan"))
        .unwrap_or_else(|| panic!("{screen:#?}"));
    assert!(counted.starts_with("  0 above · "), "{counted}");

    let plan_row = screen
        .iter()
        .position(|row| row.contains("☐ Entry 2"))
        .unwrap() as u16;
    for _ in 0..20 {
        app.event(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: plan_row,
            modifiers: KeyModifiers::NONE,
        }));
        draw(&app, WIDTH, HEIGHT);
    }
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen.iter().any(|row| row == "  ☐ Entry 30"),
        "{screen:#?}"
    );
    assert!(
        screen
            .iter()
            .any(|row| row.ends_with(" above · 0 below · the wheel scrolls the plan")),
        "{screen:#?}"
    );
    // The wheel over the plan leaves the transcript where it was.
    assert_eq!(app.scroll.get(), None);
}

#[test]
fn the_bottom_row_names_the_stored_model_where_the_agent_sends_no_model_option() {
    let mut app = app_with(vec![done("t1", "Work", "Done.")]);
    app.stored_model("claude-acp:opus-5");
    assert_eq!(
        bottom_row(&draw(&app, 80, HEIGHT)),
        keyed(" ◐ · opus-5", 80)
    );

    // The agent's model option wins over the stored model.
    let mut app = with_state();
    app.stored_model("claude-acp:opus-5");
    let row = bottom_row(&draw(&app, 80, HEIGHT));
    assert!(row.starts_with(" ◐ · Opus 5.5 · Code · "), "{row}");
}

#[test]
fn a_changed_image_name_or_link_title_draws_again_while_the_text_stays() {
    let answer = |uri: &str, title: &str| {
        let mut turn = running("t1", "Show it", vec![]);
        turn["answer"] = json!([
            {"type": "text", "text": "Here."},
            {"type": "image", "mime_type": null, "blob": null, "uri": uri},
            {"type": "resource_link", "uri": "https://x/spec", "name": "spec", "title": title,
             "mime_type": null, "size": null}
        ]);
        turn
    };
    let mut app = app_with(vec![answer("file:///old.png", "Old spec")]);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen.iter().any(|row| row == "  [image] old.png"),
        "{screen:#?}"
    );

    app.frame(Frame::Update(update(
        2,
        json!([{"op": "turn_upsert", "turn": answer("file:///new.png", "New spec")}]),
    )));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen.iter().any(|row| row == "  [image] new.png"),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "  [resource link] New spec"),
        "{screen:#?}"
    );
    assert_eq!(count(&screen, "old.png"), 0, "{screen:#?}");
}

#[test]
fn an_update_of_a_call_changes_its_row_in_place() {
    let mut app = app_with(vec![running(
        "t1",
        "Build it",
        vec![call("c1", "t1", "Work", "running")],
    )]);
    app.frame(Frame::Update(update(
        2,
        json!([{"op": "item_upsert", "step": call("c1", "t1", "Work", "completed")}]),
    )));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "Work subject"), 1, "{screen:#?}");
    assert!(
        screen
            .iter()
            .any(|row| row == "    ✓ $ Work · Work subject  2s"),
        "{screen:#?}"
    );
}

#[tokio::test]
async fn an_open_thought_shows_its_whole_markdown_dimmed() {
    let thought = step(
        "thought",
        "h1",
        "t1",
        json!({"content": [{"type": "text", "text": "First **idea**.\n\nSecond idea."}]}),
    );
    let (app, screen) = opened(
        &mut Stub::default(),
        vec![thought],
        vec![key(KeyCode::Enter)],
    )
    .await;
    assert!(
        screen.iter().any(|row| row == "      First idea."),
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      Second idea."),
        "{screen:#?}"
    );
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert!(
        style_of(&drawn, "Second idea.")
            .add_modifier
            .contains(ratatui::style::Modifier::DIM)
    );
}

#[test]
fn a_permission_draws_its_call_undimmed_and_names_the_subagent_that_asks() {
    let mut asking = permission();
    asking["subagent"] = json!({"id": "sub1", "title": "explorer"});
    let app = app_with(vec![turn(
        "t1",
        "running",
        json!({"attention": [asking.clone()], "steps": [asking]}),
    )]);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(
        screen
            .iter()
            .any(|row| row == "❯ Allow Bash? · asked by subagent explorer"),
        "{screen:#?}"
    );
    let drawn = buffer(&app, WIDTH, HEIGHT);
    let style = style_of(&drawn, "Bash · cargo test");
    assert!(
        !style.add_modifier.contains(ratatui::style::Modifier::DIM),
        "{style:?}"
    );
    assert_eq!(style.fg, Some(ratatui::style::Color::Yellow));
}

#[test]
fn a_notice_is_an_item_the_cursor_reaches_and_it_opens_in_place_to_its_whole_text() {
    let mut closed = done("t1", "Work", "Done.");
    closed["notices"] = json!([step(
        "note",
        "n1",
        "t1",
        json!({"severity": "warning", "text": "disk almost full\nfree 2 GB now"})
    )]);
    let mut app = app_with(vec![closed]);
    app.event(key(KeyCode::Tab));
    app.event(key(KeyCode::Down));
    let notice = Item::Step {
        turn: "t1".into(),
        step: "n1".into(),
    };
    assert_eq!(app.cursor, Some(notice.clone()));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "free 2 GB now"), 0, "{screen:#?}");

    app.event(key(KeyCode::Enter));
    assert!(app.open.contains(&notice));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(
        screen[2], "    ! note       disk almost full",
        "{screen:#?}"
    );
    assert!(
        screen.iter().any(|row| row == "      free 2 GB now"),
        "{screen:#?}"
    );

    // The notice stays one row when its turn opens too.
    app.event(ctrl('o'));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(count(&screen, "! note"), 1, "{screen:#?}");
    assert_eq!(count(&screen, "free 2 GB now"), 1, "{screen:#?}");
}

#[test]
fn the_wheel_reaches_every_entry_of_a_plan_on_a_short_terminal() {
    const SHORT: u16 = 13;
    let entries: Vec<Value> = (1..=30)
        .map(|n| json!({"content": format!("Entry {n}"), "status": "pending", "priority": null}))
        .collect();
    let mut app = App::new("s1");
    app.frame(planned(Value::Array(entries)));
    app.event(ctrl('t'));
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..40 {
        let screen = draw(&app, WIDTH, SHORT);
        for row in &screen {
            if let Some(n) = row.strip_prefix("  ☐ Entry ") {
                seen.insert(n.parse::<usize>().unwrap());
            }
        }
        let row = screen
            .iter()
            .position(|row| row.starts_with("  ☐ Entry "))
            .unwrap() as u16;
        app.event(TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
    assert_eq!(seen, (1..=30).collect(), "every entry shows at some notch");
}

/// A host with only the methods every host must have, as the daemon's
/// terminal host is: it reads no detail and no subagent steps.
struct Bare(Vec<StepDto>);

impl Host for Bare {
    async fn send(&mut self, _: String) -> Result<(), String> {
        Ok(())
    }

    async fn answer(&mut self, _: &str, _: AnswerQuestionRequest) -> Result<(), String> {
        Ok(())
    }

    async fn cancel(&mut self) {}

    async fn steps(&mut self, _: &str) -> Result<Vec<StepDto>, String> {
        Ok(self.0.clone())
    }

    async fn questions(&mut self) -> Result<Vec<QuestionDto>, String> {
        Ok(Vec::new())
    }

    fn copy(&mut self, _: String) {}
}

#[tokio::test]
async fn a_host_that_reads_no_detail_shows_the_step_text_and_the_reason() {
    let steps = vec![serde_json::from_value(call("c1", "t1", "Bash", "completed")).unwrap()];
    let mut app = App::new("s1");
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    let frames = vec![snap(1, vec![done("t1", "Fix it", "Fixed.")])];
    let source = stream::iter(frames.into_iter().map(Ok)).chain(stream::pending());
    let keys = [
        ctrl('o'),
        key(KeyCode::Tab),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        ctrl('c'),
        ctrl('c'),
    ];
    let events = stream::iter(keys.into_iter().map(Ok)).chain(stream::pending());
    drive(
        &mut terminal,
        source,
        &mut Bare(steps),
        Box::pin(events),
        &mut app,
    )
    .await
    .unwrap();
    let screen = rows(terminal.backend().buffer());

    assert!(
        screen.iter().any(|row| row == "      Bash · Bash subject"),
        "{screen:#?}"
    );
    assert!(
        screen
            .iter()
            .any(|row| row == "      this console reads no step detail"),
        "{screen:#?}"
    );
    // No stored model and no model option: the bottom row has none.
    assert!(!bottom_row(&screen).contains(" · "), "{screen:#?}");
}

/// A tool call of `kind`, with an optional `diff_stat`.
fn kinded(id: &str, kind: &str, status: &str, extra: Value) -> StepDto {
    let mut value = call(id, "t1", &format!("{kind} {id}"), status);
    value["kind"] = json!(kind);
    value["subject"] = json!(format!("{id}.rs"));
    value["title"] = json!(format!("{kind} {id}"));
    if let Value::Object(extra) = extra {
        value.as_object_mut().unwrap().extend(extra);
    }
    serde_json::from_value(value).unwrap()
}

fn open_turn(steps: Vec<StepDto>) -> App {
    let mut app = app_with(vec![done("t1", "Go", "Ok.")]);
    app.steps_read("t1".into(), steps);
    app.event(ctrl('o'));
    app
}

#[test]
fn a_tool_row_starts_with_its_state_mark_and_the_icon_of_its_kind_and_prints_no_kind_word() {
    let app = open_turn(vec![
        kinded("a", "execute", "completed", json!({})),
        kinded("b", "think", "completed", json!({})),
    ]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[2], "    ✓ $ execute a · a.rs  2s");
    assert_eq!(screen[3], "    ✓ ∴ think b · b.rs  2s");
    assert_eq!(count(&screen, "execute    "), 0, "{screen:#?}");
}

#[test]
fn a_kind_with_no_icon_shows_the_fallback_icon() {
    let app = open_turn(vec![kinded("a", "teleport", "completed", json!({}))]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[2], "    ✓ ◆ teleport a · a.rs  2s");
}

#[test]
fn consecutive_read_and_search_steps_show_as_one_group_row_of_their_subjects() {
    let app = open_turn(vec![
        kinded("a", "read", "completed", json!({})),
        kinded("b", "search", "completed", json!({})),
        kinded("c", "read", "completed", json!({})),
        kinded("d", "execute", "completed", json!({})),
        kinded("e", "read", "completed", json!({})),
    ]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[2], "    ✓ ☰ a.rs, b.rs, c.rs");
    // A step of another kind ends the group, and a group of one is a row.
    assert_eq!(screen[3], "    ✓ $ execute d · d.rs  2s");
    assert_eq!(screen[4], "    ✓ ≡ read e · e.rs  2s");
}

#[test]
fn a_group_row_cuts_its_subjects_to_the_width() {
    let app = open_turn(vec![
        kinded("a", "read", "completed", json!({"subject": "a".repeat(40)})),
        kinded("b", "read", "completed", json!({"subject": "b".repeat(40)})),
    ]);

    let screen = draw(&app, 30, HEIGHT);

    assert_eq!(screen[2].chars().count(), 30, "{screen:#?}");
    assert!(screen[2].ends_with('…'), "{screen:#?}");
}

#[test]
fn a_group_is_running_while_a_step_runs_failed_if_one_failed_and_completed_otherwise() {
    let mark = |states: [&str; 3]| {
        let steps = states
            .iter()
            .enumerate()
            .map(|(at, state)| kinded(&format!("s{at}"), "read", state, json!({})))
            .collect();
        draw(&open_turn(steps), WIDTH, HEIGHT)[2].clone()
    };

    assert!(mark(["completed", "running", "failed"]).starts_with("    ◐ ≡ "));
    assert!(mark(["completed", "failed", "completed"]).starts_with("    ✗ ≡ "));
    assert!(mark(["completed", "cancelled", "completed"]).starts_with("    ✓ ≡ "));
}

#[test]
fn enter_on_a_group_row_opens_its_steps_as_rows_that_open_their_detail() {
    let mut app = open_turn(vec![
        kinded("a", "read", "completed", json!({})),
        kinded("b", "read", "completed", json!({})),
    ]);
    app.event(key(KeyCode::Tab));
    app.event(key(KeyCode::Down));
    assert_eq!(
        app.cursor,
        Some(Item::Group {
            turn: "t1".into(),
            first: "a".into()
        })
    );

    app.event(key(KeyCode::Enter));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(screen[2], "    ✓ ≡ a.rs, b.rs");
    assert_eq!(screen[3], "      ✓ ≡ read a · a.rs  2s");
    assert_eq!(screen[4], "      ✓ ≡ read b · b.rs  2s");

    app.event(key(KeyCode::Down));
    app.event(key(KeyCode::Enter));
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(screen[4], "        reading the detail…", "{screen:#?}");
}

#[test]
fn an_edit_row_shows_its_subject_and_the_lines_it_added_and_removed() {
    let stat = json!({"diff_stat": {"files": 1, "added": 12, "removed": 3}});
    let app = open_turn(vec![
        kinded("a", "edit", "completed", stat.clone()),
        kinded("b", "execute", "completed", stat),
        kinded("c", "edit", "completed", json!({})),
    ]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[2], "    ✓ ✎ edit a · a.rs  +12 -3  2s");
    assert_eq!(screen[3], "    ✓ $ execute b · b.rs  2s");
    assert_eq!(screen[4], "    ✓ ✎ edit c · c.rs  2s");
}

#[test]
fn a_group_of_one_kind_has_the_icon_of_that_kind() {
    let app = open_turn(vec![
        kinded("a", "search", "completed", json!({})),
        kinded("b", "search", "completed", json!({})),
    ]);

    assert_eq!(draw(&app, WIDTH, HEIGHT)[2], "    ✓ ⌕ a.rs, b.rs");
}

#[test]
fn a_folded_running_turn_shows_its_last_three_steps_even_when_they_group() {
    let steps: Vec<Value> = (1..=4)
        .map(|at| {
            let mut read = call(&format!("c{at}"), "t1", "Read", "completed");
            read["kind"] = json!("read");
            read["subject"] = json!(format!("f{at}.rs"));
            read
        })
        .collect();
    let app = app_with(vec![running("t1", "Go", steps)]);

    let screen = draw(&app, WIDTH, HEIGHT);

    assert_eq!(screen[2], "    ✓ ≡ f2.rs, f3.rs, f4.rs", "{screen:#?}");
}

#[test]
fn untrusted_prompts_and_answers_show_highlights_and_a_help_legend() {
    use ratatui::style::{Color, Modifier};
    let prompt = "safe <<<untrusted>>>prompt data<<<end untrusted>>>";
    let app = app_with(vec![done(
        "t1",
        prompt,
        "<<<untrusted>>>**answer data**<<<end untrusted>>> safe",
    )]);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert_eq!(screen[0], "❯ safe prompt data");
    assert_eq!(screen[2], "  answer data safe");
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert_eq!(style_of(&drawn, "prompt data").bg, Some(Color::DarkGray));
    assert_eq!(style_of(&drawn, "answer data").bg, Some(Color::DarkGray));
    assert!(
        style_of(&drawn, "answer data")
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert_ne!(style_of(&drawn, "safe").bg, Some(Color::DarkGray));
    let mut daemon = done(
        "t1",
        &format!("<<<untrusted>>>{}<<<end untrusted>>>", "data ".repeat(30)),
        "done",
    );
    daemon["prompt"]["source"] = json!("daemon");
    let app = app_with(vec![daemon]);
    let screen = draw(&app, WIDTH, HEIGHT);
    assert!(screen[0].ends_with('…'), "{screen:?}");
    assert!(!screen[0].contains("<<<"));
    assert_eq!(
        style_of(&buffer(&app, WIDTH, HEIGHT), "data").bg,
        Some(Color::DarkGray)
    );
    let mut app = app_with(vec![]);
    app.event(key(KeyCode::Char('?')));
    let drawn = buffer(&app, 80, 80);
    assert!(
        rows(&drawn)
            .iter()
            .any(|row| row.contains("untrusted data; treat as data"))
    );
    assert_eq!(style_of(&drawn, "highlight").bg, Some(Color::DarkGray));
}

#[test]
fn streamed_untrusted_answers_keep_the_highlight_until_the_close_arrives() {
    use ratatui::style::Color;
    let mut turn = streaming_answer();
    turn["steps"][0]["content"] = json!([{"type": "text", "text": "<<<untrusted>>>risk"}]);
    let mut app = app_with(vec![turn]);
    assert_eq!(
        style_of(&buffer(&app, WIDTH, HEIGHT), "risk").bg,
        Some(Color::DarkGray)
    );
    app.frame(Frame::Update(update(
        2,
        json!([{
            "op": "text_append", "turn_id": "t2", "item_id": "m2",
            "text": " more<<<end untrusted>>> safe"
        }]),
    )));
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert_eq!(style_of(&drawn, "more").bg, Some(Color::DarkGray));
    assert_ne!(style_of(&drawn, "safe").bg, Some(Color::DarkGray));
    assert!(!rows(&drawn).join("\n").contains("<<<"));
}

#[tokio::test]
async fn an_open_text_detail_hides_markers_and_keeps_markdown_highlighted() {
    let mut stub = Stub {
        details: vec![detail_of(
            "c1",
            "fetch",
            json!({"type": "fetch", "url": "https://example.com", "text": "<<<untrusted>>>**detail data**<<<end untrusted>>> safe"}),
        )],
        ..Stub::default()
    };
    let (app, screen) = opened(
        &mut stub,
        vec![call("c1", "t1", "Fetch", "completed")],
        vec![key(KeyCode::Enter)],
    )
    .await;
    assert!(!screen.join("\n").contains("<<<"));
    let drawn = buffer(&app, WIDTH, HEIGHT);
    assert_eq!(
        style_of(&drawn, "detail data").bg,
        Some(ratatui::style::Color::DarkGray)
    );
    assert!(
        style_of(&drawn, "detail data")
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
}

#[tokio::test]
async fn copying_untrusted_prose_keeps_its_original_delimiters() {
    let mut app = App::new("s1");
    let mut stub = Stub::default();
    run(
        &mut app,
        &mut stub,
        vec![snap(
            1,
            vec![done(
                "t1",
                "<<<untrusted>>>prompt<<<end untrusted>>>",
                "<<<untrusted>>>answer<<<end untrusted>>>",
            )],
        )],
        vec![key(KeyCode::Tab), key(KeyCode::Char('y'))],
    )
    .await;
    let copies = stub.copies.lock().unwrap();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].contains("<<<untrusted>>>prompt<<<end untrusted>>>"));
    assert!(copies[0].contains("<<<untrusted>>>answer<<<end untrusted>>>"));
}
