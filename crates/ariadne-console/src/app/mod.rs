//! The full-screen console: a session's trajectory (040) drawn as turns on
//! the whole screen, with the input box under it.
//!
//! The screen has two zones. The transcript zone is every turn of the
//! trajectory: its prompt, one activity line, its notices and its answer,
//! and the step rows of a turn that is open. A step opens in place to its
//! detail, and a subagent to its own steps. The input zone is the box the
//! next prompt is typed into, with the plan, a pending permission or an open
//! question above it and the status row under it. One zone at a time has the
//! keys, and Tab moves them (008, rules 39 to 56).
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
    MAIN_LANE, PermissionState, PermissionStepDto, PlanDto, PlanVariant, QuestionState,
    QuestionStepDto, StepDto, ToolCallDetailDto, TrajectorySnapshotDto, TrajectoryUpdateDto,
    TurnDto, TurnState,
};

use crate::trajectory::{Printer, Touched, View, step_id, text};
use crate::transcript::{self, FormField};

mod commands;
mod detail;
mod draw;
mod form;
mod input;
mod status;

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
    /// a redial brings again. The optional prompt history comes from the host
    /// when its event window reaches beyond the trajectory page.
    Snapshot(Box<TrajectorySnapshotDto>, Option<Vec<String>>),
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

    /// Every step of the subagent `subagent_id` in the turn `turn_id`,
    /// oldest first. A host that cannot read them says so, and the
    /// subagent opens to the steps the view holds.
    fn subagent_steps(
        &mut self,
        turn_id: &str,
        subagent_id: &str,
    ) -> impl Future<Output = Result<Vec<StepDto>, String>> + Send {
        let _ = (turn_id, subagent_id);
        std::future::ready(Err("this console reads no subagent steps".to_string()))
    }

    /// The typed detail of the tool call step `step_id`, with its raw input
    /// and raw output where `raw` asks for them (040). A host that cannot
    /// read it says so, and the step opens to its preview.
    fn detail(
        &mut self,
        step_id: &str,
        raw: bool,
    ) -> impl Future<Output = Result<ToolCallDetailDto, String>> + Send {
        let _ = (step_id, raw);
        std::future::ready(Err("this console reads no step detail".to_string()))
    }

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
    /// The rows of the plan above the input.
    plan: Rect,
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

/// The detail of a tool call step as the host read it, or why it could not.
#[derive(Debug, Clone)]
enum Detail {
    Read(Box<ToolCallDetailDto>),
    Failed(String),
}

/// A read of the detail of a step: its id, and whether it asks for the raw
/// input and output.
type DetailLoad = (String, bool);

