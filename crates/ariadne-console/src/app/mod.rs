//! The full-screen console: a session's trajectory (040) drawn as turns on
//! the whole screen, with the input box under it.
//!
//! The screen has two zones. The transcript zone is every turn of the
//! trajectory: its prompt, one activity line and its answer, and the step
//! rows of a turn that is open. The input zone is the box the next prompt is
//! typed into, with a pending permission or an open question above it. One
//! zone at a time has the keys, and Tab moves them (008, rules 39 to 50).
//!
//! The console draws from the client [`View`] of the trajectory and nothing
//! else, so a frame never adds a line twice: a reconnect's snapshot replaces
//! the view, and the screen is drawn again from it. The same is why a resize
//! loses no line. The whole screen is the view drawn at the new size.
//!
//! As in [`crate::tui`], nothing here dials a daemon or owns a process
//! terminal. [`drive`] takes a stream of [`Frame`]s, a [`Host`] for what the
//! console sends and reads, a stream of terminal events and a ratatui
//! terminal on any backend. The host takes the alternate screen and gives it
//! back, and prints [`App::farewell`] after it.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use chrono::TimeZone;
use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures_util::{Stream, StreamExt};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::text::Line;
use tokio::time::interval;

use ariadne_api::sessions::{AnswerQuestionRequest, QuestionDto};
use ariadne_api::trajectory::{
    PermissionState, PermissionStepDto, QuestionState, QuestionStepDto, StepDto,
    TrajectorySnapshotDto, TrajectoryUpdateDto, TurnDto, TurnState,
};

use crate::trajectory::{Printer, Touched, View, step_id, text};
use crate::transcript::{self, FormField};

mod commands;
mod draw;
mod form;
mod input;

#[cfg(test)]
mod tests;

use form::FormState;
use input::Input;

/// How often the spinner of a running turn turns.
const TICK: Duration = Duration::from_millis(120);

/// How many lines one notch of the wheel scrolls.
const WHEEL: usize = 3;

/// How many of its newest steps a folded running turn shows live.
const LIVE_STEPS: usize = 3;

/// The most turns the console prints on the way out.
const FAREWELL_TURNS: usize = 20;

/// One frame of the trajectory stream (040), whichever transport carries it.
#[derive(Debug, Clone)]
pub enum Frame {
    /// The trajectory as it stands: what every stream opens with, and what
    /// a redial brings again.
    Snapshot(Box<TrajectorySnapshotDto>),
    /// One revision of the trajectory.
    Update(TrajectoryUpdateDto),
    /// The stream dropped. The source dials it again, and the next snapshot
    /// is what says it is back.
    Dropped,
}

/// What the console sends to the session and reads from it, over whichever
/// transport the host has.
pub trait Host {
    /// Post a typed prompt, or the id of the option a permission was
    /// answered with. The error is what the console says about a post that
    /// did not arrive, in words a person reads.
    fn send(&mut self, text: String) -> impl Future<Output = Result<(), String>> + Send;

    /// Answer the agent's question `question_id` (021).
    fn answer(
        &mut self,
        question_id: &str,
        answer: AnswerQuestionRequest,
    ) -> impl Future<Output = Result<(), String>> + Send;

    /// Cancel the running turn. A turn that ended between the key and the
    /// post has nothing to cancel, and that is not a failure.
    fn cancel(&mut self) -> impl Future<Output = ()> + Send;

    /// Every step of a closed turn, oldest first: the view holds the steps
    /// of the running turn alone.
    fn steps(&mut self, turn_id: &str)
    -> impl Future<Output = Result<Vec<StepDto>, String>> + Send;

    /// The open questions of the session, with the form each one asks.
    fn questions(&mut self) -> impl Future<Output = Result<Vec<QuestionDto>, String>> + Send;

    /// Put `text` on the clipboard of the terminal.
    fn copy(&mut self, text: String);
}

/// What a key or a click asked the console to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Nothing the host needs to hear about.
    None,
    /// Post this text to the session: a typed prompt, or the id of the
    /// option a permission was answered with.
    Send(String),
    /// Answer the agent's question of this id (021).
    Answer {
        question_id: String,
        answer: AnswerQuestionRequest,
    },
    /// Cancel the running turn.
    Cancel,
    /// Put this text on the clipboard.
    Copy(String),
    /// Leave the console. The session stays alive.
    Quit,
}

