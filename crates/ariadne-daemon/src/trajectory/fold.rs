//! The fold: a session's events, one at a time, into turns and the ops that
//! say what each event changed (040).
//!
//! A fold takes two kinds of input. A full event carries its payload, and
//! makes and changes steps. A head carries only the event's facts
//! (`ariadne_core::event_facts`), and changes the index alone. The index of
//! a turn — its step openers, its calls, its counts, its pending
//! permissions and questions — is kept from both by the one path below, so a
//! turn read as a few full events among many heads counts what a turn read
//! whole counts.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::trajectory::{
    ActivityDto, CompactionStatus, CompactionStepDto, ContentBlockDto, ContextWindowDto,
    ErrorStepDto, MAIN_LANE, NoteSeverity, NoteStepDto, PermissionAnswerDto, PermissionState,
    PermissionStepDto, PermissionToolDto, PlanDto, PlanEntryDto, PlanVariant, PromptDto,
    QuestionOptionDto, QuestionState, QuestionStepDto, SessionStateDto, StepDto, SubagentStepDto,
    ToolCallStatus, TrajectoryOpDto, TrajectorySnapshotDto, TurnDto, TurnState,
};
use ariadne_core::event_facts::{EventFacts, facts_of, lane_of as event_lane};
use ariadne_store::{EventHead, SessionDisplayState};

use super::steps::{
    append_text, apply_display, blocks_of, commands_of, content_of, is_terminal, lane_of,
    locations_of, merge_call, merge_call_facts, new_call, permission_options, status_of, step_id,
    string_at, subagent_ref, subject_of, text_step,
};

/// How many errors a turn's `attention` holds, the newest first: a screen of
/// them. `attention_more` counts the rest, and the steps route returns them.
pub(crate) const ATTENTION_ERRORS: usize = 20;

/// How many waiting permissions and questions a turn's `attention` holds,
/// the newest first: the read limit of 040 rule 16. A hundred is five
/// screens, and with the 20 errors and a running turn's window it fits the
/// payload budget of one read, so a cold read and a warm hub hold the same
/// ones. `attention_more` counts the rest, and the steps route returns them.
pub(crate) const ATTENTION_WAITS: usize = 100;

/// How many warning notes and compactions a turn's `notices` holds, the
/// newest first: a screen of them, as for the errors of `attention`.
/// `notices_more` counts the rest, and the steps route returns them.
pub(crate) const NOTICES: usize = 20;

/// How many of its last steps a running turn carries. Fifty steps are about
/// one screen of a client; the rest are a page of the steps route away.
pub(crate) const STEP_WINDOW: usize = 50;

/// A session's events, folded into turns.
#[derive(Debug, Clone)]
pub(crate) struct Fold {
    session_id: String,
    turns: Vec<Turn>,
    /// Turns older than `turns` that hold attention: kept so that a later
    /// reply, resolution or diagnosis finds its step. Never in a snapshot.
    outside: Vec<Turn>,
    has_older: bool,
    subagents: Vec<Subagent>,
    /// Each plan, by its lane and its ACP `plan_id`: none for a plan of the
    /// old `plan` kind, one to a lane.
    plans: Vec<(PlanKey, PlanDto)>,
    /// The id of the first stored event of each plan, from the heads of a
    /// read: a plan whose first payload the read did not load keeps it.
    plan_firsts: HashMap<PlanKey, String>,
    /// Each compaction, by its lane and its compaction id: the step its
    /// first event opened, in whichever turn that was. A later update
    /// changes that step, also across a prompt.
    compactions: HashMap<(String, String), CompactionAt>,
    session: SessionStateDto,
    /// The permissions no reply has answered yet, oldest first. A reply
    /// answers the newest.
    pending: Vec<Pending>,
    /// The open questions, by question or request id: the turn and the step.
    questions: HashMap<String, (String, String)>,
    /// Whether the current launch reported a display state, its commands
    /// even where they are none. A stored `available_commands_update`, from
    /// a daemon before the display state, then changes nothing.
    display_commands: bool,
}

#[derive(Debug, Clone)]
struct Turn {
    id: String,
    prompt: Option<PromptDto>,
    state: TurnState,
    stop_reason: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    /// The steps the fold holds: what full events made.
    steps: Vec<StepDto>,
    /// The index of each lane, from every event of the turn.
    lanes: HashMap<String, Lane>,
}

/// What a lane of a turn did, counted from every event, full or head.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lane {
    /// The steps the lane opened, in order.
    pub(crate) openers: Vec<Opener>,
    /// Each call by its ACP id: its state.
    calls: HashMap<String, Call>,
    thoughts: u64,
    subagents: BTreeSet<String>,
    items: BTreeSet<String>,
    pub(crate) errors: Vec<String>,
    /// The warning notes and the compactions the lane opened, in order: the
    /// steps of a turn's `notices`.
    pub(crate) notices: Vec<String>,
    /// The run of text that streams now: a thought or not, and its step.
    streaming: Option<(bool, String)>,
    /// The id of the newest message step: the turn's answer.
    answer: Option<String>,
    /// The newest event of each step after its opener — a call's last
    /// update, a permission's reply, a question's resolution, an error's
    /// diagnosis — by the step's id.
    pub(crate) follows: HashMap<String, String>,
    /// The newest update of each call that carries output text, and the
    /// newest that carries diffs, by the call's step: the preview and the
    /// diff stat come from their payloads, the rest from the facts.
    pub(crate) outputs: HashMap<String, String>,
    pub(crate) diffs: HashMap<String, String>,
}

/// One step a lane opened: its id on the wire, and the stored row that
/// opened it. A step a live event opened has no row until one is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opener {
    pub(crate) step: String,
    pub(crate) row: Option<String>,
}

impl Lane {
    /// The stored rows a read loads to show a step: its opener and its
    /// newest change.
    pub(crate) fn rows(&self, step: &str) -> Vec<String> {
        let opener = self
            .openers
            .iter()
            .find(|opener| opener.step == step)
            .and_then(|opener| opener.row.clone());
        opener
            .into_iter()
            .chain(self.outputs.get(step).cloned())
            .chain(self.diffs.get(step).cloned())
            .chain(self.follows.get(step).cloned())
            .collect()
    }
}

#[derive(Debug, Clone)]
struct Call {
    step: String,
    status: ToolCallStatus,
    kind: Option<String>,
    paths: Vec<String>,
}

/// Where a compaction's step is: the id of its first event, and the turn
/// that holds it. A read seeds a compaction whose first event comes before
/// the turns it holds with no turn: its updates then change nothing here.
#[derive(Debug, Clone)]
struct CompactionAt {
    step: String,
    turn: Option<String>,
}

#[derive(Debug, Clone)]
struct Pending {
    id: String,
    /// The turn of the request; empty for one outside every turn the fold
    /// reads.
    turn: String,
    facts: EventFacts,
}

#[derive(Debug, Clone)]
struct Subagent {
    step: SubagentStepDto,
    /// The newest `subagent_update` of it.
    latest: Option<String>,
    /// Whether its first event opened its step. A read seeds a subagent by
    /// its first event before it folds anything, so that an event of a
    /// later turn finds the subagent and opens no step of its own; the
    /// step opens where that first event folds.
    opened: bool,
}

/// One event, as the fold reads it: its facts always, its payload where the
/// read loaded it.
struct Input<'a> {
    id: &'a str,
    kind: &'a str,
    created_at: &'a str,
    lane: String,
    facts: EventFacts,
    full: Option<&'a AgentEventDto>,
    /// Whether the store holds the event, or it is live.
    stored: bool,
}

impl Input<'_> {
    /// The stored row of the event, where the store holds it.
    fn row(&self) -> Option<&str> {
        self.stored.then_some(self.id)
    }
}

/// What the selection of a read needs to know of one turn.
#[derive(Debug, Clone)]
pub(crate) struct TurnIndex {
    pub(crate) id: String,
    pub(crate) running: bool,
    pub(crate) answer: Option<String>,
    /// The steps that wait on a person: pending permissions and open
    /// questions.
    pub(crate) actions: Vec<String>,

    pub(crate) lanes: HashMap<String, Lane>,
}

/// The state of a session as its row says it, before any event: its
/// status, attention, usage and context, and the display state its current
/// launch reported (021, rule 24), where it reported one.
pub(crate) fn session_state(
    session: &SessionDto,
    display: Option<&SessionDisplayState>,
) -> SessionStateDto {
    let mut state = row_state(session);
    if let Some(display) = display {
        apply_display(
            &mut state,
            &serde_json::to_value(display).expect("a display state is JSON"),
        );
    }
    state
}

/// The fields of a session's state that its row holds.
fn row_state(session: &SessionDto) -> SessionStateDto {
    SessionStateDto {
        status: session.status,
        attention_reason: session.attention_reason,
        running_turn_id: None,
        title: None,
        mode: None,
        config_options: Vec::new(),
        context: session
            .context_used
            .zip(session.context_size)
            .map(|(used, size)| ContextWindowDto { used, size }),
        usage: session.usage,
        cost: None,
        commands: Vec::new(),
    }
}

/// A plan's lane and its ACP `plan_id`.
type PlanKey = (String, Option<String>);

/// The entries of a plan of entries, or of a `plan` event of the old kind.
fn entries_of(plan: &Value) -> Vec<PlanEntryDto> {
    plan.get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| PlanEntryDto {
            content: string_at(entry, "/content").unwrap_or_default(),
            status: string_at(entry, "/status").unwrap_or_else(|| "pending".into()),
            priority: string_at(entry, "/priority"),
        })
        .collect()
}

/// The facts of a head, as stored.
fn head_facts(head: &EventHead) -> EventFacts {
    serde_json::from_str(&head.facts).unwrap_or_default()
}

impl Fold {
    pub(crate) fn new(session_id: &str, session: SessionStateDto) -> Self {
        Self {
            display_commands: false,
            session_id: session_id.to_string(),
            turns: Vec::new(),
            outside: Vec::new(),
            has_older: false,
            subagents: Vec::new(),
            plans: Vec::new(),
            plan_firsts: HashMap::new(),
            compactions: HashMap::new(),
            session,
            pending: Vec::new(),
            questions: HashMap::new(),
        }
    }

    /// Whether the current launch reported a display state: its commands
    /// then stand, even an empty list, against a stored list of an older
    /// daemon.
    pub(crate) fn set_display_reported(&mut self, reported: bool) {
        self.display_commands = reported;
    }

    /// Name the plan of `lane` and `plan` by `first`, the id of its first
    /// stored event, whether or not the read loads that event.
    pub(crate) fn seed_plan(&mut self, lane: &str, plan: Option<String>, first: &str) {
        self.plan_firsts
            .entry((lane.to_string(), plan))
            .or_insert_with(|| first.to_string());
    }

    /// Name the compaction of `lane` and `compaction_id` by `first`, the id
    /// of its first stored event. Its first event, where the fold reads it,
    /// opens its step; an update without it opens none.
    pub(crate) fn seed_compaction(&mut self, lane: &str, compaction_id: &str, first: &str) {
        self.compactions
            .entry((lane.to_string(), compaction_id.to_string()))
            .or_insert_with(|| CompactionAt {
                step: first.to_string(),
                turn: None,
            });
    }

    /// Know the subagent `subagent_id` by `first`, the id of the first event
    /// of the session that names it, before any event folds.
    pub(crate) fn seed_subagent(&mut self, subagent_id: &str, first: &str) {
        if self
            .subagents
            .iter()
            .any(|child| child.step.subagent_id == subagent_id)
        {
            return;
        }
        self.subagents.push(Subagent {
            step: SubagentStepDto {
                id: first.to_string(),
                turn_id: String::new(),
                lane: MAIN_LANE.into(),
                created_at: String::new(),
                subagent_id: subagent_id.to_string(),
                title: subagent_id.to_string(),
                task: None,
                state: None,
                can_cancel: false,
                started_at: String::new(),
                ended_at: None,
                activity: ActivityDto::default(),
            },
            latest: None,
            opened: false,
        });
    }

    /// Whether turns older than the fold's first exist in the store.
    pub(crate) fn set_has_older(&mut self, has_older: bool) {
        self.has_older = has_older;
    }

    /// Fold one full event in, and answer what it changed. `stored` says
    /// whether the store holds it, or it is live.
    pub(crate) fn apply(&mut self, event: &AgentEventDto, stored: bool) -> Vec<TrajectoryOpDto> {
        let input = Input {
            id: &event.id,
            kind: &event.kind,
            created_at: &event.created_at,
            lane: event_lane(&event.kind, &event.payload).unwrap_or_else(|| MAIN_LANE.into()),
            facts: facts_of(&event.kind, &event.payload),
            full: Some(event),
            stored,
        };
        let mut ops = Ops::default();
        self.step(&input, &mut ops);
        self.finish(ops)
    }

    /// Fold one head in: the index changes, and no step does.
    pub(crate) fn apply_head(&mut self, head: &EventHead) {
        let input = Input {
            id: &head.id,
            kind: &head.kind,
            created_at: &head.created_at,
            lane: head.lane.clone().unwrap_or_else(|| MAIN_LANE.into()),
            facts: head_facts(head),
            full: None,
            stored: true,
        };
        self.step(&input, &mut Ops::default());
    }

    /// Pair a permission or a question outside the turns the fold reads:
    /// the stack of pending permissions and the open questions change, and
    /// nothing else.
    pub(crate) fn pair_head(&mut self, head: &EventHead) {
        let facts = head_facts(head);
        match head.kind.as_str() {
            "permission_request" => self.pending.push(Pending {
                id: head.id.clone(),
                turn: String::new(),
                facts,
            }),
            "permission.replied" => {
                self.pending.pop();
            }
            "question.resolved" | "agent_request.resolved" => {
                if let Some(id) = facts.question.or(facts.request) {
                    self.questions.remove(&id);
                }
            }
            _ => {}
        }
    }

