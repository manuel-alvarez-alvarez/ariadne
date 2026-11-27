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
use ariadne_api::trajectory::StepDto;

use super::*;
use crate::ansi::{AnsiBackend, Window};
use crate::trajectory::fixtures::{snapshot, step, turn, update};
use crate::tui::testing::{Tap, emulator, resize};

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

    async fn steps(&mut self, turn_id: &str) -> Result<Vec<StepDto>, String> {
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
    assert_eq!(screen[1], "  ▸ ✓ completed · 2 steps · 1 tool call · 2s");
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
        screen[2].starts_with("    ✓ execute    Second · Second subject  2s"),
        "{screen:#?}"
    );
    assert!(
        screen[4].starts_with("    ◐ execute    Fourth · Fourth subject"),
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

    assert_eq!(screen[1], "  ▾ ✓ completed · 2 steps · 1 tool call · 2s");
    assert_eq!(screen[2], "    ✓ execute    Bash · Bash subject  2s");
    assert_eq!(screen[3], "    ✗ execute    Edit · Edit subject  2s");
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
        assert!(screen.last().unwrap().contains("y copy"), "{screen:#?}");
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
        screen.iter().any(|row| row == "      Bash · Bash subject"),
        "an open step shows what it says whole under its row: {screen:#?}"
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
        // The transcript zone: the screen less the box and the footer.
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
        vec![
            done("t1", "One", "first answer"),
            done("t2", "Two", "second answer"),
        ],
    ));
    let back = draw(&app, WIDTH, HEIGHT);

    assert!(down.last().unwrap().contains("reconnecting"), "{down:#?}");
    assert!(!back.last().unwrap().contains("reconnecting"), "{back:#?}");
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