/// The zone that has the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Input,
    Transcript,
}

/// One item of the transcript: a turn, or one step of an open turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Item {
    Turn(String),
    Step { turn: String, step: String },
}

impl Item {
    fn turn(&self) -> &str {
        match self {
            Item::Turn(turn) | Item::Step { turn, .. } => turn,
        }
    }
}

/// Where the last draw put the transcript, so that a key or a click reads
/// the screen that the person sees.
#[derive(Debug, Default)]
struct Layout {
    /// The rows of the transcript zone.
    area: Rect,
    /// The first line of the transcript on the screen.
    top: usize,
    /// How many lines the whole transcript takes.
    total: usize,
    /// The item of each line of the whole transcript.
    items: Vec<Option<Item>>,
}

impl Layout {
    /// The first line a screen at the end of the transcript shows.
    fn bottom(&self) -> usize {
        self.total.saturating_sub(usize::from(self.area.height))
    }
}

/// A turn's answer as last drawn: the width and the text it was drawn
/// for, and its lines.
type Drawn = (usize, String, Vec<Line<'static>>);

/// The form of an open question, as the host listed it: the agent's message
/// and the fields of its schema.
type Form = (String, Vec<FormField>);

/// Everything the full-screen console shows, and everything a key changes.
///
/// No terminal in here: [`App::render`] draws it into a ratatui frame, which
/// a `TestBackend` gives as readily as the real one.
pub struct App {
    /// The session, which the hint on the way out names.
    id: String,
    view: Option<View>,
    /// Whether the stream is up. False from a drop to the next snapshot.
    live: bool,
    zone: Zone,
    /// The item at the cursor of the transcript zone.
    cursor: Option<Item>,
    /// The turns and steps that are open.
    open: HashSet<Item>,
    /// The steps of closed turns, as the host read them.
    loaded: HashMap<String, Vec<StepDto>>,
    /// The turns whose steps the host is asked for, in that order.
    loads: VecDeque<String>,
    /// The turns whose steps were asked for once already.
    asked_steps: HashSet<String>,
    /// The first line of the transcript on the screen. `None` follows the
    /// end of the transcript.
    scroll: Cell<Option<usize>>,
    /// The next draw scrolls the item at the cursor into sight.
    reveal: Cell<bool>,
    layout: RefCell<Layout>,
    /// The drawn answers of turns, by turn id: the width and the text they
    /// were drawn for, and their lines.
    answers: RefCell<HashMap<String, Drawn>>,
    input: Input,
    /// The pick of the slash-command suggestion list.
    suggested: usize,
    /// The first match the suggestion list draws.
    scrolled: Cell<usize>,
    /// Escape closed the suggestion list until the box is empty again.
    dismissed: bool,
    /// The option of the pending permission that Enter sends, and the
    /// permission it is for.
    picked: (String, usize),
    /// How far the person is through the open question.
    form: FormState,
    /// The forms of the open questions, by question id.
    forms: HashMap<String, QuestionDto>,
    /// The questions the host was asked about once already.
    asked_questions: HashSet<String>,
    /// The prompts sent that no turn holds yet, oldest first.
    pending: VecDeque<String>,
    /// The turns the console has seen: only a turn new to it takes a pending
    /// prompt away.
    known: HashSet<String>,
    /// What the console says about a post that did not arrive.
    notices: Vec<String>,
    /// Every turn this attach showed, as it last saw it.
    shown: BTreeMap<String, TurnDto>,
    /// One Ctrl-C has been seen: the next one leaves.
    armed: bool,
    tick: usize,
}

impl App {
    /// A console for the session `id`.
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            view: None,
            live: true,
            zone: Zone::Input,
            cursor: None,
            open: HashSet::new(),
            loaded: HashMap::new(),
            loads: VecDeque::new(),
            asked_steps: HashSet::new(),
            scroll: Cell::new(None),
            reveal: Cell::new(false),
            layout: RefCell::new(Layout::default()),
            answers: RefCell::new(HashMap::new()),
            input: Input::default(),
            suggested: 0,
            scrolled: Cell::new(0),
            dismissed: false,
            picked: (String::new(), 0),
            form: FormState::default(),
            forms: HashMap::new(),
            asked_questions: HashSet::new(),
            pending: VecDeque::new(),
            known: HashSet::new(),
            notices: Vec::new(),
            shown: BTreeMap::new(),
            armed: false,
            tick: 0,
        }
    }

    /// The zone that has the keys.
    pub fn zone(&self) -> Zone {
        self.zone
    }

    /// Whether the session has ended as the console last heard of it.
    pub fn has_ended(&self) -> bool {
        self.view.as_ref().is_some_and(View::ended)
    }

    /// Take one frame of the stream. `true` once the session has ended,
    /// which is the loop's cue to leave.
    pub fn frame(&mut self, frame: Frame) -> bool {
        match frame {
            Frame::Snapshot(snapshot) => {
                let view = View::new(*snapshot);
                self.live = true;
                let ids: Vec<String> = view.turns().iter().map(|turn| turn.id.clone()).collect();
                self.input.sync_history(
                    view.turns()
                        .iter()
                        .filter(|turn| from_console(turn))
                        .filter_map(prompt_text)
                        .collect(),
                );
                self.view = Some(view);
                for id in ids {
                    self.saw(&id);
                }
            }
            Frame::Update(update) => {
                let Some(view) = self.view.as_mut() else {
                    return false;
                };
                // The steps of an open turn while it runs: the view drops
                // them when the turn closes, and the turn shows them until
                // the host has read them whole.
                let running: Vec<(String, Vec<StepDto>)> = view
                    .turns()
                    .iter()
                    .filter(|turn| turn.state == TurnState::Running)
                    .filter(|turn| self.open.contains(&Item::Turn(turn.id.clone())))
                    .map(|turn| (turn.id.clone(), turn.steps.clone()))
                    .collect();
                match view.apply(update) {
                    Ok(touched) => {
                        for (id, steps) in running {
                            if view
                                .turn(&id)
                                .is_some_and(|turn| turn.state != TurnState::Running)
                            {
                                self.loaded.insert(id.clone(), steps);
                                self.asked_steps.remove(&id);
                            }
                        }
                        for touched in touched {
                            if let Touched::Turn(id) = touched {
                                self.saw(&id);
                            }
                        }
                    }
                    // A revision went missing: the source dials again, and
                    // its snapshot is what the console shows next.
                    Err(_) => self.live = false,
                }
            }
            Frame::Dropped => self.live = false,
        }
        self.settle();
        self.has_ended()
    }

    /// Note a turn of the view: what this attach showed, and the pending
    /// prompt it takes away when it is new.
    fn saw(&mut self, id: &str) {
        let Some(turn) = self.view.as_ref().and_then(|view| view.turn(id)) else {
            return;
        };
        let turn = turn.clone();
        if self.known.insert(turn.id.clone())
            && from_console(&turn)
            && let Some(text) = prompt_text(&turn)
            && let Some(at) = self.pending.iter().position(|pending| *pending == text)
        {
            self.pending.remove(at);
        }
        self.shown.insert(turn.id.clone(), turn);
    }

    /// Keep the state that hangs off the view in step with it: a cursor on
    /// an item gone goes to its turn, and an open turn with no steps to show
    /// asks the host for them.
    fn settle(&mut self) {
        let items = self.items();
        if let Some(cursor) = &self.cursor
            && !items.contains(cursor)
        {
            let turn = Item::Turn(cursor.turn().to_string());
            self.cursor = items.contains(&turn).then_some(turn);
        }
        let wanted: Vec<String> = self
            .open
            .iter()
            .filter_map(|item| match item {
                Item::Turn(id) => Some(id.clone()),
                Item::Step { .. } => None,
            })
            .filter(|id| self.wants_steps(id))
            .collect();
        for id in wanted {
            if self.asked_steps.insert(id.clone()) {
                self.loads.push_back(id);
            }
        }
        if let Some(permission) = self.permission()
            && permission.id != self.picked.0
        {
            self.picked = (permission.id.clone(), 0);
        }
    }

    /// Whether a turn is closed and the console holds none of its steps.
    fn wants_steps(&self, id: &str) -> bool {
        let Some(turn) = self.view.as_ref().and_then(|view| view.turn(id)) else {
            return false;
        };
        turn.state != TurnState::Running && !self.loaded.contains_key(id)
    }

    /// The next turn whose steps the host should read.
    fn next_load(&mut self) -> Option<String> {
        self.loads.pop_front()
    }

    /// The steps the host read for a closed turn.
    fn steps_read(&mut self, turn_id: String, steps: Vec<StepDto>) {
        self.loaded.insert(turn_id, steps);
        self.settle();
    }

    /// Whether an open question waits whose form the console has not read:
    /// true once for each such question.
    fn wants_questions(&mut self) -> bool {
        let fresh: Vec<String> = self
            .open_questions()
            .into_iter()
            .map(|question| question.request_id.clone())
            .filter(|id| !self.forms.contains_key(id) && !self.asked_questions.contains(id))
            .collect();
        let wants = !fresh.is_empty();
        self.asked_questions.extend(fresh);
        wants
    }

    /// The open questions the host read.
    fn questions_read(&mut self, questions: Vec<QuestionDto>) {
        for question in questions {
            self.forms.insert(question.id.clone(), question);
        }
    }

    /// Something the console sent did not arrive, or something it read did
    /// not come. It is said under the transcript.
    pub fn failed(&mut self, text: &str) {
        self.notices.push(text.to_string());
    }

    /// The post of `sent` was refused: a prompt it was is no longer pending.
    fn refused(&mut self, sent: &str, reason: &str) {
        if let Some(at) = self.pending.iter().rposition(|pending| pending == sent) {
            self.pending.remove(at);
        }
        self.failed(reason);
    }

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// The turns of the view, oldest first.
    fn turns(&self) -> &[TurnDto] {
        self.view.as_ref().map_or(&[], View::turns)
    }

    /// Whether a turn runs.
    fn running(&self) -> bool {
        self.view.as_ref().is_some_and(|view| {
            view.session().running_turn_id.is_some()
                || view
                    .turns()
                    .iter()
                    .any(|turn| turn.state == TurnState::Running)
        })
    }

    /// The steps an open turn shows: the steps the view holds while it
    /// runs, and those the host read once it closed. `None` while they are
    /// read.
    fn steps_of<'a>(&'a self, turn: &'a TurnDto) -> Option<&'a [StepDto]> {
        match turn.state {
            TurnState::Running => Some(&turn.steps),
            _ => self.loaded.get(&turn.id).map(Vec::as_slice),
        }
    }

    /// Every item the cursor can be on, in screen order: each turn, and the
    /// steps of each open turn under it.
    fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for turn in self.turns() {
            let item = Item::Turn(turn.id.clone());
            let open = self.open.contains(&item);
            items.push(item);
            if open {
                for step in self.steps_of(turn).unwrap_or_default() {
                    items.push(Item::Step {
                        turn: turn.id.clone(),
                        step: step_id(step).to_string(),
                    });
                }
            }
        }
        items
    }

    fn step(&self, turn: &str, step: &str) -> Option<&StepDto> {
        let turn = self.view.as_ref()?.turn(turn)?;
        self.steps_of(turn)?
            .iter()
            .find(|held| step_id(held) == step)
    }

    /// The pending permission, while the session can still be answered.
    fn permission(&self) -> Option<&PermissionStepDto> {
        if self.has_ended() {
            return None;
        }
        self.view.as_ref()?.waiting_permission()
    }

    /// The open questions of the session, oldest first.
    fn open_questions(&self) -> Vec<&QuestionStepDto> {
        if self.has_ended() {
            return Vec::new();
        }
        let mut questions: Vec<&QuestionStepDto> = self
            .turns()
            .iter()
            .flat_map(|turn| turn.attention.iter())
            .filter_map(|step| match step {
                StepDto::Question(question) if question.state == QuestionState::Open => {
                    Some(question)
                }
                _ => None,
            })
            .collect();
        questions.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        questions.dedup_by(|a, b| a.id == b.id);
        questions
    }

    /// The question the form answers: the oldest open one, while no
    /// permission waits. With its form, where the host has read it.
    fn question(&self) -> Option<(&QuestionStepDto, Option<Form>)> {
        if self.permission().is_some() {
            return None;
        }
        let question = self.open_questions().into_iter().next()?;
        let form = self.forms.get(&question.request_id).map(|form| {
            (
                form.message.clone(),
                transcript::form_fields(&form.schema, &form.message),
            )
        });
        Some((question, form))
    }

    /// Handle one terminal event other than a resize. The caller does what
    /// the answer asks for.
    pub fn event(&mut self, event: TermEvent) -> Action {
        match event {
            TermEvent::Key(key) => self.key(key),
            TermEvent::Paste(text) => {
                self.armed = false;
                self.zone = Zone::Input;
                self.input.paste(&text);
                self.edited();
                Action::None
            }
            TermEvent::Mouse(mouse) => self.mouse(mouse),
            _ => Action::None,
        }
    }

    /// Handle one key press.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Ctrl-C twice, so that one of them cannot throw away a half-typed
        // prompt; Ctrl-D at once, which is the end of input a shell gives.
        if ctrl && key.code == KeyCode::Char('c') {
            return match std::mem::replace(&mut self.armed, true) {
                true => Action::Quit,
                false => Action::None,
            };
        }
        self.armed = false;
        if ctrl && key.code == KeyCode::Char('d') {
            return Action::Quit;
        }
        if ctrl && key.code == KeyCode::Char('o') {
            if let Some(newest) = self.turns().last() {
                let item = Item::Turn(newest.id.clone());
                self.toggle(item);
            }
            return Action::None;
        }
        match key.code {
            KeyCode::PageUp => {
                self.page(false);
                return Action::None;
            }
            KeyCode::PageDown => {
                self.page(true);
                return Action::None;
            }
            _ => {}
        }
        match self.zone {
            Zone::Input => self.input_key(key),
            Zone::Transcript => self.transcript_key(key),
        }
    }

    fn input_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let modified_enter = key
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT);

        if let Some(permission) = self.permission() {
            let count = permission.options.len();
            let picked = self.picked.1;
            match key.code {
                KeyCode::Up => {
                    self.picked.1 = picked.saturating_sub(1);
                    return Action::None;
                }
                KeyCode::Down => {
                    self.picked.1 = (picked + 1).min(count.saturating_sub(1));
                    return Action::None;
                }
                KeyCode::Char(digit @ '1'..='9') => {
                    let index = digit as usize - '1' as usize;
                    if index < count {
                        self.picked.1 = index;
                    }
                    return Action::None;
                }
                KeyCode::Enter => {
                    return match permission.options.get(picked) {
                        Some(option) => Action::Send(option.id.clone()),
                        None => Action::None,
                    };
                }
                _ => {}
            }
        }

        if let Some(action) = self.form_key(key) {
            self.edited();
            return action;
        }
        if let Some(action) = self.suggest_key(key) {
            return action;
        }
        let action = match key.code {
            KeyCode::Tab => {
                self.focus_transcript();
                Action::None
            }
            KeyCode::Esc if self.running() => Action::Cancel,
            KeyCode::Enter if modified_enter => {
                self.input.newline();
                Action::None
            }
            KeyCode::Char('j' | '\n') if ctrl => {
                self.input.newline();
                Action::None
            }
            KeyCode::Enter if self.input.remove_trailing_backslash() => {
                self.input.newline();
                Action::None
            }
            KeyCode::Enter => match self.input.take() {
                Some(text) => {
                    self.pending.push_back(text.clone());
                    self.scroll.set(None);
                    Action::Send(text)
                }
                None => Action::None,
            },
            _ => {
                self.input.key(key);
                Action::None
            }
        };
        self.edited();
        action
    }

    /// Give the keys to the transcript, its cursor on the newest turn where
    /// it is on no item.
    fn focus_transcript(&mut self) {
        self.zone = Zone::Transcript;
        let items = self.items();
        if self
            .cursor
            .as_ref()
            .is_none_or(|cursor| !items.contains(cursor))
        {
            self.cursor = self.turns().last().map(|turn| Item::Turn(turn.id.clone()));
        }
        self.reveal.set(true);
    }

    fn transcript_key(&mut self, key: KeyEvent) -> Action {
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if plain => self.walk(false),
            KeyCode::Down | KeyCode::Char('j') if plain => self.walk(true),
            KeyCode::Enter | KeyCode::Right => {
                if let Some(cursor) = self.cursor.clone() {
                    self.open_item(cursor);
                }
            }
            KeyCode::Left => match self.cursor.clone() {
                Some(cursor) if self.open.contains(&cursor) => {
                    self.open.remove(&cursor);
                }
                Some(Item::Step { turn, .. }) => {
                    self.cursor = Some(Item::Turn(turn));
                    self.reveal.set(true);
                }
                _ => {}
            },
            KeyCode::Home => {
                self.cursor = self.turns().first().map(|turn| Item::Turn(turn.id.clone()));
                self.reveal.set(true);
            }
            KeyCode::End => {
                self.cursor = self.turns().last().map(|turn| Item::Turn(turn.id.clone()));
                self.reveal.set(true);
            }
            KeyCode::Char('y') if plain => {
                if let Some(text) = self.cursor.as_ref().and_then(|item| self.copied(item)) {
                    return Action::Copy(text);
                }
            }
            KeyCode::Tab | KeyCode::Esc => self.zone = Zone::Input,
            KeyCode::Char(_) if plain => {
                // A typed character is the start of a prompt.
                self.zone = Zone::Input;
                self.input.key(key);
                self.edited();
            }
            _ => {}
        }
        Action::None
    }

    /// Move the cursor to the next item, or the one before it.
    fn walk(&mut self, down: bool) {
        let items = self.items();
        let at = self
            .cursor
            .as_ref()
            .and_then(|cursor| items.iter().position(|item| item == cursor));
        let next = match (at, down) {
            (None, _) => items.len().checked_sub(1),
            (Some(at), true) => Some((at + 1).min(items.len().saturating_sub(1))),
            (Some(at), false) => Some(at.saturating_sub(1)),
        };
        self.cursor = next.and_then(|at| items.get(at).cloned());
        self.reveal.set(true);
    }

    fn open_item(&mut self, item: Item) {
        self.open.insert(item);
        self.settle();
        self.reveal.set(true);
    }

    fn toggle(&mut self, item: Item) {
        if !self.open.remove(&item) {
            self.open_item(item);
        }
    }

    /// The plain text of an item: a turn as `ariadne session logs` prints
    /// it, with the steps it shows, and a step as it prints under its turn.
    fn copied(&self, item: &Item) -> Option<String> {
        let printer = Printer::new(chrono::Local);
        match item {
            Item::Turn(id) => {
                let turn = self.view.as_ref()?.turn(id)?;
                let steps = self
                    .open
                    .contains(item)
                    .then(|| self.steps_of(turn))
                    .flatten();
                Some(printer.turn(turn, steps))
            }
            Item::Step { turn, step } => Some(printer.step(self.step(turn, step)?)),
        }
    }

    /// Scroll a screen up or down.
    fn page(&mut self, down: bool) {
        let page = usize::from(self.layout.borrow().area.height.saturating_sub(1)).max(1);
        self.scroll_by(page, down);
    }

    /// Scroll `lines` up or down. At the end, the transcript follows what
    /// comes.
    fn scroll_by(&mut self, lines: usize, down: bool) {
        let layout = self.layout.borrow();
        let top = self.scroll.get().unwrap_or(layout.bottom());
        let top = match down {
            true => top + lines,
            false => top.saturating_sub(lines),
        };
        self.scroll.set((top < layout.bottom()).then_some(top));
    }

    fn mouse(&mut self, mouse: MouseEvent) -> Action {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_by(WHEEL, false),
            MouseEventKind::ScrollDown => self.scroll_by(WHEEL, true),
            MouseEventKind::Down(MouseButton::Left) => {
                let item = {
                    let layout = self.layout.borrow();
                    let area = layout.area;
                    (mouse.row >= area.y && mouse.row < area.y + area.height).then(|| {
                        let line = layout.top + usize::from(mouse.row - area.y);
                        layout.items.get(line).cloned().flatten()
                    })
                };
                match item {
                    // A click on the transcript opens or closes the item
                    // under it, and puts the cursor there.
                    Some(Some(item)) => {
                        self.zone = Zone::Transcript;
                        self.cursor = Some(item.clone());
                        self.toggle(item);
                    }
                    Some(None) => {}
                    // A click under the transcript is a click on the input.
                    None => self.zone = Zone::Input,
                }
            }
            _ => {}
        }
        Action::None
    }

    /// What the console prints as it closes: the turns this attach showed,
    /// folded, the newest [`FAREWELL_TURNS`] of them, and where the rest is.
    pub fn farewell<Tz>(&self, printer: &Printer<Tz>) -> String
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        let skip = self.shown.len().saturating_sub(FAREWELL_TURNS);
        let mut out: String = self
            .shown
            .values()
            .skip(skip)
            .map(|turn| printer.turn(turn, None))
            .collect();
        out.push_str(&format!(
            "every turn of the session, with its steps: ariadne session logs {} --steps\n",
            self.id
        ));
        out
    }
}