    /// Move every turn older than `turn_id` out of the snapshot, keeping
    /// those that hold attention for the replies that may still come.
    pub(crate) fn retire_before(&mut self, turn_id: &str) {
        let keep = self
            .turns
            .iter()
            .position(|turn| turn.id.as_str() >= turn_id)
            .unwrap_or(self.turns.len());
        let older: Vec<Turn> = self.turns.drain(..keep).collect();
        if !older.is_empty() {
            self.has_older = true;
        }
        self.outside
            .extend(older.into_iter().filter(Turn::holds_attention));
        self.outside.sort_by(|a, b| a.id.cmp(&b.id));
    }

    /// Forget the turn `turn_id`: a page folds the prompt after it to close
    /// the turn before, and then drops the turn that prompt opened.
    pub(crate) fn drop_turn(&mut self, turn_id: &str) {
        self.turns.retain(|turn| turn.id != turn_id);
        if self.session.running_turn_id.as_deref() == Some(turn_id) {
            self.session.running_turn_id = None;
        }
    }

    /// Forget all but the newest `keep` turns; the turns that hold
    /// attention stay, outside the snapshot.
    pub(crate) fn prune(&mut self, keep: usize) {
        if self.turns.len() > keep {
            let first = self.turns[self.turns.len() - keep].id.clone();
            self.retire_before(&first);
        }
    }

    /// What the selection of a read needs of every turn, oldest first.
    pub(crate) fn index(&self) -> Vec<TurnIndex> {
        self.turns
            .iter()
            .map(|turn| TurnIndex {
                id: turn.id.clone(),
                running: turn.state == TurnState::Running,
                answer: turn
                    .lanes
                    .get(MAIN_LANE)
                    .and_then(|lane| lane.answer.clone()),
                actions: self.action_ids(turn),

                lanes: turn.lanes.clone(),
            })
            .collect()
    }

    /// Each subagent the fold knows, with the rows of its first event and
    /// its newest update, so that a read loads them.
    pub(crate) fn subagent_rows(&self) -> Vec<(String, Vec<String>)> {
        self.subagents
            .iter()
            .map(|child| {
                let rows = std::iter::once(child.step.id.clone())
                    .chain(child.latest.clone())
                    .collect();
                (child.step.subagent_id.clone(), rows)
            })
            .collect()
    }

    /// Fold a stored event that comes before every turn a read holds, in
    /// the turn `turn_id` it belongs to: a plan, or the first event of a
    /// subagent, keeps the turn it opened in.
    pub(crate) fn apply_in(&mut self, turn_id: &str, event: &AgentEventDto) {
        if self.turns.last().is_none_or(|turn| turn.id != turn_id) {
            self.end_running(
                TurnState::Interrupted,
                None,
                &event.created_at,
                &mut Ops::default(),
            );
            self.turns.push(Turn::new(
                turn_id,
                None,
                &event.created_at,
                TurnState::Completed,
            ));
        }
        self.apply(event, true);
    }

    /// The rows of the attention and notice steps the fold counts in its
    /// turns and does not hold: within each turn's newest `ATTENTION_WAITS`
    /// waiting permissions and questions, its newest `ATTENTION_ERRORS`
    /// errors and its newest `NOTICES` notices, those no full event built,
    /// with an error's diagnosis and a compaction's newest update. A hub
    /// loads them and fills them in, so its `attention` and its `notices`
    /// stay the windows a cold read holds.
    pub(crate) fn missing_attention(&self) -> Vec<String> {
        let mut rows = Vec::new();
        for turn in &self.turns {
            let held = |id: &str| turn.steps.iter().any(|step| step_id(step) == id);
            let mut waiting: Vec<&str> = self
                .pending
                .iter()
                .filter(|pending| pending.turn == turn.id)
                .map(|pending| pending.id.as_str())
                .chain(
                    self.questions
                        .values()
                        .filter(|(owner, _)| owner == &turn.id)
                        .map(|(_, step)| step.as_str()),
                )
                .collect();
            waiting.sort_unstable_by(|a, b| b.cmp(a));
            rows.extend(
                waiting
                    .into_iter()
                    .take(ATTENTION_WAITS)
                    .filter(|id| !held(id))
                    .map(str::to_string),
            );
            if let Some(lane) = turn.lanes.get(MAIN_LANE) {
                let errors = lane.errors.iter().rev().take(ATTENTION_ERRORS);
                let notices = lane.notices.iter().rev().take(NOTICES);
                for step in errors.chain(notices) {
                    if !held(step) {
                        rows.extend(lane.rows(step));
                    }
                }
            }
        }
        rows
    }

    /// Build the step of a stored event the index already counts, from its
    /// payload, in its place: what [`Self::missing_attention`] named. A
    /// diagnosis reads onto its error, and a compaction's later update onto
    /// its compaction. The index does not change.
    pub(crate) fn fill(&mut self, event: &AgentEventDto) -> Vec<TrajectoryOpDto> {
        let facts = facts_of(&event.kind, &event.payload);
        let later_update = event.kind == "compaction_update"
            && facts.compaction.as_ref().is_some_and(|compaction| {
                self.compactions
                    .get(&(MAIN_LANE.to_string(), compaction.clone()))
                    .is_some_and(|at| at.step != event.id)
            });
        if event.kind == "session.diagnosis" || later_update {
            return self.apply(event, true);
        }
        let Some(turn) = self.turns.iter_mut().find(|turn| {
            turn.lanes
                .get(MAIN_LANE)
                .is_some_and(|lane| lane.openers.iter().any(|opener| opener.step == event.id))
        }) else {
            return Vec::new();
        };
        if turn.steps.iter().any(|step| step_id(step) == event.id) {
            return Vec::new();
        }
        let step = match event.kind.as_str() {
            "permission_request" => permission_step(event, &turn.id, &facts),
            "session.error" => error_step(event, &turn.id),
            "question" | "agent_request" => {
                let key = facts
                    .question
                    .clone()
                    .or(facts.request.clone())
                    .unwrap_or_else(|| event.id.clone());
                StepDto::Question(question_step(event, &turn.id, &key))
            }
            "notice" | "session.pin_fallback" => {
                let (text, severity) = note_of(&event.kind, &facts, Some(event));
                note_step(event, &turn.id, MAIN_LANE, text, severity)
            }
            "compaction_update" => {
                compaction_step(event, &turn.id, MAIN_LANE, &facts, &self.session_id)
            }
            _ => return Vec::new(),
        };
        let at = turn
            .steps
            .iter()
            .position(|kept| step_id(kept) > event.id.as_str())
            .unwrap_or(turn.steps.len());
        turn.steps.insert(at, step.clone());
        let mut ops = Ops::default();
        ops.push(TrajectoryOpDto::ItemUpsert { step });
        ops.touch(&turn.id.clone());
        self.finish(ops)
    }