/// A subagent of a turn: the turn id and the subagent id.
type Lane = (String, String);

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
    /// The steps of the subagents of turns, as the host read them.
    lanes: HashMap<Lane, Vec<StepDto>>,
    /// The subagents whose steps the host is asked for, in that order.
    lane_loads: VecDeque<Lane>,
    /// The subagents asked for once, at the step count they had then.
    asked_lanes: HashSet<(Lane, u64)>,
    /// The detail of tool call steps, by step id.
    details: HashMap<String, Detail>,
    /// The details the host is asked for, in that order.
    detail_loads: VecDeque<DetailLoad>,
    /// The details asked for once, at the `detail_rev` of their step.
    asked_details: HashSet<(String, u64, bool)>,
    /// The tool call steps that show their raw input and output.
    raw: HashSet<String>,
    /// The open steps that show their detail whole, with no fold.
    whole: HashSet<Item>,
    /// The plan above the input shows its checklist.
    plan_open: bool,
    /// The first entry line of the checklist on the screen. The wheel over
    /// the plan moves it, and the draw keeps it in range.
    plan_scroll: Cell<usize>,
    /// The model the session was launched with, as the host read it: what
    /// the status row names where the agent reports no model option.
    model: Option<String>,
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
    question_failures: u8,
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
            lanes: HashMap::new(),
            lane_loads: VecDeque::new(),
            asked_lanes: HashSet::new(),
            details: HashMap::new(),
            detail_loads: VecDeque::new(),
            asked_details: HashSet::new(),
            raw: HashSet::new(),
            whole: HashSet::new(),
            plan_open: false,
            plan_scroll: Cell::new(0),
            model: None,
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
            question_failures: 0,
            pending: VecDeque::new(),
            known: HashSet::new(),
            notices: Vec::new(),
            shown: BTreeMap::new(),
            armed: false,
            tick: 0,
        }
    }

    /// The model the session was launched with, `<agent>:<model>`, which
    /// the status row names where the agent reports no model option.
    pub fn stored_model(&mut self, model: &str) {
        let model = model.split_once(':').map_or(model, |(_, model)| model);
        self.model = (!model.is_empty()).then(|| model.to_string());
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
            Frame::Snapshot(snapshot, history) => {
                self.question_failures = 0;
                self.asked_questions.clear();
                let view = View::new(*snapshot);
                self.live = true;
                let ids: Vec<String> = view.turns().iter().map(|turn| turn.id.clone()).collect();
                self.input.sync_history(history.unwrap_or_else(|| {
                    view.turns()
                        .iter()
                        .filter(|turn| from_console(turn))
                        .filter_map(prompt_text)
                        .collect()
                }));
                self.view = Some(view);
                for id in ids {
                    self.saw(&id);
                }
            }
            Frame::Update(update) => {
                let Some(view) = self.view.as_mut() else {
                    return false;
                };
                match view.apply(update) {
                    Ok(touched) => {
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
        let open_steps: Vec<(String, String)> = self
            .open
            .iter()
            .filter_map(|item| match item {
                Item::Step { turn, step } => Some((turn.clone(), step.clone())),
                Item::Turn(_) => None,
            })
            .collect();
        for (turn, step) in open_steps {
            match self.step(&turn, &step).cloned() {
                Some(StepDto::ToolCall(call)) => {
                    let raw = self.raw.contains(&call.id);
                    let held = match self.details.get(&call.id) {
                        Some(Detail::Read(detail)) => {
                            detail.detail_rev >= call.detail_rev && (!raw || detail.raw.is_some())
                        }
                        Some(Detail::Failed(_)) | None => false,
                    };
                    if !held
                        && self
                            .asked_details
                            .insert((call.id.clone(), call.detail_rev, raw))
                    {
                        self.detail_loads.push_back((call.id.clone(), raw));
                    }
                }
                Some(StepDto::Subagent(subagent)) => {
                    let lane = (turn, subagent.subagent_id.clone());
                    if self
                        .asked_lanes
                        .insert((lane.clone(), subagent.activity.steps))
                    {
                        self.lane_loads.push_back(lane);
                    }
                }
                _ => {}
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

    /// The next subagent whose steps the host should read.
    fn next_lane(&mut self) -> Option<Lane> {
        self.lane_loads.pop_front()
    }

    /// The steps the host read for a subagent of a turn.
    fn lane_read(&mut self, lane: Lane, steps: Vec<StepDto>) {
        self.lanes.insert(lane, steps);
        self.settle();
    }

    /// The next step whose detail the host should read.
    fn next_detail(&mut self) -> Option<DetailLoad> {
        self.detail_loads.pop_front()
    }

    /// The detail the host read for a step, or why it could not.
    fn detail_read(&mut self, step_id: String, detail: Result<ToolCallDetailDto, String>) {
        let detail = match detail {
            Ok(detail) => Detail::Read(Box::new(detail)),
            Err(reason) => Detail::Failed(reason),
        };
        self.details.insert(step_id, detail);
        self.settle();
    }

    /// Whether an open question waits whose form the console has not read:
    /// true once for each such question.
    fn wants_questions(&mut self) -> bool {
        if self.question_failures >= 3 {
            return false;
        }
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

    /// The steps of a subagent of a turn: those the host read, with the
    /// live steps of its lane that the view holds while the turn runs.
    fn lane_steps<'a>(&'a self, turn: &'a TurnDto, subagent: &str) -> Vec<&'a StepDto> {
        let mut steps: Vec<&StepDto> = self
            .lanes
            .get(&(turn.id.clone(), subagent.to_string()))
            .map(|steps| steps.iter().collect())
            .unwrap_or_default();
        if turn.state == TurnState::Running {
            for live in turn.steps.iter().filter(|step| lane_of(step) == subagent) {
                match steps.iter().position(|held| step_id(held) == step_id(live)) {
                    Some(at) => steps[at] = live,
                    None => steps.push(live),
                }
            }
        }
        steps
    }

    /// The step rows an open turn shows, each at its depth: the steps of
    /// the session's own lane, and under an open subagent its own steps. A
    /// permission that was answered is a mark on the row of its call, and
    /// has no row of its own where that call has one.
    fn shown_steps<'a>(&'a self, turn: &'a TurnDto) -> Option<Vec<(usize, &'a StepDto)>> {
        let steps = self.steps_of(turn)?;
        let announced: HashSet<&str> = steps
            .iter()
            .filter_map(|step| match step {
                StepDto::Subagent(subagent) => Some(subagent.subagent_id.as_str()),
                _ => None,
            })
            .collect();
        // The notices the turn carries show above its steps (rule 55), so a
        // step that is one of them has no second row.
        let noticed: HashSet<&str> = turn.notices.iter().map(step_id).collect();
        let top: Vec<&StepDto> = steps
            .iter()
            .filter(|step| lane_of(step) == MAIN_LANE || !announced.contains(lane_of(step)))
            .filter(|step| !noticed.contains(step_id(step)))
            .collect();
        let mut shown = Vec::new();
        self.push_steps(turn, top, 0, &mut shown);
        Some(shown)
    }

    fn push_steps<'a>(
        &'a self,
        turn: &'a TurnDto,
        steps: Vec<&'a StepDto>,
        depth: usize,
        shown: &mut Vec<(usize, &'a StepDto)>,
    ) {
        let calls: HashSet<&str> = steps
            .iter()
            .filter_map(|step| match step {
                StepDto::ToolCall(call) => call.permission.as_deref(),
                _ => None,
            })
            .collect();
        for step in steps {
            if let StepDto::Permission(permission) = step
                && permission.state != PermissionState::Pending
                && calls.contains(permission.id.as_str())
            {
                continue;
            }
            shown.push((depth, step));
            if let StepDto::Subagent(subagent) = step
                && self.open.contains(&Item::Step {
                    turn: turn.id.clone(),
                    step: subagent.id.clone(),
                })
                && depth < 8
            {
                let children = self.lane_steps(turn, &subagent.subagent_id);
                self.push_steps(turn, children, depth + 1, shown);
            }
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
                for (_, step) in self.shown_steps(turn).unwrap_or_default() {
                    items.push(Item::Step {
                        turn: turn.id.clone(),
                        step: step_id(step).to_string(),
                    });
                }
            }
        }
        items
    }

    /// A step of a turn, of its own lane or of one of its subagents.
    fn step(&self, turn: &str, step: &str) -> Option<&StepDto> {
        let turn = self.view.as_ref()?.turn(turn)?;
        let own = self.steps_of(turn).unwrap_or_default().iter();
        let lanes = self
            .lanes
            .iter()
            .filter(|((held, _), _)| *held == turn.id)
            .flat_map(|(_, steps)| steps.iter());
        own.chain(turn.steps.iter())
            .chain(lanes)
            .find(|held| step_id(held) == step)
    }

    /// The permission that asked about a call, where the turn holds it.
    fn permission_of<'a>(&'a self, turn: &'a TurnDto, id: &str) -> Option<&'a PermissionStepDto> {
        let lanes = self
            .lanes
            .iter()
            .filter(|((held, _), _)| *held == turn.id)
            .flat_map(|(_, steps)| steps.iter());
        self.steps_of(turn)
            .unwrap_or_default()
            .iter()
            .chain(turn.steps.iter())
            .chain(turn.attention.iter())
            .chain(lanes)
            .find_map(|step| match step {
                StepDto::Permission(permission) if permission.id == id => Some(permission),
                _ => None,
            })
    }

    /// The plan above the input: the newest plan with an entry still open.
    fn plan(&self) -> Option<&PlanDto> {
        self.view
            .as_ref()?
            .plans()
            .iter()
            .filter(|plan| {
                !plan.removed
                    && plan.variant == PlanVariant::Entries
                    && plan.entries.iter().any(|entry| entry.status != "completed")
            })
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
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
        if ctrl && key.code == KeyCode::Char('t') {
            self.plan_open = !self.plan_open && self.plan().is_some();
            return Action::None;
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
                    // An open step shows its detail whole at the second
                    // press.
                    if matches!(cursor, Item::Step { .. }) && self.open.contains(&cursor) {
                        self.whole.insert(cursor.clone());
                    }
                    self.open_item(cursor);
                }
            }
            KeyCode::Char('r') if plain => {
                if let Some(Item::Step { turn, step }) = self.cursor.clone()
                    && let Some(StepDto::ToolCall(call)) = self.step(&turn, &step)
                {
                    let id = call.id.clone();
                    if !self.raw.remove(&id) {
                        self.raw.insert(id);
                    }
                    self.open_item(Item::Step { turn, step });
                }
            }
            KeyCode::Left => match self.cursor.clone() {
                Some(cursor) if self.open.contains(&cursor) => {
                    self.open.remove(&cursor);
                    self.whole.remove(&cursor);
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
        if self.open.remove(&item) {
            self.whole.remove(&item);
        } else {
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

    /// Whether `row` is on the open checklist of the plan.
    fn over_plan(&self, row: u16) -> bool {
        let plan = self.layout.borrow().plan;
        self.plan_open && row >= plan.y && row < plan.y + plan.height
    }

    fn mouse(&mut self, mouse: MouseEvent) -> Action {
        match mouse.kind {
            // The wheel over an open checklist scrolls the plan, and
            // anywhere else the transcript.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.over_plan(mouse.row) => {
                let at = self.plan_scroll.get();
                self.plan_scroll.set(match mouse.kind {
                    MouseEventKind::ScrollUp => at.saturating_sub(WHEEL),
                    _ => at + WHEEL,
                });
            }
            MouseEventKind::ScrollUp => self.scroll_by(WHEEL, false),
            MouseEventKind::ScrollDown => self.scroll_by(WHEEL, true),
            MouseEventKind::Down(MouseButton::Left) => {
                let plan = self.layout.borrow().plan;
                if mouse.row >= plan.y && mouse.row < plan.y + plan.height {
                    self.plan_open = !self.plan_open;
                    return Action::None;
                }
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

/// The lane a step is in: the session's own, or a subagent's id.
fn lane_of(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.lane,
        StepDto::Thought(step) => &step.lane,
        StepDto::ToolCall(step) => &step.lane,
        StepDto::Permission(step) => &step.lane,
        StepDto::Question(step) => &step.lane,
        StepDto::Note(step) => &step.lane,
        StepDto::Error(step) => &step.lane,
        StepDto::Subagent(step) => &step.lane,
        StepDto::Compaction(step) => &step.lane,
    }
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
        while let Some(lane) = app.next_lane() {
            // A host that cannot read them leaves the subagent with the
            // live steps the view holds.
            let steps = host.subagent_steps(&lane.0, &lane.1).await;
            app.lane_read(lane, steps.unwrap_or_default());
        }
        while let Some((step, raw)) = app.next_detail() {
            let detail = host.detail(&step, raw).await;
            app.detail_read(step, detail);
        }
        if app.wants_questions() {
            match host.questions().await {
                Ok(questions) => app.questions_read(questions),
                Err(reason) => {
                    app.question_failures += 1;
                    app.asked_questions.clear();
                    app.failed(&reason);
                }
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