/// Whether a turn's prompt was typed at a console.
fn from_console(turn: &TurnDto) -> bool {
    turn.prompt.as_ref().is_some_and(|prompt| {
        prompt
            .source
            .as_deref()
            .is_none_or(|source| source == "console")
    })
}

/// The text a turn's prompt was submitted with.
fn prompt_text(turn: &TurnDto) -> Option<String> {
    let prompt = turn.prompt.as_ref()?;
    Some(prompt.text.clone().unwrap_or_else(|| text(&prompt.content)))
}

/// Whether a step is still under way.
fn is_live(step: &StepDto) -> bool {
    !crate::trajectory::settled(step)
        || matches!(step, StepDto::Permission(permission) if permission.state == PermissionState::Pending)
}

/// One turn of the loop: whichever of the things the console waits on
/// happened first.
enum Step {
    Frame(Frame),
    Ended,
    Event(TermEvent),
    Closed,
    Tick,
}

/// Run the console until the session ends, the source ends, or the user
/// leaves.
///
/// The source, the host and the event stream are arguments, so a test
/// scripts all three, and the terminal is any ratatui backend: the process
/// terminal, a socket, or a test buffer.
pub async fn drive<B, S, H, E>(
    terminal: &mut Terminal<B>,
    source: S,
    host: &mut H,
    events: E,
    app: &mut App,
) -> Result<()>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    S: Stream<Item = Result<Frame>>,
    H: Host,
    E: Stream<Item = std::io::Result<TermEvent>> + Unpin,
{
    let mut source = std::pin::pin!(source);
    let mut events = events;
    let mut ticker = interval(TICK);

    // The stream opens with its snapshot. Take it before any key, so the
    // first thing drawn is the trajectory as it stands. A session that had
    // already ended is over here: the host prints its turns on the way out.
    let ended = match source.next().await {
        Some(frame) => app.frame(frame?),
        None => false,
    };
    if ended {
        return Ok(());
    }

    loop {
        while let Some(turn) = app.next_load() {
            match host.steps(&turn).await {
                Ok(steps) => app.steps_read(turn, steps),
                Err(reason) => app.failed(&reason),
            }
        }
        if app.wants_questions() {
            match host.questions().await {
                Ok(questions) => app.questions_read(questions),
                Err(reason) => app.failed(&reason),
            }
        }
        terminal.draw(|frame| app.render(frame))?;

        let step = tokio::select! {
            biased;
            frame = source.next() => match frame {
                Some(frame) => Step::Frame(frame?),
                None => Step::Ended,
            },
            event = events.next() => match event {
                Some(Ok(event)) => Step::Event(event),
                Some(Err(_)) | None => Step::Closed,
            },
            _ = ticker.tick() => Step::Tick,
        };

        match step {
            Step::Frame(frame) => {
                if app.frame(frame) {
                    return Ok(());
                }
            }
            // A resize draws the whole screen again, from the view, at the
            // new size.
            Step::Event(TermEvent::Resize(..)) => {
                terminal.autoresize()?;
                terminal.clear()?;
            }
            Step::Event(event) => match app.event(event) {
                Action::None => {}
                Action::Quit => return Ok(()),
                Action::Send(text) => {
                    // A refused prompt is said under the transcript rather
                    // than thrown: a console that closed itself on one bad
                    // post would take a half-typed conversation with it.
                    if let Err(reason) = host.send(text.clone()).await {
                        app.refused(&text, &reason);
                    }
                }
                Action::Answer {
                    question_id,
                    answer,
                } => {
                    if let Err(reason) = host.answer(&question_id, answer).await {
                        app.failed(&reason);
                    }
                }
                Action::Cancel => host.cancel().await,
                Action::Copy(text) => host.copy(text),
            },
            Step::Ended | Step::Closed => return Ok(()),
            Step::Tick => app.tick(),
        }
    }
}