    /// The steps of a turn's lane that the fold holds, oldest first.
    pub(crate) fn held_steps(&self, turn_id: &str, lane: &str) -> Vec<StepDto> {
        self.turns
            .iter()
            .find(|turn| turn.id == turn_id)
            .map(|turn| {
                turn.steps
                    .iter()
                    .filter(|step| lane_of(step) == lane)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The trajectory at revision `rev`: the newest `limit` turns, or with a
    /// `subagent` the turns where it has steps, each carrying them.
    pub(crate) fn snapshot(
        &self,
        rev: u64,
        limit: usize,
        subagent: Option<&str>,
    ) -> TrajectorySnapshotDto {
        let turns: Vec<&Turn> = match subagent {
            Some(lane) => self
                .turns
                .iter()
                .filter(|turn| {
                    turn.lanes
                        .get(lane)
                        .is_some_and(|index| !index.openers.is_empty())
                })
                .collect(),
            None => self.turns.iter().collect(),
        };
        let skip = turns.len().saturating_sub(limit);
        TrajectorySnapshotDto {
            rev,
            session: self.session.clone(),
            turns: turns[skip..]
                .iter()
                .map(|turn| match subagent {
                    Some(lane) => turn.dto_with(lane, self.waiting_in(&turn.id)),
                    None => turn.dto(self.waiting_in(&turn.id)),
                })
                .collect(),
            has_older: self.has_older || skip > 0,
            subagents: self
                .subagents
                .iter()
                .filter(|child| child.opened)
                .map(|child| child.step.clone())
                .collect(),
            plans: self.plans.iter().map(|(_, plan)| plan.clone()).collect(),
        }
    }

    /// The session's row changed: its status, attention, usage or context.
    pub(crate) fn patch_session(&mut self, session: &SessionDto) -> Option<TrajectoryOpDto> {
        let mut next = self.session.clone();
        let row = row_state(session);
        next.status = row.status;
        next.attention_reason = row.attention_reason;
        next.usage = row.usage;
        next.context = row.context;
        (next != self.session).then(|| {
            self.session = next;
            self.session_op()
        })
    }

    fn finish(&self, ops: Ops) -> Vec<TrajectoryOpDto> {
        let mut out = ops.ops;
        for id in ops.turns {
            if let Some(turn) = self
                .turns
                .iter()
                .chain(&self.outside)
                .find(|turn| turn.id == id)
            {
                let mut header = turn.dto(self.waiting_in(&turn.id));
                header.steps.clear();
                out.push(TrajectoryOpDto::TurnUpsert { turn: header });
            }
        }
        out
    }

    /// How many permissions and questions of a turn wait, held or not.
    fn waiting_in(&self, turn_id: &str) -> usize {
        self.pending
            .iter()
            .filter(|pending| pending.turn == turn_id)
            .count()
            + self
                .questions
                .values()
                .filter(|(owner, _)| owner == turn_id)
                .count()
    }

    fn session_op(&self) -> TrajectoryOpDto {
        TrajectoryOpDto::SessionPatch {
            session: self.session.clone(),
        }
    }

    fn action_ids(&self, turn: &Turn) -> Vec<String> {
        let mut ids: Vec<String> = self
            .pending
            .iter()
            .filter(|pending| pending.turn == turn.id)
            .map(|pending| pending.id.clone())
            .collect();
        ids.extend(
            self.questions
                .values()
                .filter(|(owner, _)| owner == &turn.id)
                .map(|(_, step)| step.clone()),
        );
        ids.sort();
        ids
    }

    fn running(&self) -> Option<usize> {
        self.turns
            .iter()
            .rposition(|turn| turn.state == TurnState::Running)
    }

    /// The turn an event belongs to: the running one, else the newest one,
    /// else a turn the event opens with no prompt.
    fn turn_for(&mut self, input: &Input, ops: &mut Ops) -> usize {
        if let Some(at) = self.running().or(self.turns.len().checked_sub(1)) {
            return at;
        }
        self.turns.push(Turn::new(
            input.id,
            None,
            input.created_at,
            TurnState::Completed,
        ));
        ops.touch(input.id);
        self.turns.len() - 1
    }

    fn turn_mut(&mut self, turn_id: &str) -> Option<&mut Turn> {
        self.turns
            .iter_mut()
            .chain(self.outside.iter_mut())
            .find(|turn| turn.id == turn_id)
    }

    fn step(&mut self, input: &Input, ops: &mut Ops) {
        if input.lane != MAIN_LANE && input.kind != "available_commands_update" {
            let id = input.lane.clone();
            self.subagent_at(input, &id, ops);
        }
        match input.kind {
            "user_prompt_submit" => self.open_turn(input, ops),
            "agent_message_chunk" | "agent_thought_chunk" => self.chunk(input, ops),
            "agent_message" | "agent_thought" => self.whole(input, ops),
            "pre_tool_use" | "tool_call_update" | "post_tool_use" | "tool_call_abandoned" => {
                self.tool(input, ops)
            }
            "permission_request" => self.permission(input, ops),
            "permission.replied" => self.reply(input, ops),
            "question" | "agent_request" => self.question(input, ops),
            "question.resolved" | "agent_request.resolved" => self.resolved(input, ops),
            "session.error" => self.error(input, ops),
            "session.diagnosis" => self.diagnosis(input, ops),
            "stop" => {
                let state = match input.facts.stop_reason.as_deref() {
                    Some("cancelled") => TurnState::Cancelled,
                    _ => TurnState::Completed,
                };
                self.end_running(
                    state,
                    input.facts.stop_reason.clone(),
                    input.created_at,
                    ops,
                );
            }
            "session_start" | "session_end" => {
                self.end_running(TurnState::Interrupted, None, input.created_at, ops);
                let (text, severity) = note_of(input.kind, &input.facts, input.full);
                self.note(input, text, severity, ops);
            }
            "session.switched" | "session.pin_fallback" | "notice" => {
                let (text, severity) = note_of(input.kind, &input.facts, input.full);
                self.note(input, text, severity, ops);
            }
            "plan" | "plan_update" | "plan_removed" => {
                // A plan is named by its first event, a removal included, on
                // every path: the read seeds the same from the heads.
                self.plan_firsts
                    .entry((input.lane.clone(), input.facts.plan.clone()))
                    .or_insert_with(|| input.id.to_string());
                match input.kind {
                    "plan_removed" => self.plan_removed(input, ops),
                    _ => self.plan(input, ops),
                }
            }
            "subagent_update" => self.subagent(input, ops),
            "compaction_update" => self.compaction(input, ops),
            "available_commands_update" if input.lane == MAIN_LANE => {
                if let Some(event) = input.full
                    && !(input.stored && self.display_commands)
                {
                    self.session.commands = commands_of(&event.payload);
                    ops.push(self.session_op());
                }
            }
            "display_state_update" if input.lane == MAIN_LANE => {
                if let Some(event) = input.full {
                    let mut next = self.session.clone();
                    if apply_display(&mut next, &event.payload) {
                        self.display_commands = true;
                    }
                    if next != self.session {
                        self.session = next;
                        ops.push(self.session_op());
                    }
                }
            }
            _ => {}
        }
        if input.lane != MAIN_LANE {
            self.recount_subagent(&input.lane, ops);
        }
    }

    fn open_turn(&mut self, input: &Input, ops: &mut Ops) {
        self.end_running(TurnState::Interrupted, None, input.created_at, ops);
        let prompt = PromptDto {
            id: input.id.to_string(),
            source: input
                .full
                .and_then(|event| string_at(&event.payload, "/source")),
            prompt_kind: input
                .full
                .and_then(|event| string_at(&event.payload, "/prompt_kind")),
            text: input
                .full
                .and_then(|event| string_at(&event.payload, "/text")),
            content: input
                .full
                .map(|event| content_of(&event.payload, &self.session_id, &event.id))
                .unwrap_or_default(),
        };
        self.turns.push(Turn::new(
            input.id,
            Some(prompt),
            input.created_at,
            TurnState::Running,
        ));
        ops.touch(input.id);
        self.session.running_turn_id = Some(input.id.to_string());
        ops.push(self.session_op());
    }

    /// End the running turn, if one runs: its text stops streaming and the
    /// calls still open end with it.
    fn end_running(&mut self, state: TurnState, reason: Option<String>, at: &str, ops: &mut Ops) {
        let Some(index) = self.running() else {
            return;
        };
        let call_state = match state {
            TurnState::Cancelled => ToolCallStatus::Cancelled,
            _ => ToolCallStatus::Interrupted,
        };
        let turn = &mut self.turns[index];
        turn.state = state;
        turn.stop_reason = reason;
        turn.ended_at = Some(at.to_string());
        for lane in turn.lanes.values_mut() {
            lane.streaming = None;
            for call in lane.calls.values_mut() {
                if !is_terminal(call.status) {
                    call.status = call_state;
                }
            }
        }
        for step in &mut turn.steps {
            let changed = match step {
                StepDto::Message(message) if message.streaming => {
                    message.streaming = false;
                    true
                }
                StepDto::Thought(thought) if thought.streaming => {
                    thought.streaming = false;
                    true
                }
                StepDto::ToolCall(call) if !is_terminal(call.status) => {
                    call.status = call_state;
                    call.ended_at = Some(at.to_string());
                    call.detail_rev += 1;
                    true
                }
                _ => false,
            };
            if changed {
                ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            }
        }
        let id = turn.id.clone();
        ops.touch(&id);
        if self.session.running_turn_id.take().is_some() {
            ops.push(self.session_op());
        }
    }

    /// Stop the text streaming in a lane: something else happened there.
    fn end_streaming(&mut self, index: usize, lane: &str, ops: &mut Ops) {
        let turn = &mut self.turns[index];
        let Some((_, id)) = turn
            .lanes
            .get_mut(lane)
            .and_then(|lane| lane.streaming.take())
        else {
            return;
        };
        if let Some(step) = turn.steps.iter_mut().find(|step| step_id(step) == id) {
            match step {
                StepDto::Message(message) => message.streaming = false,
                StepDto::Thought(thought) => thought.streaming = false,
                _ => return,
            }
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
        }
    }

    /// A step opens in a lane: the index counts it, and a full event makes
    /// it.
    #[allow(clippy::too_many_arguments)]
    fn open_step(
        &mut self,
        index: usize,
        lane: &str,
        id: &str,
        row: Option<&str>,
        step: Option<StepDto>,
        ops: &mut Ops,
    ) {
        self.end_streaming(index, lane, ops);
        let turn = &mut self.turns[index];
        turn.lanes
            .entry(lane.to_string())
            .or_default()
            .openers
            .push(Opener {
                step: id.to_string(),
                row: row.map(str::to_string),
            });
        ops.touch(&turn.id);
        if let Some(step) = step {
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            turn.steps.push(step);
        }
    }

    fn chunk(&mut self, input: &Input, ops: &mut Ops) {
        let Some(event) = input.full else {
            return;
        };
        let index = self.turn_for(input, ops);
        let thought = input.kind == "agent_thought_chunk";
        let text = string_at(&event.payload, "/text").unwrap_or_default();
        let item = input.facts.item.clone();
        let turn = &mut self.turns[index];
        let streaming = turn
            .lanes
            .get(&input.lane)
            .and_then(|lane| lane.streaming.clone());
        // A chunk of an item whose whole already closed its step is late: the
        // whole holds its text, so it changes nothing.
        if let Some(item) = &item
            && turn
                .lanes
                .get(&input.lane)
                .is_some_and(|lane| lane.items.contains(item))
            && streaming.as_ref().is_none_or(|(_, id)| id != item)
        {
            return;
        }
        if let Some((kind, id)) = streaming
            && kind == thought
            && item.as_ref().is_none_or(|item| item == &id)
        {
            if let Some(step) = turn.steps.iter_mut().find(|step| step_id(step) == id) {
                match step {
                    StepDto::Message(message) => append_text(&mut message.content, &text),
                    StepDto::Thought(thought) => append_text(&mut thought.content, &text),
                    _ => {}
                }
            }
            ops.push(TrajectoryOpDto::TextAppend {
                turn_id: turn.id.clone(),
                item_id: id,
                text,
            });
            return;
        }
        // A run with no `item_id` is named after the stored event before it,
        // which every open of the session reads the same while it streams.
        // A run streams under its `item_id`. A run with none takes the id of
        // the chunk that opened it, which holds for this open alone.
        let id = item.unwrap_or_else(|| input.id.to_string());
        let step = text_step(
            thought,
            id.clone(),
            turn.id.clone(),
            &input.lane,
            event,
            vec![ContentBlockDto::Text { text }],
            true,
        );
        self.open_step(index, &input.lane, &id, None, Some(step), ops);
        let lane = self.turns[index]
            .lanes
            .entry(input.lane.clone())
            .or_default();
        lane.items.insert(id.clone());
        lane.streaming = Some((thought, id.clone()));
        if thought {
            lane.thoughts += 1;
        } else {
            lane.answer = Some(id);
        }
    }

    fn whole(&mut self, input: &Input, ops: &mut Ops) {
        let index = self.turn_for(input, ops);
        let thought = input.kind == "agent_thought";
        let content = input
            .full
            .map(|event| content_of(&event.payload, &self.session_id, &event.id));
        let item = input.facts.item.clone();
        let lane = self.turns[index]
            .lanes
            .entry(input.lane.clone())
            .or_default();
        // The step this whole closes: the one its `item_id` names, else the
        // run that streams in its lane.
        let closes = match &item {
            Some(item) if lane.items.contains(item) => Some(item.clone()),
            Some(_) => None,
            None => lane
                .streaming
                .clone()
                .filter(|(kind, _)| *kind == thought)
                .map(|(_, id)| id),
        };
        let Some(closed) = closes else {
            let id = item.unwrap_or_else(|| input.id.to_string());
            let turn_id = self.turns[index].id.clone();
            let step = input.full.map(|event| {
                text_step(
                    thought,
                    id.clone(),
                    turn_id,
                    &input.lane,
                    event,
                    content.clone().unwrap_or_default(),
                    false,
                )
            });
            self.open_step(index, &input.lane, &id, input.row(), step, ops);
            let lane = self.turns[index]
                .lanes
                .entry(input.lane.clone())
                .or_default();
            lane.items.insert(id.clone());
            if thought {
                lane.thoughts += 1;
            } else {
                lane.answer = Some(id);
            }
            return;
        };
        // A run with no `item_id` takes the id of its stored whole, which is
        // the id a read of the store gives it.
        let renamed = item.is_none() && closed != input.id;
        let id = if renamed {
            input.id.to_string()
        } else {
            closed.clone()
        };
        lane.streaming = None;
        lane.items.insert(id.clone());
        if let Some(opener) = lane
            .openers
            .iter_mut()
            .rev()
            .find(|opener| opener.step == closed)
        {
            opener.step = id.clone();
            match (&opener.row, input.row()) {
                (None, Some(row)) => opener.row = Some(row.to_string()),
                (Some(first), Some(row)) if first != row => {
                    lane.follows.insert(id.clone(), row.to_string());
                }
                _ => {}
            }
        }
        if !thought {
            lane.answer = Some(id.clone());
        }
        let turn = &mut self.turns[index];
        ops.touch(&turn.id);
        let Some(step) = turn.steps.iter_mut().find(|step| step_id(step) == closed) else {
            return;
        };
        match step {
            StepDto::Message(message) => {
                if renamed {
                    message.created_at = input.created_at.to_string();
                }
                message.id = id.clone();
                if let Some(content) = content {
                    message.content = content;
                }
                message.streaming = false;
                if message.message_id.is_none() {
                    message.message_id = input
                        .full
                        .and_then(|event| string_at(&event.payload, "/message_id"));
                }
            }
            StepDto::Thought(thought_step) => {
                if renamed {
                    thought_step.created_at = input.created_at.to_string();
                }
                thought_step.id = id.clone();
                if let Some(content) = content {
                    thought_step.content = content;
                }
                thought_step.streaming = false;
            }
            _ => {}
        }
        if renamed {
            ops.push(TrajectoryOpDto::ItemRemove {
                turn_id: turn.id.clone(),
                item_id: closed,
            });
        }
        ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
    }

    fn tool(&mut self, input: &Input, ops: &mut Ops) {
        let index = self.turn_for(input, ops);
        let call_id = input.facts.call.clone().unwrap_or_default();
        let open = self.turns[index]
            .lanes
            .get(&input.lane)
            .and_then(|lane| lane.calls.get(&call_id))
            .filter(|call| !is_terminal(call.status))
            .map(|call| call.step.clone());
        let target = match open {
            Some(step) if input.kind != "pre_tool_use" => {
                self.end_streaming(index, &input.lane, ops);
                step
            }
            _ => {
                let turn_id = self.turns[index].id.clone();
                let step = input.full.map(|event| {
                    let mut call = new_call(event, &turn_id, &input.lane, &call_id);
                    call.permission = self.pending.iter().rev().find_map(|pending| {
                        (pending.turn == turn_id
                            && !call_id.is_empty()
                            && pending.facts.call.as_deref() == Some(call_id.as_str()))
                        .then(|| pending.id.clone())
                    });
                    StepDto::ToolCall(call)
                });
                self.open_step(index, &input.lane, input.id, input.row(), step, ops);
                self.turns[index]
                    .lanes
                    .entry(input.lane.clone())
                    .or_default()
                    .calls
                    .insert(
                        call_id.clone(),
                        Call {
                            step: input.id.to_string(),
                            status: ToolCallStatus::Pending,
                            kind: None,
                            paths: Vec::new(),
                        },
                    );
                input.id.to_string()
            }
        };
        let turn = &mut self.turns[index];
        let lane = turn.lanes.entry(input.lane.clone()).or_default();
        if let Some(call) = lane.calls.get_mut(&call_id) {
            if let Some(status) = input.facts.status.as_deref().and_then(status_of) {
                call.status = status;
            }
            if input.facts.tool_kind.is_some() {
                call.kind = input.facts.tool_kind.clone();
            }
            if !input.facts.paths.is_empty() {
                call.paths = input.facts.paths.clone();
            }
        }
        if target != input.id
            && let Some(row) = input.row()
        {
            lane.follows.insert(target.clone(), row.to_string());
            if input.facts.output {
                lane.outputs.insert(target.clone(), row.to_string());
            }
            if input.facts.diffs {
                lane.diffs.insert(target.clone(), row.to_string());
            }
        }
        ops.touch(&turn.id);
        // An update whose payload the read did not load still changes its
        // call: by its facts, which hold every field but the output.
        if let Some(step) = turn.steps.iter_mut().find(|step| step_id(step) == target)
            && let StepDto::ToolCall(call) = step
        {
            match input.full {
                Some(event) => merge_call(call, event),
                None => merge_call_facts(call, &input.facts, input.created_at),
            }
            if target != input.id {
                call.detail_rev += 1;
            }
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
        }
    }

    fn permission(&mut self, input: &Input, ops: &mut Ops) {
        let index = self.turn_for(input, ops);
        let turn_id = self.turns[index].id.clone();
        let subagent = input.full.and_then(|event| subagent_ref(&event.payload));
        let step = input
            .full
            .map(|event| permission_step(event, &turn_id, &input.facts));
        self.open_step(index, MAIN_LANE, input.id, input.row(), step, ops);
        self.pending.push(Pending {
            id: input.id.to_string(),
            turn: turn_id,
            facts: input.facts.clone(),
        });
        // The open call the permission asks about names it.
        let call_lane = subagent.map_or_else(|| MAIN_LANE.to_string(), |child| child.id);
        let call_id = input.facts.call.clone().unwrap_or_default();
        let turn = &mut self.turns[index];
        let call_step = turn
            .lanes
            .get(&call_lane)
            .and_then(|lane| lane.calls.get(&call_id))
            .filter(|call| !call_id.is_empty() && !is_terminal(call.status))
            .map(|call| call.step.clone());
        if let Some(call_step) = call_step
            && let Some(step) = turn
                .steps
                .iter_mut()
                .find(|step| step_id(step) == call_step)
            && let StepDto::ToolCall(call) = step
        {
            call.permission = Some(input.id.to_string());
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
        }
    }

    fn reply(&mut self, input: &Input, ops: &mut Ops) {
        let Some(request) = self.pending.pop() else {
            return;
        };
        let removed = request.facts.ariadne && input.facts.daemon_allowed(&request.facts);
        let Some(turn) = self.turn_mut(&request.turn) else {
            return;
        };
        ops.touch(&turn.id);
        let lane = turn.lanes.entry(MAIN_LANE.into()).or_default();
        if let Some(row) = input.row() {
            lane.follows.insert(request.id.clone(), row.to_string());
        }
        if removed {
            // A permission on one of Ariadne's own tools that the daemon
            // allowed by itself asked nobody anything: the call's step says
            // what the agent did.
            lane.openers.retain(|opener| opener.step != request.id);
            if let Some(at) = turn
                .steps
                .iter()
                .position(|step| step_id(step) == request.id)
            {
                turn.steps.remove(at);
                ops.push(TrajectoryOpDto::ItemRemove {
                    turn_id: turn.id.clone(),
                    item_id: request.id.clone(),
                });
            }
            for step in &mut turn.steps {
                if let StepDto::ToolCall(call) = step
                    && call.permission.as_deref() == Some(request.id.as_str())
                {
                    call.permission = None;
                    ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
                }
            }
            return;
        }
        let Some(step) = turn
            .steps
            .iter_mut()
            .find(|step| step_id(step) == request.id)
        else {
            return;
        };
        let StepDto::Permission(permission) = step else {
            return;
        };
        let facts = &input.facts;
        let payload = input.full.map(|event| &event.payload);
        let chosen = payload
            .and_then(|payload| string_at(payload, "/console_option_id"))
            .or_else(|| facts.option.clone());
        let label = match (&facts.option, &chosen) {
            (None, _) => "cancelled".to_string(),
            (Some(id), chosen) => permission
                .options
                .iter()
                .find(|option| Some(&option.id) == chosen.as_ref())
                .map_or_else(|| id.clone(), |option| option.name.clone()),
        };
        permission.state = match facts.option {
            Some(_) => PermissionState::Answered,
            None => PermissionState::Cancelled,
        };
        if let Some(payload) = payload {
            if facts.decided_by.as_deref() == Some("ai")
                && let Some(note) = ariadne_api::permissions::ai_permission_note(payload)
            {
                permission.ai = Some(note);
            }
            if facts.decided_by.as_deref() == Some("learned") {
                permission.learned = match string_at(payload, "/learned_level").as_deref() {
                    Some("family") => Some(format!(
                        "family {}",
                        string_at(payload, "/learned_key").unwrap_or_default()
                    )),
                    Some(level) => Some(level.to_string()),
                    None => None,
                };
            }
        }
        permission.answer = Some(PermissionAnswerDto {
            option_id: facts.option.clone(),
            label,
            decided_by: facts.decided_by.clone(),
        });
        ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
    }

    fn question(&mut self, input: &Input, ops: &mut Ops) {
        let asks = input.kind == "question" || input.facts.asks;
        if !asks {
            let text = match input
                .full
                .and_then(|event| string_at(&event.payload, "/hint"))
            {
                Some(hint) => format!("waiting for you · {hint}"),
                None => "waiting for you".into(),
            };
            self.note(input, text, NoteSeverity::Info, ops);
            return;
        }
        let index = self.turn_for(input, ops);
        let turn_id = self.turns[index].id.clone();
        let key = input
            .facts
            .question
            .clone()
            .or(input.facts.request.clone())
            .unwrap_or_else(|| input.id.to_string());
        let step = input
            .full
            .map(|event| StepDto::Question(question_step(event, &turn_id, &key)));
        self.open_step(index, MAIN_LANE, input.id, input.row(), step, ops);
        self.questions.insert(key, (turn_id, input.id.to_string()));
    }

    fn resolved(&mut self, input: &Input, ops: &mut Ops) {
        let Some(key) = input.facts.question.clone().or(input.facts.request.clone()) else {
            return;
        };
        let Some((turn_id, step_id_of)) = self.questions.remove(&key) else {
            return;
        };
        let Some(turn) = self.turn_mut(&turn_id) else {
            return;
        };
        ops.touch(&turn_id);
        turn.lanes
            .entry(MAIN_LANE.into())
            .or_default()
            .follows
            .insert(step_id_of.clone(), input.id.to_string());
        let Some(step) = turn
            .steps
            .iter_mut()
            .find(|step| step_id(step) == step_id_of)
        else {
            return;
        };
        if let StepDto::Question(question) = step {
            let payload = input.full.map(|event| &event.payload);
            let action = payload
                .and_then(|payload| {
                    string_at(payload, "/action").or_else(|| string_at(payload, "/resolution"))
                })
                .unwrap_or_default();
            question.state = match action.as_str() {
                "decline" => QuestionState::Declined,
                "cancel" | "withdrawn" | "expired" => QuestionState::Cancelled,
                _ => QuestionState::Answered,
            };
            question.answer = payload.and_then(answer_of);
        }
        ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
    }

    /// An error step comes from its payload; the index counts every error,
    /// loaded or not.
    fn error(&mut self, input: &Input, ops: &mut Ops) {
        let index = self.turn_for(input, ops);
        let turn_id = self.turns[index].id.clone();
        let step = input.full.map(|event| error_step(event, &turn_id));
        self.open_step(index, MAIN_LANE, input.id, input.row(), step, ops);
        self.turns[index]
            .lanes
            .entry(MAIN_LANE.into())
            .or_default()
            .errors
            .push(input.id.to_string());
        self.end_running(TurnState::Failed, None, input.created_at, ops);
    }

    /// A diagnosis reads onto the error it names, from its payload.
    fn diagnosis(&mut self, input: &Input, ops: &mut Ops) {
        let Some(error_id) = input.facts.error.clone() else {
            return;
        };
        let Some(turn) = self
            .turns
            .iter_mut()
            .chain(self.outside.iter_mut())
            .find(|turn| {
                turn.lanes
                    .get(MAIN_LANE)
                    .is_some_and(|lane| lane.errors.contains(&error_id))
            })
        else {
            return;
        };
        ops.touch(&turn.id);
        if let Some(row) = input.row() {
            turn.lanes
                .entry(MAIN_LANE.into())
                .or_default()
                .follows
                .insert(error_id.clone(), row.to_string());
        }
        if let Some(event) = input.full
            && let Some(step) = turn.steps.iter_mut().find(|step| step_id(step) == error_id)
            && let StepDto::Error(error) = step
        {
            error.diagnosis = Some(
                ariadne_api::events::diagnosis_note(&event.payload)
                    .unwrap_or_else(|| event.summary.clone()),
            );
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
        }
    }

    fn note(&mut self, input: &Input, text: String, severity: NoteSeverity, ops: &mut Ops) {
        let index = self.turn_for(input, ops);
        let turn_id = self.turns[index].id.clone();
        let step = input
            .full
            .map(|event| note_step(event, &turn_id, &input.lane, text, severity));
        // A note is in the lane of its event: a subagent's notice is the
        // subagent's.
        self.open_step(index, &input.lane, input.id, input.row(), step, ops);
        if severity == NoteSeverity::Warning {
            self.turns[index]
                .lanes
                .entry(input.lane.clone())
                .or_default()
                .notices
                .push(input.id.to_string());
        }
    }

    /// A plan update: a `plan` of the old kind replaces the one plan of its
    /// lane, and a `plan_update` the plan of its lane and `plan_id`. The
    /// plan keeps the id of its first event.
    fn plan(&mut self, input: &Input, ops: &mut Ops) {
        let Some(event) = input.full else {
            return;
        };
        let key = (input.lane.clone(), input.facts.plan.clone());
        let (variant, entries, markdown, path) = match input.kind {
            "plan_update" => {
                let plan = event.payload.get("plan").unwrap_or(&Value::Null);
                match string_at(plan, "/type").as_deref() {
                    Some("markdown") => (
                        PlanVariant::Markdown,
                        Vec::new(),
                        string_at(plan, "/content"),
                        string_at(plan, "/path"),
                    ),
                    _ => (PlanVariant::Entries, entries_of(plan), None, None),
                }
            }
            _ => (PlanVariant::Entries, entries_of(&event.payload), None, None),
        };
        let plan_id = self
            .plans
            .iter()
            .find(|(kept, _)| *kept == key)
            .map(|(_, plan)| plan.plan_id.clone())
            .or_else(|| self.plan_firsts.get(&key).cloned())
            .unwrap_or_else(|| event.id.clone());
        let plan = PlanDto {
            plan_id,
            variant,
            entries,
            markdown,
            path,
            turn_id: self.plan_turn(),
            removed: false,
            updated_at: event.created_at.clone(),
        };
        match self.plans.iter_mut().find(|(kept, _)| *kept == key) {
            Some((_, kept)) => *kept = plan.clone(),
            None => self.plans.push((key, plan.clone())),
        }
        ops.push(TrajectoryOpDto::PlanUpsert { plan });
    }

    /// A `plan_removed` marks the plan of its lane and `plan_id` removed.
    fn plan_removed(&mut self, input: &Input, ops: &mut Ops) {
        let key = (input.lane.clone(), input.facts.plan.clone());
        let turn_id = self.plan_turn();
        let Some((_, plan)) = self.plans.iter_mut().find(|(kept, _)| *kept == key) else {
            return;
        };
        plan.removed = true;
        plan.turn_id = turn_id;
        plan.updated_at = input.created_at.to_string();
        ops.push(TrajectoryOpDto::PlanUpsert { plan: plan.clone() });
    }

    /// The turn a plan event changes a plan in: the running one, else the
    /// newest.
    fn plan_turn(&self) -> Option<String> {
        self.running()
            .or(self.turns.len().checked_sub(1))
            .map(|index| self.turns[index].id.clone())
    }

    fn subagent(&mut self, input: &Input, ops: &mut Ops) {
        let Some(id) = input.facts.subagent.clone() else {
            return;
        };
        let at = self.subagent_at(input, &id, ops);
        let child = &mut self.subagents[at];
        child.latest = Some(input.id.to_string());
        if let Some(event) = input.full {
            let payload = &event.payload;
            if let Some(title) = string_at(payload, "/subagent/title") {
                child.step.title = title;
            }
            if let Some(task) = string_at(payload, "/subagent/description") {
                child.step.task = Some(task);
            }
            child.step.can_cancel =
                payload.pointer("/capabilities/cancel") == Some(&Value::Bool(true));
        }
        child.step.state = input.facts.state.clone();
        let ended = matches!(
            child.step.state.as_deref(),
            Some("completed" | "failed" | "cancelled")
        );
        if ended && child.step.ended_at.is_none() {
            child.step.ended_at = Some(input.created_at.to_string());
        }
        self.publish_subagent(at, ops);
    }

    /// The index of a subagent, which the first event that names it opens
    /// as a step of the main lane.
    fn subagent_at(&mut self, input: &Input, id: &str, ops: &mut Ops) -> usize {
        let known = self
            .subagents
            .iter()
            .position(|child| child.step.subagent_id == id);
        // A known subagent's step opens at its first event alone: a later
        // event, in this turn or another, is the subagent's and opens none.
        if let Some(at) = known
            && (self.subagents[at].opened || self.subagents[at].step.id != input.id)
        {
            return at;
        }
        let index = self.turn_for(input, ops);
        let payload = input.full.map(|event| &event.payload);
        let step = SubagentStepDto {
            id: input.id.to_string(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: input.created_at.to_string(),
            subagent_id: id.to_string(),
            title: payload
                .and_then(|payload| string_at(payload, "/subagent/title"))
                .unwrap_or_else(|| id.to_string()),
            task: payload.and_then(|payload| string_at(payload, "/subagent/description")),
            state: None,
            can_cancel: false,
            started_at: input.created_at.to_string(),
            ended_at: None,
            activity: ActivityDto::default(),
        };
        let child = Subagent {
            step: step.clone(),
            latest: None,
            opened: true,
        };
        let at = match known {
            Some(at) => {
                self.subagents[at] = child;
                at
            }
            None => {
                self.subagents.push(child);
                self.subagents.len() - 1
            }
        };
        self.open_step(
            index,
            MAIN_LANE,
            input.id,
            input.row(),
            input.full.map(|_| StepDto::Subagent(step)),
            ops,
        );
        self.turns[index]
            .lanes
            .entry(MAIN_LANE.into())
            .or_default()
            .subagents
            .insert(id.to_string());
        at
    }

    /// Put the registry's copy of a subagent into its step, and say so.
    fn publish_subagent(&mut self, at: usize, ops: &mut Ops) {
        if !self.subagents[at].opened {
            return;
        }
        let child = self.subagents[at].step.clone();
        if let Some(turn) = self.turn_mut(&child.turn_id) {
            if let Some(step) = turn.steps.iter_mut().find(|step| {
                matches!(step, StepDto::Subagent(kept) if kept.subagent_id == child.subagent_id)
            }) {
                *step = StepDto::Subagent(child.clone());
                ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            }
            ops.touch(&child.turn_id);
        }
        ops.push(TrajectoryOpDto::SubagentUpsert { subagent: child });
    }

    /// A subagent's activity is counted over its own lane, in every turn the
    /// fold holds.
    fn recount_subagent(&mut self, lane: &str, ops: &mut Ops) {
        let Some(at) = self
            .subagents
            .iter()
            .position(|child| child.step.subagent_id == lane)
        else {
            return;
        };
        let mut activity = ActivityDto::default();
        for turn in self.turns.iter().chain(&self.outside) {
            if let Some(index) = turn.lanes.get(lane) {
                activity = add(activity, index.activity());
            }
        }
        if activity != self.subagents[at].step.activity {
            self.subagents[at].step.activity = activity;
            self.publish_subagent(at, ops);
        }
    }

    fn compaction(&mut self, input: &Input, ops: &mut Ops) {
        let compaction_id = input
            .facts
            .compaction
            .clone()
            .unwrap_or_else(|| input.id.to_string());
        let parts = input
            .full
            .map(|event| compaction_of(&event.payload, &self.session_id, input.id));
        let key = (input.lane.clone(), compaction_id.clone());
        if let Some(at) = self.compactions.get(&key).cloned()
            && at.step != input.id
        {
            // A later update changes the step the compaction's first event
            // opened, in the turn that holds it, and counts nothing new. Where
            // the fold does not hold that turn, it changes nothing here.
            let Some(turn) = at.turn.and_then(|turn| self.turn_mut(&turn)) else {
                return;
            };
            let lane = turn.lanes.entry(input.lane.clone()).or_default();
            // The newest change: a read loads it beside the opener, so the
            // step shows its status and summary.
            if let Some(row) = input.row() {
                lane.follows.insert(at.step.clone(), row.to_string());
            }
            ops.touch(&turn.id);
            // By id and lane: a subagent's first event also opens its
            // subagent step in the main lane, under the same id.
            if let Some(step) = turn.steps.iter_mut().find(|kept| {
                step_id(kept) == at.step.as_str() && lane_of(kept) == input.lane.as_str()
            }) && let StepDto::Compaction(kept) = step
            {
                if let Some((status, summary, error)) = parts {
                    kept.status = status;
                    if let Some(summary) = summary {
                        kept.summary = summary;
                    }
                    kept.error = error;
                }
                ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            }
            return;
        }
        let index = self.turn_for(input, ops);
        let turn_id = self.turns[index].id.clone();
        self.compactions.insert(
            key,
            CompactionAt {
                step: input.id.to_string(),
                turn: Some(turn_id.clone()),
            },
        );
        let step = input.full.map(|event| {
            compaction_step(event, &turn_id, &input.lane, &input.facts, &self.session_id)
        });
        self.open_step(index, &input.lane, input.id, input.row(), step, ops);
        self.turns[index]
            .lanes
            .entry(input.lane.clone())
            .or_default()
            .notices
            .push(input.id.to_string());
    }
}

impl Turn {
    fn new(id: &str, prompt: Option<PromptDto>, at: &str, state: TurnState) -> Self {
        Self {
            id: id.to_string(),
            prompt,
            state,
            stop_reason: None,
            started_at: at.to_string(),
            ended_at: (state != TurnState::Running).then(|| at.to_string()),
            steps: Vec::new(),
            lanes: HashMap::new(),
        }
    }

    fn holds_attention(&self) -> bool {
        self.steps.iter().any(waits_or_failed)
    }

    fn main(&self) -> Lane {
        self.lanes.get(MAIN_LANE).cloned().unwrap_or_default()
    }

    /// The turn as a client reads it: a running turn with its last steps,
    /// a closed one with none.
    fn dto(&self, waiting: usize) -> TurnDto {
        let main = self.main();
        let held: Vec<&StepDto> = self
            .steps
            .iter()
            .filter(|step| lane_of(step) == MAIN_LANE)
            .collect();
        let carried = match self.state {
            TurnState::Running => held.len().min(STEP_WINDOW),
            _ => 0,
        };
        let mut dto = self.header(&main, waiting);
        dto.steps = held[held.len() - carried..]
            .iter()
            .map(|step| (*step).clone())
            .collect();
        dto.earlier_steps = (main.openers.len() as u64).saturating_sub(carried as u64);
        dto
    }

    /// The turn with the last steps of one subagent's lane.
    fn dto_with(&self, lane: &str, waiting: usize) -> TurnDto {
        let mut dto = self.header(&self.main(), waiting);
        let held: Vec<&StepDto> = self
            .steps
            .iter()
            .filter(|step| lane_of(step) == lane)
            .collect();
        let carried = held.len().min(STEP_WINDOW);
        dto.steps = held[held.len() - carried..]
            .iter()
            .map(|step| (*step).clone())
            .collect();
        let openers = self.lanes.get(lane).map_or(0, |lane| lane.openers.len());
        dto.earlier_steps = (openers as u64).saturating_sub(carried as u64);
        dto
    }

    /// The turn without its steps. `waiting` is how many permissions and
    /// questions of the turn wait, held or not.
    fn header(&self, main: &Lane, waiting: usize) -> TurnDto {
        let held = self.steps.iter().filter(|step| lane_of(step) == MAIN_LANE);
        let waits: Vec<&StepDto> = held.clone().filter(|step| waits(step)).rev().collect();
        let errors: Vec<&StepDto> = held
            .clone()
            .filter(|step| matches!(step, StepDto::Error(_)))
            .rev()
            .collect();
        // The newest notices the index counts, as far as the fold holds
        // them: a cold read and a warm hub hold the same, and
        // `notices_more` counts the rest, exact from the index.
        let notices: Vec<StepDto> = main
            .notices
            .iter()
            .rev()
            .take(NOTICES)
            .filter_map(|id| held.clone().find(|step| step_id(step) == id.as_str()))
            .cloned()
            .collect();
        let shown_waits = waits.len().min(ATTENTION_WAITS);
        let shown_errors = errors.len().min(ATTENTION_ERRORS);
        let attention_more = waiting.max(waits.len()) - shown_waits
            + main.errors.len().max(errors.len())
            - shown_errors;
        TurnDto {
            id: self.id.clone(),
            lane: MAIN_LANE.into(),
            prompt: self.prompt.clone(),
            state: self.state,
            stop_reason: self.stop_reason.clone(),
            started_at: self.started_at.clone(),
            ended_at: self.ended_at.clone(),
            activity: main.activity(),
            steps: Vec::new(),
            earlier_steps: 0,
            attention: waits
                .into_iter()
                .take(shown_waits)
                .chain(errors.into_iter().take(shown_errors))
                .cloned()
                .collect(),
            attention_more: attention_more as u64,
            notices_more: (main.notices.len() - notices.len()) as u64,
            notices,
            answer: held
                .filter_map(|step| match step {
                    StepDto::Message(message) if Some(&message.id) == main.answer.as_ref() => {
                        Some(message.content.clone())
                    }
                    _ => None,
                })
                .next_back()
                .unwrap_or_default(),
        }
    }
}

impl Lane {
    /// What the lane did, counted.
    fn activity(&self) -> ActivityDto {
        let mut activity = ActivityDto {
            tool_calls: self.calls.len() as u64,
            thoughts: self.thoughts,
            subagents: self.subagents.len() as u64,
            ..ActivityDto::default()
        };
        activity.steps = activity.tool_calls + activity.subagents;
        let mut changed = BTreeSet::new();
        for call in self.calls.values() {
            match call.status {
                ToolCallStatus::Failed => activity.failed += 1,
                ToolCallStatus::Cancelled => activity.cancelled += 1,
                ToolCallStatus::Completed
                    if matches!(call.kind.as_deref(), Some("edit" | "delete" | "move")) =>
                {
                    changed.extend(call.paths.iter());
                }
                _ => {}
            }
        }
        activity.files_changed = changed.len() as u64;
        activity
    }
}

fn add(a: ActivityDto, b: ActivityDto) -> ActivityDto {
    ActivityDto {
        steps: a.steps + b.steps,
        tool_calls: a.tool_calls + b.tool_calls,
        files_changed: a.files_changed + b.files_changed,
        failed: a.failed + b.failed,
        cancelled: a.cancelled + b.cancelled,
        thoughts: a.thoughts + b.thoughts,
        subagents: a.subagents + b.subagents,
    }
}

fn waits_or_failed(step: &StepDto) -> bool {
    waits(step) || matches!(step, StepDto::Error(_))
}

/// Whether a step waits on a person: a pending permission or an open
/// question.
fn waits(step: &StepDto) -> bool {
    match step {
        StepDto::Permission(permission) => permission.state == PermissionState::Pending,
        StepDto::Question(question) => question.state == QuestionState::Open,
        _ => false,
    }
}

/// The text and the severity of a note event. The severity comes from the
/// kind and the facts, so a head counts a warning as its payload does: a
/// pin fallback is a warning, and so is a `warning` or an `error` notice,
/// the strongest a note says. Every other note is info.
fn note_of(kind: &str, facts: &EventFacts, full: Option<&AgentEventDto>) -> (String, NoteSeverity) {
    let payload = full.map(|event| &event.payload);
    let summary = || full.map(|event| event.summary.clone()).unwrap_or_default();
    match kind {
        "session_start" => ("session started".into(), NoteSeverity::Info),
        "session_end" => ("session ended".into(), NoteSeverity::Info),
        "session.pin_fallback" => (summary(), NoteSeverity::Warning),
        "notice" => {
            let severity = match facts.severity.as_deref() {
                Some("warning" | "error") => NoteSeverity::Warning,
                _ => NoteSeverity::Info,
            };
            let title = payload
                .and_then(|payload| string_at(payload, "/title"))
                .unwrap_or_default();
            let text = match payload.and_then(|payload| string_at(payload, "/description")) {
                Some(description) if !description.is_empty() => {
                    format!("{title} · {description}")
                }
                _ => title,
            };
            (text, severity)
        }
        _ => (summary(), NoteSeverity::Info),
    }
}

fn note_step(
    event: &AgentEventDto,
    turn_id: &str,
    lane: &str,
    text: String,
    severity: NoteSeverity,
) -> StepDto {
    StepDto::Note(NoteStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: lane.to_string(),
        created_at: event.created_at.clone(),
        origin: event.kind.clone(),
        severity,
        text,
    })
}

/// The status, the summary and the error of a compaction update. A stored
/// update holds the whole summary so far (021, rule 24): an empty one is a
/// summary the agent cleared, and replaces the last. None where the update
/// sends no summary.
fn compaction_of(
    payload: &Value,
    session_id: &str,
    event_id: &str,
) -> (
    CompactionStatus,
    Option<Vec<ContentBlockDto>>,
    Option<String>,
) {
    let status = match string_at(payload, "/status").as_deref() {
        Some("running" | "started" | "in_progress") => CompactionStatus::Running,
        Some("failed") => CompactionStatus::Failed,
        _ => CompactionStatus::Completed,
    };
    let summary = match payload.get("summary") {
        Some(Value::String(text)) => Some(vec![ContentBlockDto::Text { text: text.clone() }]),
        Some(Value::Array(blocks)) => Some(blocks_of(blocks, session_id, event_id)),
        Some(Value::Null) => Some(Vec::new()),
        _ => None,
    };
    (status, summary, string_at(payload, "/error"))
}

/// The step a compaction's first event opens, from its payload.
fn compaction_step(
    event: &AgentEventDto,
    turn_id: &str,
    lane: &str,
    facts: &EventFacts,
    session_id: &str,
) -> StepDto {
    let (status, summary, error) = compaction_of(&event.payload, session_id, &event.id);
    StepDto::Compaction(CompactionStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: lane.to_string(),
        created_at: event.created_at.clone(),
        compaction_id: facts.compaction.clone().unwrap_or_else(|| event.id.clone()),
        status,
        summary: summary.unwrap_or_default(),
        error,
    })
}

/// A permission step, from its request's payload.
fn permission_step(event: &AgentEventDto, turn_id: &str, facts: &EventFacts) -> StepDto {
    let payload = &event.payload;
    let acp = payload.get("acp").unwrap_or(payload);
    let title = string_at(payload, "/tool_name")
        .or_else(|| string_at(acp, "/title"))
        .unwrap_or_else(|| event.summary.clone());
    let kind = string_at(acp, "/kind");
    let tool_input = payload
        .get("tool_input")
        .or_else(|| acp.get("rawInput"))
        .unwrap_or(&Value::Null);
    let subagent = subagent_ref(payload);
    StepDto::Permission(PermissionStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: MAIN_LANE.into(),
        created_at: event.created_at.clone(),
        tool_call_id: facts.call.clone().unwrap_or_default(),
        tool: PermissionToolDto {
            subject: subject_of(kind.as_deref(), tool_input, &locations_of(acp)),
            title: title.clone(),
            kind,
        },
        question: match &subagent {
            Some(child) => format!("{title} · asked by subagent {}", child.title),
            None => title,
        },
        options: permission_options(payload),
        state: PermissionState::Pending,
        answer: None,
        ai: ariadne_api::permissions::ai_permission_note(payload),
        learned: None,
        subagent,
    })
}

/// An error step, from its payload.
fn error_step(event: &AgentEventDto, turn_id: &str) -> StepDto {
    let payload = &event.payload;
    StepDto::Error(ErrorStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: MAIN_LANE.into(),
        created_at: event.created_at.clone(),
        text: string_at(payload, "/error/data/message")
            .or_else(|| string_at(payload, "/error/message"))
            .or_else(|| string_at(payload, "/message"))
            .unwrap_or_else(|| event.summary.clone()),
        diagnosis: None,
    })
}

/// A question step: a native `question` (021, rule 21), or a legacy
/// `agent_request` with a question.
fn question_step(event: &AgentEventDto, turn_id: &str, key: &str) -> QuestionStepDto {
    let payload = &event.payload;
    let mut step = QuestionStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: MAIN_LANE.into(),
        created_at: event.created_at.clone(),
        request_id: key.to_string(),
        hint: String::new(),
        question: String::new(),
        options: Vec::new(),
        recommended: None,
        recommendation_reason: None,
        state: QuestionState::Open,
        answer: None,
    };
    let question = payload.get("question").unwrap_or(&Value::Null);
    if event.kind == "question" {
        step.question = string_at(question, "/message").unwrap_or_default();
        // The first field that offers choices is the one the step lists.
        let choices = question
            .pointer("/schema/properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .find_map(|(_, field)| {
                let options = field
                    .get("oneOf")
                    .or_else(|| field.pointer("/items/anyOf"))?
                    .as_array()?;
                Some((field, options))
            });
        if let Some((field, options)) = choices {
            step.hint = string_at(field, "/title").unwrap_or_default();
            step.options = options
                .iter()
                .filter_map(|option| {
                    let id = string_at(option, "/const")?;
                    Some(QuestionOptionDto {
                        label: string_at(option, "/title").unwrap_or_else(|| id.clone()),
                        description: string_at(option, "/description"),
                        id,
                    })
                })
                .collect();
        }
        return step;
    }
    step.question = string_at(question, "/text").unwrap_or_default();
    step.hint = string_at(payload, "/hint").unwrap_or_default();
    step.options = question
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(QuestionOptionDto {
                id: string_at(option, "/id")?,
                label: string_at(option, "/label")?,
                description: string_at(option, "/description"),
            })
        })
        .collect();
    step.recommended = string_at(question, "/recommended_option_id");
    step.recommendation_reason = string_at(question, "/recommendation_reason");
    step
}

/// The answer a resolution carries, in words: the legacy answer text, or
/// each field of an accepted form as `field: value`.
fn answer_of(payload: &Value) -> Option<String> {
    if let Some(text) = string_at(payload, "/answer/text") {
        return Some(text);
    }
    let content = payload.get("content")?.as_object()?;
    let line = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_string)
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    };
    Some(
        content
            .iter()
            .map(|(field, value)| format!("{field}: {}", line(value)))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The ops one event makes, and the turns whose header it changed.
#[derive(Default)]
struct Ops {
    ops: Vec<TrajectoryOpDto>,
    turns: Vec<String>,
}

impl Ops {
    fn push(&mut self, op: TrajectoryOpDto) {
        self.ops.push(op);
    }

    fn touch(&mut self, turn_id: &str) {
        if !self.turns.iter().any(|id| id == turn_id) {
            self.turns.push(turn_id.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use ariadne_api::events::AgentEventDto;
    use ariadne_api::trajectory::{
        ActivityDto, CompactionStatus, ContentBlockDto, MAIN_LANE, NoteSeverity, PermissionState,
        PlanVariant, QuestionState, SessionStateDto, StepDto, TrajectoryOpDto, TurnState,
    };
    use ariadne_api::usage::TokenUsageDto;
    use ariadne_core::SessionStatus;
    use serde_json::{Value, json};

    use super::{Fold, STEP_WINDOW};
    use crate::trajectory::step_id;

    /// A fold that hands out its events' ids in order, as the store does.
    struct Session {
        fold: Fold,
        next: u32,
    }

    impl Session {
        fn new() -> Self {
            let state = SessionStateDto {
                status: SessionStatus::Running,
                attention_reason: None,
                running_turn_id: None,
                title: None,
                mode: None,
                config_options: Vec::new(),
                context: None,
                usage: TokenUsageDto::default(),
                cost: None,
                commands: Vec::new(),
            };
            Self {
                fold: Fold::new("session", state),
                next: 0,
            }
        }

        /// Fold one event, and answer its id and its ops.
        fn event(&mut self, kind: &str, payload: Value) -> (String, Vec<TrajectoryOpDto>) {
            let stored = !kind.ends_with("_chunk") && kind != "display_state_update";
            self.fold_event(kind, payload, stored)
        }

        fn fold_event(
            &mut self,
            kind: &str,
            payload: Value,
            stored: bool,
        ) -> (String, Vec<TrajectoryOpDto>) {
            self.next += 1;
            let id = format!("{:04}", self.next);
            let event = AgentEventDto {
                id: id.clone(),
                session_id: Some("session".into()),
                task_id: None,
                kind: kind.into(),
                payload,
                summary: kind.into(),
                created_at: format!("2026-11-26T00:00:{:02}Z", self.next % 60),
            };
            let ops = self.fold.apply(&event, stored);
            (id, ops)
        }

        fn prompt(&mut self, text: &str) -> String {
            self.event(
                "user_prompt_submit",
                json!({"text": text, "source": "console"}),
            )
            .0
        }

        fn call(&mut self, call_id: &str, kind: &str, status: &str, path: &str) {
            self.event(
                "pre_tool_use",
                json!({"tool_name": "Edit", "tool_input": {"file_path": path},
                       "acp": {"toolCallId": call_id, "kind": kind, "status": "pending"}}),
            );
            self.event(
                "post_tool_use",
                json!({"tool_name": "Edit",
                       "acp": {"toolCallId": call_id, "kind": kind, "status": status,
                               "locations": [{"path": path}]}}),
            );
        }

        fn stop(&mut self) {
            self.event("stop", json!({"stop_reason": "end_turn"}));
        }
    }

    fn text(value: &str) -> Vec<ContentBlockDto> {
        vec![ContentBlockDto::Text { text: value.into() }]
    }

    /// The first chunk of a run opens a streaming step, every later one is
    /// an append onto it, and the stored whole closes the same step.
    #[test]
    fn streamed_text_arrives_as_appends() {
        let mut s = Session::new();
        let turn = s.prompt("Go");

        let (_, first) = s.event("agent_message_chunk", json!({"text": "Half "}));
        let (_, second) = s.event("agent_message_chunk", json!({"text": "done."}));
        let (stored, whole) = s.event("agent_message", json!({"text": "Half done."}));

        let opened = first
            .iter()
            .find_map(|op| match op {
                TrajectoryOpDto::ItemUpsert {
                    step: StepDto::Message(m),
                } if m.streaming && m.content == text("Half ") => Some(m.id.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the first chunk opens a streaming step: {first:?}"));
        assert_eq!(
            second,
            [TrajectoryOpDto::TextAppend {
                turn_id: turn.clone(),
                item_id: opened.clone(),
                text: "done.".into(),
            }],
            "a later chunk is an append alone"
        );
        assert!(
            whole.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step: StepDto::Message(m) }
                    if m.id == stored && !m.streaming && m.content == text("Half done."))),
            "the stored whole closes the run under its own id: {whole:?}"
        );
    }

    /// With `item_id`, the chunks and the whole are one item under that id,
    /// whatever ids their events have.
    #[test]
    fn an_item_id_names_the_message_its_chunks_and_whole_belong_to() {
        let mut s = Session::new();
        s.prompt("Go");
        s.event(
            "agent_message_chunk",
            json!({"text": "Hi", "item_id": "item-1"}),
        );
        s.event(
            "agent_message",
            json!({"text": "Hi there", "item_id": "item-1",
                                        "message_id": "m-1"}),
        );

        let turn = &s.fold.snapshot(0, 20, None).turns[0];
        let [StepDto::Message(message)] = turn.steps.as_slice() else {
            panic!("one message: {:?}", turn.steps);
        };
        assert_eq!(message.id, "item-1");
        assert_eq!(message.content, text("Hi there"));
        assert_eq!(message.message_id.as_deref(), Some("m-1"));
    }

    /// A closed turn carries its prompt, what it did as counts, and its
    /// answer, and none of its steps: they are a page of the steps route
    /// away.
    #[test]
    fn a_closed_turn_carries_its_prompt_activity_and_answer_and_no_steps() {
        let mut s = Session::new();
        let prompt = s.prompt("Fix the build");
        s.event("agent_thought", json!({"text": "Think"}));
        s.call("call-1", "edit", "completed", "src/a.rs");
        s.call("call-2", "edit", "completed", "src/a.rs");
        s.call("call-3", "execute", "failed", "Makefile");
        s.event("agent_message", json!({"text": "Fixed."}));
        s.stop();

        let turn = &s.fold.snapshot(0, 20, None).turns[0];

        assert_eq!(turn.id, prompt);
        assert_eq!(turn.state, TurnState::Completed);
        assert_eq!(turn.prompt.as_ref().unwrap().content, text("Fix the build"));
        assert_eq!(turn.answer, text("Fixed."));
        assert!(turn.steps.is_empty(), "a closed turn carries no steps");
        assert_eq!(
            turn.earlier_steps, 5,
            "a thought, three calls and a message"
        );
        assert_eq!(
            turn.activity,
            ActivityDto {
                steps: 3,
                tool_calls: 3,
                files_changed: 1,
                failed: 1,
                cancelled: 0,
                thoughts: 1,
                subagents: 0,
            }
        );
    }

    /// A prompt keeps the text as it was submitted beside the content the
    /// agent read, which leads with the seat brief on the first turn of a
    /// launch: a console matches what it sent by that text.
    #[test]
    fn a_prompt_keeps_its_submitted_text_beside_the_seat_brief() {
        let mut s = Session::new();
        s.event(
            "user_prompt_submit",
            json!({"text": "Start", "source": "console",
                   "content": [{"type": "text", "text": "Brief\n\nStart"}]}),
        );

        let turn = &s.fold.snapshot(0, 20, None).turns[0];
        let prompt = turn.prompt.as_ref().unwrap();

        assert_eq!(prompt.text.as_deref(), Some("Start"));
        assert_eq!(prompt.content, text("Brief\n\nStart"));
    }

    /// A running turn carries its last steps, and counts the ones before.
    #[test]
    fn a_running_turn_carries_its_last_steps() {
        let mut s = Session::new();
        s.prompt("Read everything");
        for n in 0..STEP_WINDOW + 10 {
            s.call(&format!("call-{n}"), "read", "completed", "a.rs");
        }

        let turn = &s.fold.snapshot(0, 20, None).turns[0];

        assert_eq!(turn.state, TurnState::Running);
        assert_eq!(turn.steps.len(), STEP_WINDOW);
        assert_eq!(turn.earlier_steps, 10);
        let StepDto::ToolCall(last) = turn.steps.last().unwrap() else {
            panic!("{:?}", turn.steps.last());
        };
        assert_eq!(last.tool_call_id, format!("call-{}", STEP_WINDOW + 9));
    }

    /// A pending permission, an open question and an error stay in the
    /// attention of their turn after it closed; an answer takes the
    /// question out.
    #[test]
    fn attention_keeps_what_waits_and_the_errors_of_closed_turns() {
        let mut s = Session::new();
        s.prompt("Ask me");
        let (question, _) = s.event(
            "question",
            json!({"question": {"id": "q-1", "message": "Which?",
                                "schema": {"type": "object", "properties": {
                                    "pick": {"type": "string", "title": "Pick one",
                                             "oneOf": [{"const": "a", "title": "A"}]}}}}}),
        );
        s.stop();
        s.prompt("Fail");
        let (error, _) = s.event("session.error", json!({"error": {"message": "boom"}}));
        s.prompt("Write");
        let (permission, _) = s.event(
            "permission_request",
            json!({"tool_name": "Write", "acp": {"toolCallId": "call-9", "kind": "edit"},
                   "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"}]}),
        );

        let snapshot = s.fold.snapshot(0, 20, None);
        let ids = |at: usize| -> Vec<String> {
            snapshot.turns[at]
                .attention
                .iter()
                .map(|step| match step {
                    StepDto::Question(q) => q.id.clone(),
                    StepDto::Error(e) => e.id.clone(),
                    StepDto::Permission(p) => p.id.clone(),
                    other => panic!("not attention: {other:?}"),
                })
                .collect()
        };
        assert_eq!(snapshot.turns[0].state, TurnState::Completed);
        assert_eq!(
            ids(0),
            std::slice::from_ref(&question),
            "the open question of a closed turn"
        );
        assert_eq!(snapshot.turns[1].state, TurnState::Failed);
        assert_eq!(ids(1), [error], "the error of a failed turn");
        assert_eq!(
            ids(2),
            [permission],
            "the pending permission of the running turn"
        );

        s.event(
            "question.resolved",
            json!({"question_id": "q-1", "action": "accept", "content": {"pick": "a"},
                   "closed_by": "user"}),
        );
        s.event(
            "permission.replied",
            json!({"option_id": "yes", "decided_by": "console"}),
        );
        let snapshot = s.fold.snapshot(0, 20, None);
        assert!(
            snapshot.turns[0].attention.is_empty(),
            "an answered question waits no more"
        );
        assert!(
            snapshot.turns[2].attention.is_empty(),
            "an answered permission waits no more"
        );
        let held = s.fold.held_steps(&snapshot.turns[2].id, MAIN_LANE);
        let StepDto::Permission(answered) = &held[0] else {
            panic!("the permission step");
        };
        assert_eq!(answered.state, PermissionState::Answered);
        assert_eq!(answered.answer.as_ref().unwrap().label, "Allow");
        let held = s.fold.held_steps(&snapshot.turns[0].id, MAIN_LANE);
        let Some(StepDto::Question(answered)) = held.iter().find(|step| step_id(step) == question)
        else {
            panic!("the question step");
        };
        assert_eq!(answered.state, QuestionState::Answered);
        assert_eq!(answered.answer.as_deref(), Some("pick: a"));
        assert_eq!(answered.options[0].label, "A");
    }

    /// A subagent is one step in its parent's turn, counting what it did,
    /// and its own steps are in its lane.
    #[test]
    fn a_subagent_is_a_step_in_its_parents_turn_and_its_steps_are_its_lane() {
        let mut s = Session::new();
        let turn = s.prompt("Explore");
        let child = json!({"id": "child-1", "title": "explorer", "description": "Look"});
        s.event(
            "subagent_update",
            json!({"subagent": child, "state": "running", "capabilities": {"cancel": true}}),
        );
        let (call, _) = s.event(
            "pre_tool_use",
            json!({"tool_name": "Read", "subagent": {"id": "child-1", "title": "explorer"},
                   "acp": {"toolCallId": "c-1", "kind": "read", "status": "pending"}}),
        );
        s.event(
            "post_tool_use",
            json!({"tool_name": "Read", "subagent": {"id": "child-1", "title": "explorer"},
                   "acp": {"toolCallId": "c-1", "kind": "read", "status": "completed"}}),
        );

        let snapshot = s.fold.snapshot(0, 20, None);
        let [StepDto::Subagent(step)] = snapshot.turns[0].steps.as_slice() else {
            panic!(
                "the parent's turn holds the subagent alone: {:?}",
                snapshot.turns[0].steps
            );
        };
        assert_eq!(step.subagent_id, "child-1");
        assert_eq!(step.turn_id, turn);
        assert!(step.can_cancel);
        assert_eq!(step.activity.tool_calls, 1);
        assert_eq!(snapshot.subagents, std::slice::from_ref(step));

        let view = s.fold.snapshot(0, 20, Some("child-1"));
        let [StepDto::ToolCall(read)] = view.turns[0].steps.as_slice() else {
            panic!("the subagent's lane: {:?}", view.turns[0].steps);
        };
        assert_eq!(read.id, call);
        assert_eq!(read.lane, "child-1");
    }

    /// An event of a kind the fold does not know, and a payload without the
    /// fields a later daemon stores, fold without a change or a panic.
    #[test]
    fn events_without_the_new_fields_and_unknown_kinds_fold() {
        let mut s = Session::new();
        s.prompt("Go");
        let (_, unknown) = s.event("some_future_kind", json!({"anything": 1}));
        let (_, bare) = s.event("agent_message", json!({}));

        assert!(unknown.is_empty());
        assert!(!bare.is_empty(), "a message with no text is still a step");
    }

    /// A call's step previews its output and counts the lines of its diffs
    /// (rule 13).
    #[test]
    fn a_call_previews_its_output_and_counts_its_diff() {
        let mut s = Session::new();
        s.prompt("Edit");
        let output: String = (1..=8).map(|n| format!("line {n}\n")).collect();
        s.event(
            "pre_tool_use",
            json!({"tool_name": "Edit", "tool_input": {"file_path": "a.rs"},
                   "acp": {"toolCallId": "c-1", "kind": "edit", "status": "pending"}}),
        );
        s.event(
            "post_tool_use",
            json!({"tool_name": "Edit",
                   "acp": {"toolCallId": "c-1", "kind": "edit", "status": "completed",
                           "content": [
                               {"type": "content", "content": {"type": "text", "text": output}},
                               {"type": "diff", "path": "a.rs",
                                "patch": {"text": "--- a.rs\n+++ a.rs\n@@ -1,2 +1,3 @@\n-one\n+uno\n+dos\n two\n"}},
                               {"type": "diff", "path": "b.rs",
                                "oldText": "a\nb\nc\n", "newText": "a\nB\nc\n"}]}}),
        );

        let StepDto::ToolCall(call) = &s.fold.snapshot(0, 20, None).turns[0].steps[0] else {
            panic!("the call");
        };
        assert_eq!(
            call.preview.as_deref(),
            Some("line 1\nline 2\nline 3\nline 4\nline 5"),
            "five lines of the output"
        );
        let stat = call.diff_stat.unwrap();
        assert_eq!((stat.files, stat.added, stat.removed), (2, 3, 2));
        assert_eq!(call.subject.as_deref(), Some("a.rs"));
        assert_eq!(call.detail_rev, 1, "one update after the opener");
    }

    /// The lifecycle events are notes, and a pin fallback is a warning
    /// (rule 17); a start in a running turn interrupts it (rule 3).
    #[test]
    fn lifecycle_events_are_notes_and_a_restart_interrupts_the_turn() {
        use ariadne_api::trajectory::NoteSeverity;
        let mut s = Session::new();
        s.event("session_start", json!({}));
        let turn = s.prompt("Go");
        s.event("session.pin_fallback", json!({}));
        s.event("session_start", json!({}));

        let snapshot = s.fold.snapshot(0, 20, None);
        let shown = snapshot.turns.iter().find(|t| t.id == turn).unwrap();
        assert_eq!(shown.state, TurnState::Interrupted);
        let notes: Vec<(String, NoteSeverity)> = s
            .fold
            .held_steps(&turn, MAIN_LANE)
            .into_iter()
            .map(|step| match step {
                StepDto::Note(note) => (note.origin, note.severity),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            notes,
            [
                ("session.pin_fallback".to_string(), NoteSeverity::Warning),
                ("session_start".to_string(), NoteSeverity::Info),
            ]
        );
        assert_eq!(
            snapshot.turns[0].prompt, None,
            "the start before a prompt is a turn of its own"
        );
    }

    /// A compaction is one step by its id, which a later update changes
    /// (rule 18).
    #[test]
    fn a_compaction_is_one_step_that_its_updates_change() {
        use ariadne_api::trajectory::CompactionStatus;
        let mut s = Session::new();
        let turn = s.prompt("Go");
        s.event(
            "compaction_update",
            json!({"compaction_id": "k-1", "status": "running"}),
        );
        s.event(
            "compaction_update",
            json!({"compaction_id": "k-1", "status": "completed", "summary": "Kept the plan."}),
        );

        let held = s.fold.held_steps(&turn, MAIN_LANE);
        let [StepDto::Compaction(step)] = held.as_slice() else {
            panic!("one compaction step: {held:?}");
        };
        assert_eq!(step.status, CompactionStatus::Completed);
        assert_eq!(step.summary, text("Kept the plan."));
    }

    /// A turn carries its warning notes and its compactions in `notices`,
    /// newest first, and no info note; a compaction there shows its newest
    /// update, and the change comes as an update of the turn. `attention`
    /// holds none of them (rules 26, 27).
    #[test]
    fn a_turn_carries_its_warnings_and_compactions_in_its_notices() {
        use ariadne_api::trajectory::CompactionStatus;
        let mut s = Session::new();
        let turn = s.prompt("Go");
        let (fallback, _) = s.event("session.pin_fallback", json!({}));
        let (warning, _) = s.event(
            "notice",
            json!({"severity": "warning", "title": "Slow", "description": "The API is slow."}),
        );
        s.event("notice", json!({"severity": "info", "title": "Hello"}));
        s.event("session.switched", json!({}));
        let (compaction, _) = s.event(
            "compaction_update",
            json!({"compaction_id": "k-1", "status": "running"}),
        );
        let (error, _) = s.event("notice", json!({"severity": "error", "title": "Broke"}));

        let notices = |turn: &ariadne_api::trajectory::TurnDto| -> Vec<String> {
            turn.notices
                .iter()
                .map(|step| step_id(step).to_string())
                .collect()
        };
        let running = s.fold.snapshot(0, 20, None).turns[0].clone();
        assert_eq!(
            notices(&running),
            [
                error.clone(),
                compaction.clone(),
                warning.clone(),
                fallback.clone()
            ]
        );
        assert_eq!(running.notices_more, 0);
        assert!(running.attention.is_empty(), "attention does not change");

        let (_, ops) = s.event(
            "compaction_update",
            json!({"compaction_id": "k-1", "status": "completed", "summary": "Kept the plan."}),
        );
        let update = ops
            .iter()
            .find_map(|op| match op {
                TrajectoryOpDto::TurnUpsert { turn: upsert } if upsert.id == turn => Some(upsert),
                _ => None,
            })
            .expect("the change updates the turn");
        let Some(StepDto::Compaction(step)) = update
            .notices
            .iter()
            .find(|step| step_id(step) == compaction)
        else {
            panic!("the compaction is a notice: {:?}", update.notices);
        };
        assert_eq!(step.status, CompactionStatus::Completed);
        assert_eq!(step.summary, text("Kept the plan."));

        s.stop();
        let closed = s.fold.snapshot(0, 20, None).turns[0].clone();
        assert!(closed.steps.is_empty());
        assert_eq!(
            notices(&closed),
            [
                error.clone(),
                compaction.clone(),
                warning.clone(),
                fallback.clone()
            ]
        );
        assert_eq!(closed.notices_more, 0);
    }

    /// A turn that holds attention stays outside the snapshot when the fold
    /// forgets it, and a later answer changes the step in its own turn.
    #[test]
    fn a_pruned_turn_keeps_its_open_question_for_a_later_answer() {
        let mut s = Session::new();
        let old = s.prompt("Ask");
        let (question, _) = s.event(
            "question",
            json!({"question": {"id": "q-1", "message": "Which?", "schema": {}}}),
        );
        s.stop();
        for _ in 0..3 {
            s.prompt("More");
            s.stop();
        }
        s.fold.prune(2);
        assert!(
            s.fold
                .snapshot(0, 20, None)
                .turns
                .iter()
                .all(|t| t.id != old)
        );

        let (_, ops) = s.event(
            "question.resolved",
            json!({"question_id": "q-1", "action": "decline", "closed_by": "user"}),
        );

        assert!(
            ops.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step: StepDto::Question(q) }
                    if q.id == question && q.turn_id == old && q.state == QuestionState::Declined)),
            "{ops:?}"
        );
        assert!(
            ops.iter().any(|op| matches!(op,
                TrajectoryOpDto::TurnUpsert { turn } if turn.id == old && turn.attention.is_empty())),
            "the old turn waits no more: {ops:?}"
        );
    }

    /// The index counts what a turn did from heads as it does from whole
    /// events: a fold of heads and a fold of whole events agree.
    #[test]
    fn heads_and_whole_events_count_a_turn_the_same() {
        let mut whole = Session::new();
        let mut events = Vec::new();
        for (kind, payload) in [
            ("user_prompt_submit", json!({"text": "Go"})),
            ("agent_thought", json!({"text": "Think"})),
            (
                "pre_tool_use",
                json!({"acp": {"toolCallId": "c-1", "kind": "edit", "status": "pending"}}),
            ),
            (
                "post_tool_use",
                json!({"acp": {"toolCallId": "c-1", "kind": "edit", "status": "completed",
                               "locations": [{"path": "a.rs"}]}}),
            ),
            (
                "pre_tool_use",
                json!({"acp": {"toolCallId": "c-2", "kind": "execute", "status": "pending"}}),
            ),
            (
                "post_tool_use",
                json!({"acp": {"toolCallId": "c-2", "status": "failed"}}),
            ),
            ("agent_message", json!({"text": "Done"})),
            ("session_end", json!({})),
        ] {
            let (id, _) = whole.event(kind, payload.clone());
            events.push((id, kind.to_string(), payload));
        }

        let mut heads = Fold::new("session", whole.fold.snapshot(0, 1, None).session);
        for (id, kind, payload) in &events {
            heads.apply_head(&ariadne_store::EventHead {
                id: id.clone(),
                kind: kind.clone(),
                lane: ariadne_core::event_facts::lane_of(kind, payload),
                facts: serde_json::to_string(&ariadne_core::event_facts::facts_of(kind, payload))
                    .unwrap(),
                created_at: "2026-11-26T00:00:00Z".into(),
            });
        }

        let counted = |fold: &Fold| {
            let turn = fold.snapshot(0, 20, None).turns[0].clone();
            (turn.activity, turn.earlier_steps, turn.state)
        };
        assert_eq!(counted(&heads), counted(&whole.fold));
        let (activity, earlier, _) = counted(&heads);
        assert_eq!(
            (activity.tool_calls, activity.failed, activity.files_changed),
            (2, 1, 1)
        );
        assert_eq!(earlier, 5, "a thought, two calls, a message and a note");
    }

    /// A diagnosis reads onto the error it names (rule 21).
    #[test]
    fn a_diagnosis_reads_onto_the_error_it_names() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        let (error, _) = s.event("session.error", json!({"message": "quota"}));
        s.event(
            "session.diagnosis",
            json!({"error_event_id": error, "category": "quota_exhausted", "confidence": 0.8}),
        );

        let held = s.fold.held_steps(&turn, MAIN_LANE);
        let [StepDto::Error(step)] = held.as_slice() else {
            panic!("one error: {held:?}");
        };
        assert_eq!(step.text, "quota");
        assert!(step.diagnosis.is_some(), "{step:?}");
    }

    /// A block with bytes carries a blob URL and never the bytes (rule 24).
    #[test]
    fn a_block_with_bytes_carries_a_blob_url_and_not_the_bytes() {
        let mut s = Session::new();
        let (prompt, _) = s.event(
            "user_prompt_submit",
            json!({"content": [
                {"type": "text", "text": "Look"},
                {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgo="}]}),
        );

        let snapshot = s.fold.snapshot(0, 20, None);
        let content = &snapshot.turns[0].prompt.as_ref().unwrap().content;
        assert_eq!(
            content[1],
            ContentBlockDto::Image {
                mime_type: Some("image/png".into()),
                blob: Some(format!("/v1/sessions/session/trajectory/blobs/{prompt}/1")),
                uri: None,
            }
        );
        assert!(!serde_json::to_string(content).unwrap().contains("iVBOR"));
    }

    /// A plan keeps the id of its first event across its updates (rule 28).
    #[test]
    fn a_plan_keeps_the_id_of_its_first_event() {
        let mut s = Session::new();
        s.prompt("Go");
        let (first, _) = s.event(
            "plan",
            json!({"entries": [{"content": "a", "status": "pending"}]}),
        );
        s.event(
            "plan",
            json!({"entries": [{"content": "a", "status": "completed", "priority": "high"}]}),
        );

        let plans = s.fold.snapshot(0, 20, None).plans;
        let [plan] = plans.as_slice() else {
            panic!("one plan: {plans:?}");
        };
        assert_eq!(plan.plan_id, first);
        assert_eq!(plan.entries[0].status, "completed");
        assert_eq!(plan.entries[0].priority.as_deref(), Some("high"));

        // A plan of the new kinds is its lane's and its `plan_id`'s.
        let (items, _) = s.event(
            "plan_update",
            json!({"plan": {"type": "items", "plan_id": "p-1",
                            "entries": [{"content": "b", "status": "pending"}]}}),
        );
        let (markdown, _) = s.event(
            "plan_update",
            json!({"plan": {"type": "markdown", "plan_id": "p-2", "content": "# P",
                            "path": "/w/P.md"}}),
        );
        s.event(
            "plan_update",
            json!({"plan": {"type": "items", "plan_id": "p-1",
                            "entries": [{"content": "b", "status": "completed"}]}}),
        );
        let (_, removed) = s.event("plan_removed", json!({"plan_id": "p-1"}));
        assert!(
            removed
                .iter()
                .any(|op| matches!(op, TrajectoryOpDto::PlanUpsert { plan }
                if plan.plan_id == items && plan.removed)),
            "{removed:?}"
        );
        let (_, nothing) = s.event("plan_removed", json!({"plan_id": "p-9"}));
        assert!(nothing.is_empty(), "no such plan: {nothing:?}");

        let plans = s.fold.snapshot(0, 20, None).plans;
        let ids: Vec<&str> = plans.iter().map(|plan| plan.plan_id.as_str()).collect();
        assert_eq!(ids, [first.as_str(), items.as_str(), markdown.as_str()]);
        assert_eq!(plans[1].entries[0].status, "completed");
        assert!(plans[1].removed);
        assert_eq!(plans[2].variant, PlanVariant::Markdown);
        assert_eq!(plans[2].markdown.as_deref(), Some("# P"));
        assert_eq!(plans[2].path.as_deref(), Some("/w/P.md"));
        assert!(!plans[2].removed);
    }

    /// A launch that reported no commands has none, whatever an older daemon
    /// stored; a session whose launch reported nothing reads the stored list
    /// (rule 33).
    #[test]
    fn a_reported_empty_command_list_stands_against_a_stored_one() {
        let stored_list = json!({"available_commands": [{"name": "old", "description": "Old"}]});
        let mut reported = Session::new();
        reported.fold.set_display_reported(true);
        reported.prompt("Go");
        reported.event("available_commands_update", stored_list.clone());
        assert!(reported.fold.session.commands.is_empty());

        let mut unreported = Session::new();
        unreported.prompt("Go");
        unreported.event("available_commands_update", stored_list);
        assert_eq!(unreported.fold.session.commands[0].name, "old");

        // A live report of no commands stands too.
        let mut live = Session::new();
        live.prompt("Go");
        live.event("display_state_update", json!({"commands": []}));
        live.event(
            "available_commands_update",
            json!({"available_commands": [{"name": "old", "description": "Old"}]}),
        );
        assert!(live.fold.session.commands.is_empty());
    }

    /// A compaction's update replaces its summary, an empty one included,
    /// and changes the step of its own lane: a compaction of the same ACP id
    /// in another lane keeps its step (rule 27).
    #[test]
    fn a_compaction_takes_each_whole_summary_and_keeps_to_its_lane() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        let (main, _) = s.event(
            "compaction_update",
            json!({"compaction_id": "c-1", "status": "in_progress",
                   "summary": [{"type": "text", "text": "Draft"}]}),
        );
        let (child, _) = s.event(
            "compaction_update",
            json!({"compaction_id": "c-1", "status": "in_progress",
                   "summary": [{"type": "text", "text": "Child"}],
                   "subagent": {"id": "child-1"}}),
        );
        s.event(
            "compaction_update",
            json!({"compaction_id": "c-1", "status": "completed", "summary": [],
                   "subagent": {"id": "child-1"}}),
        );

        let steps = s.fold.held_steps(&turn, "child-1");
        let [StepDto::Compaction(theirs)] = steps.as_slice() else {
            panic!("the child's compaction alone: {steps:?}");
        };
        assert_eq!(theirs.id, child);
        assert_eq!(theirs.status, CompactionStatus::Completed);
        assert!(theirs.summary.is_empty(), "the cleared summary");

        let steps = s.fold.held_steps(&turn, MAIN_LANE);
        let ours: Vec<_> = steps
            .iter()
            .filter_map(|step| match step {
                StepDto::Compaction(kept) => Some(kept),
                _ => None,
            })
            .collect();
        let [ours] = ours.as_slice() else {
            panic!("the main lane's compaction alone: {ours:?}");
        };
        assert_eq!(ours.id, main);
        assert_eq!(
            ours.status,
            CompactionStatus::Running,
            "the main step stays"
        );
        assert_eq!(ours.summary, text("Draft"));
    }

    /// A compaction that a later turn finishes changes the step of the turn
    /// it began in, and the later turn counts no step for it (rules 17, 27).
    #[test]
    fn a_compaction_across_a_prompt_changes_its_first_step() {
        let mut s = Session::new();
        let first = s.prompt("Go");
        let (opened, _) = s.event(
            "compaction_update",
            json!({"compaction_id": "c-2", "status": "in_progress"}),
        );
        s.stop();
        let second = s.prompt("Again");
        s.event(
            "compaction_update",
            json!({"compaction_id": "c-2", "status": "completed",
                   "summary": [{"type": "text", "text": "Kept."}]}),
        );
        s.stop();

        let steps = s.fold.held_steps(&first, MAIN_LANE);
        let [StepDto::Compaction(kept)] = steps.as_slice() else {
            panic!("the first turn's compaction: {steps:?}");
        };
        assert_eq!(kept.id, opened);
        assert_eq!(kept.status, CompactionStatus::Completed);
        assert_eq!(kept.summary, text("Kept."));
        assert!(s.fold.held_steps(&second, MAIN_LANE).is_empty());
        let snapshot = s.fold.snapshot(0, 20, None);
        let counts: Vec<u64> = snapshot.turns.iter().map(|t| t.earlier_steps).collect();
        assert_eq!(counts, [1, 0], "one step, in the turn it began in");
    }

    /// A removal that comes before a plan's first update names the plan, as
    /// a read of the store names it (rule 32).
    #[test]
    fn a_removal_before_a_plans_first_update_names_the_plan() {
        let mut s = Session::new();
        s.prompt("Go");
        let (removal, nothing) = s.event("plan_removed", json!({"plan_id": "p-1"}));
        assert!(nothing.is_empty(), "no plan to remove: {nothing:?}");
        s.event(
            "plan_update",
            json!({"plan": {"type": "items", "plan_id": "p-1", "entries": []}}),
        );
        let plans = s.fold.snapshot(0, 20, None).plans;
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].plan_id, removal);
        assert!(!plans[0].removed);
    }

    /// A subagent's notice is a note of its lane, and the main lane does not
    /// count it (rules 26, 31).
    #[test]
    fn a_subagents_notice_is_a_note_of_its_lane() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        let (notice, _) = s.event(
            "notice",
            json!({"severity": "warning", "title": "Slow", "subagent": {"id": "child-1"}}),
        );
        s.stop();

        let snapshot = s.fold.snapshot(0, 20, None);
        let header = snapshot.turns.iter().find(|t| t.id == turn).unwrap();
        // The main lane opened the subagent's step alone.
        assert_eq!(header.earlier_steps, 1);
        let steps = s.fold.held_steps(&turn, "child-1");
        let [StepDto::Note(note)] = steps.as_slice() else {
            panic!("the child's note: {steps:?}");
        };
        assert_eq!(note.id, notice);
        assert_eq!(note.lane, "child-1");
        assert_eq!(note.severity, NoteSeverity::Warning);
    }

    /// The display state the session starts from gives its commands, which a
    /// stored list of an older daemon does not replace; a live display
    /// change is a `session_patch` (rule 33).
    #[test]
    fn a_display_change_is_a_session_patch_and_a_stored_list_yields_to_it() {
        let mut s = Session::new();
        let mut state = s.fold.session.clone();
        state.commands = vec![ariadne_api::trajectory::CommandDto {
            name: "review".into(),
            description: "Review".into(),
            hint: None,
        }];
        s.fold = Fold::new("session", state);
        s.fold.set_display_reported(true);
        s.prompt("Go");
        let (_, stored) = s.event(
            "available_commands_update",
            json!({"available_commands": [{"name": "old", "description": "Old"}]}),
        );
        assert!(stored.is_empty(), "{stored:?}");
        assert_eq!(s.fold.session.commands[0].name, "review");

        let (_, ops) = s.event(
            "display_state_update",
            json!({"session_id": "agent", "title": "Fix it",
                   "mode": {"current_id": "ask", "available": [{"id": "ask", "name": "Ask"}]},
                   "cost": {"amount": 1.5, "currency": "EUR"},
                   "config_options": [{"id": "effort", "name": "Effort",
                                       "type": "boolean", "currentValue": true,
                                       "options": [{"group": "g", "name": "G",
                                                    "options": [{"value": "x", "name": "X"}]}]}]}),
        );
        let [TrajectoryOpDto::SessionPatch { session }] = ops.as_slice() else {
            panic!("one patch: {ops:?}");
        };
        assert_eq!(session.title.as_deref(), Some("Fix it"));
        let mode = session.mode.as_ref().unwrap();
        assert_eq!(mode.current_id.as_deref(), Some("ask"));
        assert_eq!(mode.available[0].name, "Ask");
        assert_eq!(session.cost.as_ref().map(|cost| cost.amount), Some(1.5));
        let option = &session.config_options[0];
        assert_eq!(option.current_value.as_deref(), Some("true"));
        assert_eq!(option.values[0].value, "x", "a group's values are listed");
        assert_eq!(session.commands[0].name, "review", "no commands were sent");

        let (_, again) = s.event("display_state_update", json!({"title": "Fix it"}));
        assert!(again.is_empty(), "nothing changed: {again:?}");
    }

    /// The session's state follows its row, its running turn and the
    /// commands its agent offers (rule 29).
    #[test]
    fn the_session_state_follows_the_row_the_running_turn_and_the_commands() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        let (_, ops) = s.event(
            "available_commands_update",
            json!({"available_commands": [{"name": "review", "description": "Review",
                                           "input": {"hint": "a path"}}]}),
        );
        assert!(
            ops.iter()
                .any(|op| matches!(op, TrajectoryOpDto::SessionPatch { session }
                if session.commands[0].name == "review"
                    && session.commands[0].hint.as_deref() == Some("a path")
                    && session.running_turn_id.as_deref() == Some(turn.as_str()))),
            "{ops:?}"
        );

        let mut row: ariadne_api::sessions::SessionDto = serde_json::from_value(json!({
            "id": "session", "goal_id": null, "task_id": null, "seat": null,
            "task_agent_id": null, "step": null, "model": "stub:m", "effort": null,
            "internal_session_id": null, "worktree_path": null, "status": "idle",
            "attention_reason": null, "attention_since": null, "last_activity_at": null,
            "usage": {"input_tokens": 5, "cached_input_tokens": 0, "output_tokens": 7},
            "context_used": 10, "context_size": 100, "created_at": "2026-11-27T00:00:00Z",
            "ended_at": null, "switched_from": null, "pull_request_id": null}))
        .unwrap();
        let patch = s.fold.patch_session(&row).expect("the row changed");
        let TrajectoryOpDto::SessionPatch { session } = patch else {
            panic!("{patch:?}");
        };
        assert_eq!(session.status, SessionStatus::Idle);
        assert_eq!(session.usage.output_tokens, 7);
        assert_eq!(session.context.map(|c| (c.used, c.size)), Some((10, 100)));
        row.status = SessionStatus::Idle;
        assert!(
            s.fold.patch_session(&row).is_none(),
            "nothing changed again"
        );
    }

    /// A run with no `item_id` streams under the id of the chunk that opened
    /// it, with no promise across a new open, and takes the id of its stored
    /// whole when it ends: the id a read of the store gives it (rule 20).
    #[test]
    fn a_run_without_an_item_id_takes_its_wholes_id_when_it_ends() {
        let mut live = Session::new();
        let turn = live.prompt("Go");
        let (chunk, opened) = live.event("agent_message_chunk", json!({"text": "Half "}));
        assert!(
            opened.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step } if step_id(step) == chunk)),
            "{opened:?}"
        );

        let (whole, closed) = live.event("agent_message", json!({"text": "Half done."}));
        assert!(
            closed.contains(&TrajectoryOpDto::ItemRemove {
                turn_id: turn,
                item_id: chunk,
            }),
            "the chunk's id goes: {closed:?}"
        );
        let live_step = live.fold.snapshot(0, 20, None).turns[0].steps[0].clone();
        assert_eq!(step_id(&live_step), whole);

        let mut replay = Session::new();
        replay.prompt("Go");
        replay.next = 2;
        let (stored, _) = replay.event("agent_message", json!({"text": "Half done."}));
        assert_eq!(stored, whole);
        assert_eq!(
            replay.fold.snapshot(0, 20, None).turns[0].steps[0],
            live_step
        );
    }

    /// A stored whole keeps a block per chunk, and reads as the one text the
    /// stream appended (rule 28).
    #[test]
    fn a_stored_whole_reads_its_chunks_as_one_text() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        s.event(
            "agent_message",
            json!({"item_id": "item-1", "text": "All good.",
                   "content": [{"type": "text", "text": "All "}, {"type": "text", "text": "good."}]}),
        );

        let held = s.fold.held_steps(&turn, MAIN_LANE);
        let [StepDto::Message(message)] = held.as_slice() else {
            panic!("{held:?}");
        };
        assert_eq!(message.content, text("All good."));
    }

    /// A chunk that arrives after the whole of its item closed the step is
    /// late, and changes nothing: the whole holds its text (rule 19).
    #[test]
    fn a_chunk_after_its_whole_changes_nothing() {
        let mut s = Session::new();
        let turn = s.prompt("Go");
        s.event(
            "agent_message",
            json!({"item_id": "item-1", "text": "Half done.",
                   "content": [{"type": "text", "text": "Half done."}]}),
        );

        let (_, late) = s.event(
            "agent_message_chunk",
            json!({"item_id": "item-1", "text": "Half "}),
        );

        assert!(late.is_empty(), "{late:?}");
        let held = s.fold.held_steps(&turn, MAIN_LANE);
        let [StepDto::Message(message)] = held.as_slice() else {
            panic!("one message: {held:?}");
        };
        assert_eq!(message.content, text("Half done."));
        assert!(!message.streaming);
    }
}
