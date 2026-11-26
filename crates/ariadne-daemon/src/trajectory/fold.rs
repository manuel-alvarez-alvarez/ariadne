//! The fold: a session's events, one at a time, into turns and the ops that
//! say what each event changed (039).

use std::collections::BTreeSet;

use serde_json::Value;

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::trajectory::{
    ActivityDto, CommandDto, CompactionStatus, CompactionStepDto, ContentBlockDto,
    ContextWindowDto, DiffStatDto, ErrorStepDto, LocationDto, MAIN_LANE, MessageStepDto,
    NoteSeverity, NoteStepDto, PermissionAnswerDto, PermissionOptionDto, PermissionState,
    PermissionStepDto, PermissionToolDto, PlanDto, PlanEntryDto, PlanVariant, PromptDto,
    QuestionOptionDto, QuestionState, QuestionStepDto, SessionStateDto, StepDto, SubagentRefDto,
    SubagentStepDto, ThoughtStepDto, ToolCallStatus, ToolCallStepDto, TrajectoryOpDto,
    TrajectorySnapshotDto, TrajectoryStepsDto, TurnDto, TurnState,
};

/// How many of its last steps a running turn carries. The rest are a page
/// of the steps route away: a turn that ran for an hour holds thousands, and
/// a client draws the last screenful first.
pub(crate) const STEP_WINDOW: usize = 50;

/// What a tool call reads as when its event names no tool.
const UNNAMED_TOOL: &str = "ACP tool";

/// How much of a call's output its step previews. The whole output is the
/// item detail, which a later route serves.
const PREVIEW_CHARS: usize = 400;
const PREVIEW_LINES: usize = 5;

/// The kinds of a session's events, folded into turns.
#[derive(Debug, Clone)]
pub(crate) struct Fold {
    session_id: String,
    turns: Vec<Turn>,
    has_older: bool,
    subagents: Vec<SubagentStepDto>,
    /// Each plan, with the lane it belongs to.
    plans: Vec<(String, PlanDto)>,
    session: SessionStateDto,
}

#[derive(Debug, Clone)]
struct Turn {
    id: String,
    prompt: Option<PromptDto>,
    state: TurnState,
    stop_reason: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    /// Every step of every lane, in the order they opened.
    steps: Vec<StepDto>,
    /// Steps that came before the first event the fold read.
    cut: u64,
}

/// The state of a session as its row says it, before any event.
pub(crate) fn session_state(session: &SessionDto) -> SessionStateDto {
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

impl Fold {
    pub(crate) fn new(session_id: &str, session: SessionStateDto) -> Self {
        Self {
            session_id: session_id.to_string(),
            turns: Vec::new(),
            has_older: false,
            subagents: Vec::new(),
            plans: Vec::new(),
            session,
        }
    }

    /// Whether turns older than the fold's first exist in the store.
    pub(crate) fn set_has_older(&mut self, has_older: bool) {
        self.has_older = has_older;
    }

    /// Count `steps` that the read left out of a turn, before its first
    /// read step.
    pub(crate) fn cut_turn(&mut self, turn_id: &str, steps: u64) {
        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == turn_id) {
            turn.cut += steps;
        }
    }

    /// Forget all but the newest `keep` turns. A hub that follows a session
    /// for a day holds what a snapshot can show, not the whole day.
    pub(crate) fn prune(&mut self, keep: usize) {
        if self.turns.len() > keep {
            self.turns.drain(..self.turns.len() - keep);
            self.has_older = true;
        }
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
                .filter(|turn| turn.steps.iter().any(|step| lane_of(step) == lane))
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
                    Some(lane) => turn.dto_with(lane, STEP_WINDOW),
                    None => turn.dto(),
                })
                .collect(),
            has_older: self.has_older || skip > 0,
            subagents: self.subagents.clone(),
            plans: self.plans.iter().map(|(_, plan)| plan.clone()).collect(),
        }
    }

    /// One page of the steps of a turn in one lane: the `limit` steps before
    /// the step `before`, or the last `limit` steps.
    pub(crate) fn steps(
        &self,
        turn_id: &str,
        lane: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Option<TrajectoryStepsDto> {
        let turn = self.turns.iter().find(|turn| turn.id == turn_id)?;
        let older: Vec<&StepDto> = turn
            .steps
            .iter()
            .filter(|step| lane_of(step) == lane)
            .filter(|step| before.is_none_or(|before| step_id(step) < before))
            .collect();
        let skip = older.len().saturating_sub(limit);
        Some(TrajectoryStepsDto {
            steps: older[skip..].iter().map(|step| (*step).clone()).collect(),
            earlier_steps: skip as u64 + if lane == MAIN_LANE { turn.cut } else { 0 },
        })
    }

    /// The session's row changed: its status, attention, usage or context.
    pub(crate) fn patch_session(&mut self, session: &SessionDto) -> Option<TrajectoryOpDto> {
        let mut next = self.session.clone();
        let row = session_state(session);
        next.status = row.status;
        next.attention_reason = row.attention_reason;
        next.usage = row.usage;
        next.context = row.context;
        (next != self.session).then(|| {
            self.session = next;
            self.session_op()
        })
    }

    /// Fold one event in, and answer what it changed. An event of a kind the
    /// fold does not know changes nothing.
    pub(crate) fn apply(&mut self, event: &AgentEventDto) -> Vec<TrajectoryOpDto> {
        let mut ops = Ops::default();
        let lane = event_lane(event);
        match event.kind.as_str() {
            "user_prompt_submit" => self.open_turn(event, &mut ops),
            "agent_message_chunk" | "agent_thought_chunk" => self.chunk(event, &lane, &mut ops),
            "agent_message" | "agent_thought" => self.whole(event, &lane, &mut ops),
            "pre_tool_use" | "tool_call_update" | "post_tool_use" | "tool_call_abandoned" => {
                self.tool(event, &lane, &mut ops)
            }
            "permission_request" => self.permission(event, &mut ops),
            "permission.replied" => self.reply(event, &mut ops),
            "agent_request" => self.request(event, &mut ops),
            "agent_request.resolved" => self.resolved(event, &mut ops),
            "session.error" => self.error(event, &mut ops),
            "session.diagnosis" => self.diagnosis(event, &mut ops),
            "stop" => self.stop(event, &mut ops),
            "session_start" | "session_end" => {
                self.end_running(TurnState::Interrupted, None, &event.created_at, &mut ops);
                let text = match event.kind.as_str() {
                    "session_start" => "session started",
                    _ => "session ended",
                };
                self.note(event, text.into(), NoteSeverity::Info, &mut ops);
            }
            "session.switched" => {
                self.note(event, event.summary.clone(), NoteSeverity::Info, &mut ops)
            }
            "session.pin_fallback" => self.note(
                event,
                event.summary.clone(),
                NoteSeverity::Warning,
                &mut ops,
            ),
            "plan" => self.plan(event, &lane, &mut ops),
            "subagent_update" => self.subagent(event, &mut ops),
            "compaction_update" => self.compaction(event, &lane, &mut ops),
            "available_commands_update" if lane == MAIN_LANE => {
                self.session.commands = commands_of(&event.payload);
                ops.push(self.session_op());
            }
            _ => {}
        }
        if lane != MAIN_LANE {
            self.recount_subagent(&lane, &mut ops);
        }
        self.finish(ops)
    }

    fn finish(&self, ops: Ops) -> Vec<TrajectoryOpDto> {
        let mut out = ops.ops;
        for id in ops.turns {
            if let Some(turn) = self.turns.iter().find(|turn| turn.id == id) {
                let mut header = turn.dto();
                header.steps.clear();
                out.push(TrajectoryOpDto::TurnUpsert { turn: header });
            }
        }
        out
    }

    fn session_op(&self) -> TrajectoryOpDto {
        TrajectoryOpDto::SessionPatch {
            session: self.session.clone(),
        }
    }

    fn running(&self) -> Option<usize> {
        self.turns
            .iter()
            .rposition(|turn| turn.state == TurnState::Running)
    }

    /// The turn an event belongs to: the running one, else the newest one,
    /// else a turn the event opens with no prompt — what a session does
    /// before its first prompt.
    fn turn_for(&mut self, event: &AgentEventDto, ops: &mut Ops) -> usize {
        if let Some(at) = self.running().or(self.turns.len().checked_sub(1)) {
            return at;
        }
        self.turns.push(Turn {
            id: event.id.clone(),
            prompt: None,
            state: TurnState::Completed,
            stop_reason: None,
            started_at: event.created_at.clone(),
            ended_at: Some(event.created_at.clone()),
            steps: Vec::new(),
            cut: 0,
        });
        ops.touch(&event.id);
        self.turns.len() - 1
    }

    fn open_turn(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        self.end_running(TurnState::Interrupted, None, &event.created_at, ops);
        let payload = &event.payload;
        self.turns.push(Turn {
            id: event.id.clone(),
            prompt: Some(PromptDto {
                id: event.id.clone(),
                source: string_at(payload, "/source"),
                prompt_kind: string_at(payload, "/prompt_kind"),
                content: content_of(payload, &self.session_id, &event.id),
            }),
            state: TurnState::Running,
            stop_reason: None,
            started_at: event.created_at.clone(),
            ended_at: None,
            steps: Vec::new(),
            cut: 0,
        });
        ops.touch(&event.id);
        self.session.running_turn_id = Some(event.id.clone());
        ops.push(self.session_op());
    }

    /// End the running turn, if one runs: its text stops streaming and the
    /// calls still open end with it.
    fn end_running(
        &mut self,
        state: TurnState,
        stop_reason: Option<String>,
        at: &str,
        ops: &mut Ops,
    ) {
        let Some(index) = self.running() else {
            return;
        };
        let call_state = match state {
            TurnState::Cancelled => ToolCallStatus::Cancelled,
            _ => ToolCallStatus::Interrupted,
        };
        let turn = &mut self.turns[index];
        turn.state = state;
        turn.stop_reason = stop_reason;
        turn.ended_at = Some(at.to_string());
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
        for step in self.turns[index].steps.iter_mut().rev() {
            if lane_of(step) != lane {
                continue;
            }
            let streaming = match step {
                StepDto::Message(message) => &mut message.streaming,
                StepDto::Thought(thought) => &mut thought.streaming,
                _ => return,
            };
            if *streaming {
                *streaming = false;
                ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            }
            return;
        }
    }

    fn push_step(&mut self, index: usize, step: StepDto, ops: &mut Ops) {
        self.end_streaming(index, lane_of(&step), ops);
        ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
        let turn = &mut self.turns[index];
        ops.touch(&turn.id);
        turn.steps.push(step);
    }

    fn chunk(&mut self, event: &AgentEventDto, lane: &str, ops: &mut Ops) {
        let index = self.turn_for(event, ops);
        let thought = event.kind == "agent_thought_chunk";
        let text = string_at(&event.payload, "/text").unwrap_or_default();
        let item_id = string_at(&event.payload, "/item_id");
        let turn = &mut self.turns[index];
        if let Some(step) = turn
            .steps
            .iter_mut()
            .rev()
            .find(|step| lane_of(step) == lane)
        {
            let open = match step {
                StepDto::Message(message) if !thought && message.streaming => {
                    Some((&message.id, &mut message.content))
                }
                StepDto::Thought(thought_step) if thought && thought_step.streaming => {
                    Some((&thought_step.id, &mut thought_step.content))
                }
                _ => None,
            };
            if let Some((id, content)) = open
                && item_id.as_ref().is_none_or(|item| item == id)
            {
                append_text(content, &text);
                ops.push(TrajectoryOpDto::TextAppend {
                    turn_id: turn.id.clone(),
                    item_id: id.clone(),
                    text,
                });
                return;
            }
        }
        let id = item_id.unwrap_or_else(|| event.id.clone());
        let step = text_step(
            thought,
            id,
            turn.id.clone(),
            lane,
            event,
            vec![ContentBlockDto::Text { text }],
            true,
        );
        self.push_step(index, step, ops);
    }

    fn whole(&mut self, event: &AgentEventDto, lane: &str, ops: &mut Ops) {
        let index = self.turn_for(event, ops);
        let thought = event.kind == "agent_thought";
        let content = content_of(&event.payload, &self.session_id, &event.id);
        let item_id = string_at(&event.payload, "/item_id");
        let turn = &mut self.turns[index];
        let found = turn
            .steps
            .iter_mut()
            .rev()
            .find(|step| match (step, &item_id) {
                (StepDto::Message(message), Some(item)) => !thought && &message.id == item,
                (StepDto::Thought(thought_step), Some(item)) => thought && &thought_step.id == item,
                (step, None) => {
                    lane_of(step) == lane
                        && match step {
                            StepDto::Message(message) => !thought && message.streaming,
                            StepDto::Thought(thought_step) => thought && thought_step.streaming,
                            _ => false,
                        }
                }
                _ => false,
            });
        if let Some(step) = found {
            match step {
                StepDto::Message(message) => {
                    message.content = content;
                    message.streaming = false;
                    if message.message_id.is_none() {
                        message.message_id = string_at(&event.payload, "/message_id");
                    }
                }
                StepDto::Thought(thought_step) => {
                    thought_step.content = content;
                    thought_step.streaming = false;
                }
                _ => {}
            }
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            ops.touch(&turn.id);
            return;
        }
        let id = item_id.unwrap_or_else(|| event.id.clone());
        let step = text_step(thought, id, turn.id.clone(), lane, event, content, false);
        self.push_step(index, step, ops);
    }

    fn tool(&mut self, event: &AgentEventDto, lane: &str, ops: &mut Ops) {
        let payload = &event.payload;
        let acp = payload.get("acp").unwrap_or(payload);
        let tool_call_id = string_at(acp, "/toolCallId")
            .or_else(|| string_at(payload, "/tool_call_id"))
            .unwrap_or_default();
        let found = (event.kind != "pre_tool_use")
            .then(|| self.open_call(lane, &tool_call_id))
            .flatten();
        let Some((index, at)) = found else {
            let index = self.turn_for(event, ops);
            let mut call = new_call(event, &self.turns[index].id, lane, &tool_call_id);
            merge_call(&mut call, event);
            call.permission = self.turns[index].steps.iter().find_map(|step| match step {
                StepDto::Permission(permission)
                    if permission.tool_call_id == tool_call_id && !tool_call_id.is_empty() =>
                {
                    Some(permission.id.clone())
                }
                _ => None,
            });
            self.push_step(index, StepDto::ToolCall(call), ops);
            return;
        };
        self.end_streaming(index, lane, ops);
        let turn = &mut self.turns[index];
        if let StepDto::ToolCall(call) = &mut turn.steps[at] {
            merge_call(call, event);
            call.detail_rev += 1;
        }
        ops.push(TrajectoryOpDto::ItemUpsert {
            step: turn.steps[at].clone(),
        });
        ops.touch(&turn.id);
    }

    /// The newest call of a lane that has not ended, by its ACP id.
    fn open_call(&self, lane: &str, tool_call_id: &str) -> Option<(usize, usize)> {
        self.turns
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, turn)| {
                turn.steps
                    .iter()
                    .rposition(|step| match step {
                        StepDto::ToolCall(call) => {
                            call.lane == lane
                                && call.tool_call_id == tool_call_id
                                && !is_terminal(call.status)
                        }
                        _ => false,
                    })
                    .map(|at| (index, at))
            })
    }

    fn permission(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let index = self.turn_for(event, ops);
        let payload = &event.payload;
        let acp = payload.get("acp").unwrap_or(payload);
        let tool_call_id = string_at(acp, "/toolCallId").unwrap_or_default();
        let title = string_at(payload, "/tool_name")
            .or_else(|| string_at(acp, "/title"))
            .unwrap_or_else(|| event.summary.clone());
        let kind = string_at(acp, "/kind");
        let input = payload
            .get("tool_input")
            .or_else(|| acp.get("rawInput"))
            .unwrap_or(&Value::Null);
        let subagent = subagent_ref(payload);
        let question = match &subagent {
            Some(child) => format!("{title} · asked by subagent {}", child.title),
            None => title.clone(),
        };
        let step = PermissionStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: event.created_at.clone(),
            tool_call_id: tool_call_id.clone(),
            tool: PermissionToolDto {
                subject: subject_of(kind.as_deref(), input, &locations_of(acp)),
                title,
                kind,
            },
            question,
            options: permission_options(payload),
            state: PermissionState::Pending,
            answer: None,
            ai: ariadne_api::permissions::ai_permission_note(payload),
            learned: None,
            subagent: subagent.clone(),
        };
        let call_lane = subagent.map_or_else(|| MAIN_LANE.to_string(), |child| child.id);
        if let Some((call_index, at)) = self.open_call(&call_lane, &tool_call_id)
            && let StepDto::ToolCall(call) = &mut self.turns[call_index].steps[at]
        {
            call.permission = Some(event.id.clone());
            ops.push(TrajectoryOpDto::ItemUpsert {
                step: StepDto::ToolCall(call.clone()),
            });
        }
        self.push_step(index, StepDto::Permission(step), ops);
    }

    fn reply(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let payload = &event.payload;
        let Some((index, at)) = self.find_step(|step| {
            matches!(step, StepDto::Permission(permission) if permission.state == PermissionState::Pending)
        }) else {
            return;
        };
        let turn = &mut self.turns[index];
        ops.touch(&turn.id);
        let StepDto::Permission(permission) = &mut turn.steps[at] else {
            return;
        };
        // A permission on one of Ariadne's own tools that the daemon allowed
        // by itself asked nobody anything: the call's step says what the
        // agent did. A person's answer, a denial and a cancel stay.
        if permission
            .tool
            .title
            .to_ascii_lowercase()
            .contains("ariadne")
            && daemon_allowed(payload, &permission.options)
        {
            let id = permission.id.clone();
            let tool_call_id = permission.tool_call_id.clone();
            turn.steps.remove(at);
            ops.push(TrajectoryOpDto::ItemRemove {
                turn_id: turn.id.clone(),
                item_id: id.clone(),
            });
            for step in &mut turn.steps {
                if let StepDto::ToolCall(call) = step
                    && call.tool_call_id == tool_call_id
                    && call.permission.as_deref() == Some(id.as_str())
                {
                    call.permission = None;
                    ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
                }
            }
            return;
        }
        let option_id = string_at(payload, "/option_id");
        let decided_by = string_at(payload, "/decided_by");
        let chosen = string_at(payload, "/console_option_id").or_else(|| option_id.clone());
        let label = match (&option_id, &chosen) {
            (None, _) => "cancelled".to_string(),
            (Some(id), chosen) => permission
                .options
                .iter()
                .find(|option| Some(&option.id) == chosen.as_ref())
                .map_or_else(|| id.clone(), |option| option.name.clone()),
        };
        permission.state = match option_id {
            Some(_) => PermissionState::Answered,
            None => PermissionState::Cancelled,
        };
        if decided_by.as_deref() == Some("ai")
            && let Some(note) = ariadne_api::permissions::ai_permission_note(payload)
        {
            permission.ai = Some(note);
        }
        if decided_by.as_deref() == Some("learned") {
            permission.learned = match string_at(payload, "/learned_level").as_deref() {
                Some("family") => Some(format!(
                    "family {}",
                    string_at(payload, "/learned_key").unwrap_or_default()
                )),
                Some(level) => Some(level.to_string()),
                None => None,
            };
        }
        permission.answer = Some(PermissionAnswerDto {
            option_id,
            label,
            decided_by,
        });
        ops.push(TrajectoryOpDto::ItemUpsert {
            step: turn.steps[at].clone(),
        });
    }

    fn request(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let payload = &event.payload;
        let hint = string_at(payload, "/hint");
        let Some(question) = payload.get("question").filter(|value| value.is_object()) else {
            let text = match hint {
                Some(hint) => format!("waiting for you · {hint}"),
                None => "waiting for you".into(),
            };
            self.note(event, text, NoteSeverity::Info, ops);
            return;
        };
        let index = self.turn_for(event, ops);
        let step = QuestionStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: event.created_at.clone(),
            request_id: string_at(payload, "/request_id").unwrap_or_else(|| event.id.clone()),
            hint: hint.unwrap_or_default(),
            question: string_at(question, "/text").unwrap_or_default(),
            options: question
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
                .collect(),
            recommended: string_at(question, "/recommended_option_id"),
            recommendation_reason: string_at(question, "/recommendation_reason"),
            state: QuestionState::Open,
            answer: None,
        };
        self.push_step(index, StepDto::Question(step), ops);
    }

    fn resolved(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let request = string_at(&event.payload, "/request_id");
        let Some((index, at)) = self.find_step(|step| {
            matches!(step, StepDto::Question(question) if Some(&question.request_id) == request.as_ref())
        }) else {
            return;
        };
        let turn = &mut self.turns[index];
        if let StepDto::Question(question) = &mut turn.steps[at] {
            question.state = match string_at(&event.payload, "/resolution").as_deref() {
                Some("withdrawn") => QuestionState::Withdrawn,
                Some("expired") => QuestionState::Expired,
                _ => QuestionState::Answered,
            };
            question.answer = string_at(&event.payload, "/answer/text");
        }
        ops.push(TrajectoryOpDto::ItemUpsert {
            step: turn.steps[at].clone(),
        });
        ops.touch(&turn.id);
    }

    fn error(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let index = self.turn_for(event, ops);
        let payload = &event.payload;
        let step = ErrorStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: event.created_at.clone(),
            text: string_at(payload, "/error/data/message")
                .or_else(|| string_at(payload, "/error/message"))
                .or_else(|| string_at(payload, "/message"))
                .unwrap_or_else(|| event.summary.clone()),
            diagnosis: None,
        };
        self.push_step(index, StepDto::Error(step), ops);
        self.end_running(TurnState::Failed, None, &event.created_at, ops);
    }

    fn diagnosis(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let error_id = string_at(&event.payload, "/error_event_id");
        let Some((index, at)) = self.find_step(
            |step| matches!(step, StepDto::Error(error) if Some(&error.id) == error_id.as_ref()),
        ) else {
            return;
        };
        let turn = &mut self.turns[index];
        if let StepDto::Error(error) = &mut turn.steps[at] {
            error.diagnosis = Some(
                ariadne_api::events::diagnosis_note(&event.payload)
                    .unwrap_or_else(|| event.summary.clone()),
            );
        }
        ops.push(TrajectoryOpDto::ItemUpsert {
            step: turn.steps[at].clone(),
        });
        ops.touch(&turn.id);
    }

    fn stop(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let reason = string_at(&event.payload, "/stop_reason");
        let state = match reason.as_deref() {
            Some("cancelled") => TurnState::Cancelled,
            _ => TurnState::Completed,
        };
        self.end_running(state, reason, &event.created_at, ops);
    }

    fn note(&mut self, event: &AgentEventDto, text: String, severity: NoteSeverity, ops: &mut Ops) {
        let index = self.turn_for(event, ops);
        let step = NoteStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: event.created_at.clone(),
            origin: event.kind.clone(),
            severity,
            text,
        };
        self.push_step(index, StepDto::Note(step), ops);
    }

    fn plan(&mut self, event: &AgentEventDto, lane: &str, ops: &mut Ops) {
        let turn_id = self
            .running()
            .or(self.turns.len().checked_sub(1))
            .map(|index| self.turns[index].id.clone());
        let entries = event
            .payload
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| PlanEntryDto {
                content: string_at(entry, "/content").unwrap_or_default(),
                status: string_at(entry, "/status").unwrap_or_else(|| "pending".into()),
                priority: string_at(entry, "/priority"),
            })
            .collect();
        let plan_id = self
            .plans
            .iter()
            .find(|(plan_lane, _)| plan_lane == lane)
            .map_or_else(|| event.id.clone(), |(_, plan)| plan.plan_id.clone());
        let plan = PlanDto {
            plan_id,
            variant: PlanVariant::Entries,
            entries,
            markdown: None,
            path: None,
            turn_id,
            removed: false,
            updated_at: event.created_at.clone(),
        };
        match self
            .plans
            .iter_mut()
            .find(|(plan_lane, _)| plan_lane == lane)
        {
            Some((_, kept)) => *kept = plan.clone(),
            None => self.plans.push((lane.to_string(), plan.clone())),
        }
        ops.push(TrajectoryOpDto::PlanUpsert { plan });
    }

    fn subagent(&mut self, event: &AgentEventDto, ops: &mut Ops) {
        let payload = &event.payload;
        let Some(id) = string_at(payload, "/subagent/id") else {
            return;
        };
        let at = self.subagent_at(event, &id, ops);
        let child = &mut self.subagents[at];
        if let Some(title) = string_at(payload, "/subagent/title") {
            child.title = title;
        }
        if let Some(task) = string_at(payload, "/subagent/description") {
            child.task = Some(task);
        }
        child.state = string_at(payload, "/state");
        child.can_cancel = payload.pointer("/capabilities/cancel") == Some(&Value::Bool(true));
        let ended = matches!(
            child.state.as_deref(),
            Some("completed" | "failed" | "cancelled")
        );
        if ended && child.ended_at.is_none() {
            child.ended_at = Some(event.created_at.clone());
        }
        self.publish_subagent(at, ops);
    }

    /// The index of a subagent, which the first event that names it
    /// announces.
    fn subagent_at(&mut self, event: &AgentEventDto, id: &str, ops: &mut Ops) -> usize {
        if let Some(at) = self
            .subagents
            .iter()
            .position(|child| child.subagent_id == id)
        {
            return at;
        }
        let index = self.turn_for(event, ops);
        let step = SubagentStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: MAIN_LANE.into(),
            created_at: event.created_at.clone(),
            subagent_id: id.to_string(),
            title: string_at(&event.payload, "/subagent/title").unwrap_or_else(|| id.to_string()),
            task: string_at(&event.payload, "/subagent/description"),
            state: None,
            can_cancel: false,
            started_at: event.created_at.clone(),
            ended_at: None,
            activity: ActivityDto::default(),
        };
        self.subagents.push(step.clone());
        self.push_step(index, StepDto::Subagent(step), ops);
        self.subagents.len() - 1
    }

    /// Put the registry's copy of a subagent into its step, and say so.
    fn publish_subagent(&mut self, at: usize, ops: &mut Ops) {
        let child = self.subagents[at].clone();
        if let Some(turn) = self.turns.iter_mut().find(|turn| turn.id == child.turn_id)
            && let Some(step) = turn.steps.iter_mut().find(
                |step| matches!(step, StepDto::Subagent(kept) if kept.subagent_id == child.subagent_id),
            )
        {
            *step = StepDto::Subagent(child.clone());
            ops.push(TrajectoryOpDto::ItemUpsert { step: step.clone() });
            ops.touch(&turn.id);
        }
        ops.push(TrajectoryOpDto::SubagentUpsert { subagent: child });
    }

    /// A subagent's activity is counted over its own lane, in every turn the
    /// fold holds.
    fn recount_subagent(&mut self, lane: &str, ops: &mut Ops) {
        let Some(at) = self
            .subagents
            .iter()
            .position(|child| child.subagent_id == lane)
        else {
            return;
        };
        let steps: Vec<&StepDto> = self
            .turns
            .iter()
            .flat_map(|turn| turn.steps.iter())
            .filter(|step| lane_of(step) == lane)
            .collect();
        let activity = activity_of(steps.into_iter());
        if activity != self.subagents[at].activity {
            self.subagents[at].activity = activity;
            self.publish_subagent(at, ops);
        }
    }

    fn compaction(&mut self, event: &AgentEventDto, lane: &str, ops: &mut Ops) {
        let payload = &event.payload;
        let compaction_id =
            string_at(payload, "/compaction_id").unwrap_or_else(|| event.id.clone());
        let status = match string_at(payload, "/status").as_deref() {
            Some("running" | "started" | "in_progress") => CompactionStatus::Running,
            Some("failed") => CompactionStatus::Failed,
            _ => CompactionStatus::Completed,
        };
        let summary = match payload.get("summary") {
            Some(Value::String(text)) => vec![ContentBlockDto::Text { text: text.clone() }],
            Some(Value::Array(blocks)) => blocks_of(blocks, &self.session_id, &event.id),
            _ => Vec::new(),
        };
        let error = string_at(payload, "/error");
        if let Some((index, at)) = self.find_step(
            |step| matches!(step, StepDto::Compaction(kept) if kept.compaction_id == compaction_id),
        ) {
            let turn = &mut self.turns[index];
            if let StepDto::Compaction(kept) = &mut turn.steps[at] {
                kept.status = status;
                if !summary.is_empty() {
                    kept.summary = summary;
                }
                kept.error = error;
            }
            ops.push(TrajectoryOpDto::ItemUpsert {
                step: turn.steps[at].clone(),
            });
            return;
        }
        let index = self.turn_for(event, ops);
        let step = CompactionStepDto {
            id: event.id.clone(),
            turn_id: self.turns[index].id.clone(),
            lane: lane.to_string(),
            created_at: event.created_at.clone(),
            compaction_id,
            status,
            summary,
            error,
        };
        self.push_step(index, StepDto::Compaction(step), ops);
    }

    /// The newest step that matches, as its turn's index and its own.
    fn find_step(&self, wanted: impl Fn(&StepDto) -> bool) -> Option<(usize, usize)> {
        self.turns
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, turn)| turn.steps.iter().rposition(&wanted).map(|at| (index, at)))
    }
}

impl Turn {
    /// The turn as a client reads it: a running turn with its last steps,
    /// a closed one with none.
    fn dto(&self) -> TurnDto {
        let main: Vec<&StepDto> = self
            .steps
            .iter()
            .filter(|step| lane_of(step) == MAIN_LANE)
            .collect();
        let carried = match self.state {
            TurnState::Running => main.len().min(STEP_WINDOW),
            _ => 0,
        };
        let mut dto = self.header(main.iter().copied());
        dto.steps = main[main.len() - carried..]
            .iter()
            .map(|step| (*step).clone())
            .collect();
        dto.earlier_steps = self.cut + (main.len() - carried) as u64;
        dto
    }

    /// The turn with the last `window` steps of one subagent's lane.
    fn dto_with(&self, lane: &str, window: usize) -> TurnDto {
        let main = self.steps.iter().filter(|step| lane_of(step) == MAIN_LANE);
        let mut dto = self.header(main);
        let steps: Vec<&StepDto> = self
            .steps
            .iter()
            .filter(|step| lane_of(step) == lane)
            .collect();
        let carried = steps.len().min(window);
        dto.steps = steps[steps.len() - carried..]
            .iter()
            .map(|step| (*step).clone())
            .collect();
        dto.earlier_steps = (steps.len() - carried) as u64;
        dto
    }

    fn header<'a>(&self, main: impl Iterator<Item = &'a StepDto> + Clone) -> TurnDto {
        TurnDto {
            id: self.id.clone(),
            lane: MAIN_LANE.into(),
            prompt: self.prompt.clone(),
            state: self.state,
            stop_reason: self.stop_reason.clone(),
            started_at: self.started_at.clone(),
            ended_at: self.ended_at.clone(),
            activity: activity_of(main.clone()),
            steps: Vec::new(),
            earlier_steps: 0,
            attention: main
                .clone()
                .filter(|step| match step {
                    StepDto::Permission(permission) => permission.state == PermissionState::Pending,
                    StepDto::Question(question) => question.state == QuestionState::Open,
                    StepDto::Error(_) => true,
                    _ => false,
                })
                .cloned()
                .collect(),
            answer: main
                .filter_map(|step| match step {
                    StepDto::Message(message) => Some(message.content.clone()),
                    _ => None,
                })
                .last()
                .unwrap_or_default(),
        }
    }
}

/// What a set of steps did, counted.
fn activity_of<'a>(steps: impl Iterator<Item = &'a StepDto>) -> ActivityDto {
    let mut activity = ActivityDto::default();
    let mut changed = BTreeSet::new();
    for step in steps {
        match step {
            StepDto::ToolCall(call) => {
                activity.steps += 1;
                activity.tool_calls += 1;
                match call.status {
                    ToolCallStatus::Failed => activity.failed += 1,
                    ToolCallStatus::Cancelled => activity.cancelled += 1,
                    ToolCallStatus::Completed
                        if matches!(call.kind.as_deref(), Some("edit" | "delete" | "move")) =>
                    {
                        changed.extend(call.locations.iter().map(|location| &location.path));
                    }
                    _ => {}
                }
            }
            StepDto::Subagent(_) => {
                activity.steps += 1;
                activity.subagents += 1;
            }
            StepDto::Thought(_) => activity.thoughts += 1,
            _ => {}
        }
    }
    activity.files_changed = changed.len() as u64;
    activity
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

/// The lane an event is in: the subagent it names, or the session's own.
/// A permission is the session's to answer, so it is in the main lane
/// whoever asks.
fn event_lane(event: &AgentEventDto) -> String {
    match event.kind.as_str() {
        "permission_request" | "permission.replied" | "subagent_update" => MAIN_LANE.into(),
        _ => string_at(&event.payload, "/subagent/id").unwrap_or_else(|| MAIN_LANE.into()),
    }
}

fn subagent_ref(payload: &Value) -> Option<SubagentRefDto> {
    let id = string_at(payload, "/subagent/id")?;
    Some(SubagentRefDto {
        title: string_at(payload, "/subagent/title").unwrap_or_else(|| id.clone()),
        id,
    })
}

fn step_id(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.id,
        StepDto::Thought(step) => &step.id,
        StepDto::ToolCall(step) => &step.id,
        StepDto::Permission(step) => &step.id,
        StepDto::Question(step) => &step.id,
        StepDto::Note(step) => &step.id,
        StepDto::Error(step) => &step.id,
        StepDto::Subagent(step) => &step.id,
        StepDto::Compaction(step) => &step.id,
    }
}

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

fn text_step(
    thought: bool,
    id: String,
    turn_id: String,
    lane: &str,
    event: &AgentEventDto,
    content: Vec<ContentBlockDto>,
    streaming: bool,
) -> StepDto {
    if thought {
        StepDto::Thought(ThoughtStepDto {
            id,
            turn_id,
            lane: lane.to_string(),
            created_at: event.created_at.clone(),
            content,
            streaming,
        })
    } else {
        StepDto::Message(MessageStepDto {
            id,
            turn_id,
            lane: lane.to_string(),
            created_at: event.created_at.clone(),
            content,
            streaming,
            message_id: string_at(&event.payload, "/message_id"),
        })
    }
}

fn append_text(content: &mut Vec<ContentBlockDto>, text: &str) {
    match content.last_mut() {
        Some(ContentBlockDto::Text { text: last }) => last.push_str(text),
        _ => content.push(ContentBlockDto::Text {
            text: text.to_string(),
        }),
    }
}

/// The content an event carries: its ACP `content` blocks, else its `text`.
fn content_of(payload: &Value, session_id: &str, event_id: &str) -> Vec<ContentBlockDto> {
    if let Some(blocks) = payload.get("content").and_then(Value::as_array) {
        return blocks_of(blocks, session_id, event_id);
    }
    string_at(payload, "/text")
        .map(|text| vec![ContentBlockDto::Text { text }])
        .unwrap_or_default()
}

/// ACP content blocks, with every byte left behind a `blob` URL.
fn blocks_of(blocks: &[Value], session_id: &str, event_id: &str) -> Vec<ContentBlockDto> {
    let blob =
        |index: usize| format!("/v1/sessions/{session_id}/trajectory/blobs/{event_id}/{index}");
    blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let has_data =
                |pointer: &str| block.pointer(pointer).is_some_and(|data| !data.is_null());
            Some(match string_at(block, "/type")?.as_str() {
                "text" => ContentBlockDto::Text {
                    text: string_at(block, "/text").unwrap_or_default(),
                },
                "image" => ContentBlockDto::Image {
                    mime_type: string_at(block, "/mimeType"),
                    blob: has_data("/data").then(|| blob(index)),
                    uri: string_at(block, "/uri"),
                },
                "audio" => ContentBlockDto::Audio {
                    mime_type: string_at(block, "/mimeType"),
                    blob: has_data("/data").then(|| blob(index)),
                },
                "resource" => ContentBlockDto::Resource {
                    uri: string_at(block, "/resource/uri").unwrap_or_default(),
                    mime_type: string_at(block, "/resource/mimeType"),
                    text: string_at(block, "/resource/text"),
                    blob: has_data("/resource/blob").then(|| blob(index)),
                },
                "resource_link" => ContentBlockDto::ResourceLink {
                    uri: string_at(block, "/uri").unwrap_or_default(),
                    name: string_at(block, "/name").unwrap_or_default(),
                    title: string_at(block, "/title"),
                    mime_type: string_at(block, "/mimeType"),
                    size: block.get("size").and_then(Value::as_u64),
                },
                _ => return None,
            })
        })
        .collect()
}

fn new_call(
    event: &AgentEventDto,
    turn_id: &str,
    lane: &str,
    tool_call_id: &str,
) -> ToolCallStepDto {
    ToolCallStepDto {
        id: event.id.clone(),
        turn_id: turn_id.to_string(),
        lane: lane.to_string(),
        created_at: event.created_at.clone(),
        tool_call_id: tool_call_id.to_string(),
        title: UNNAMED_TOOL.into(),
        kind: None,
        status: ToolCallStatus::Pending,
        subject: None,
        locations: Vec::new(),
        preview: None,
        diff_stat: None,
        started_at: event.created_at.clone(),
        ended_at: None,
        permission: None,
        detail_rev: 0,
    }
}

/// Take what an event of a call says, and keep what it does not say again.
fn merge_call(call: &mut ToolCallStepDto, event: &AgentEventDto) {
    let payload = &event.payload;
    let acp = payload.get("acp").unwrap_or(payload);
    if let Some(title) = string_at(payload, "/tool_name").or_else(|| string_at(acp, "/title"))
        && title != UNNAMED_TOOL
    {
        call.title = title;
    }
    if let Some(kind) = string_at(acp, "/kind") {
        call.kind = Some(kind);
    }
    let mut locations = locations_of(acp);
    let diffs = diffs_of(acp);
    if locations.is_empty() {
        locations = diffs
            .iter()
            .filter_map(|(path, _, _)| path.clone())
            .map(|path| LocationDto { path, line: None })
            .collect();
    }
    if !locations.is_empty() {
        call.locations = locations;
    }
    if !diffs.is_empty() {
        call.diff_stat = Some(DiffStatDto {
            files: diffs.len() as u64,
            added: diffs.iter().map(|(_, added, _)| added).sum(),
            removed: diffs.iter().map(|(_, _, removed)| removed).sum(),
        });
    }
    let input = payload
        .get("tool_input")
        .or_else(|| acp.get("rawInput"))
        .filter(|input| !input.is_null());
    if let Some(input) = input {
        call.subject = subject_of(call.kind.as_deref(), input, &call.locations);
    } else if call.subject.is_none() && !call.locations.is_empty() {
        call.subject = subject_of(call.kind.as_deref(), &Value::Null, &call.locations);
    }
    if let Some(preview) = preview_of(acp) {
        call.preview = Some(preview);
    }
    let status = match event.kind.as_str() {
        "tool_call_abandoned" => Some(match string_at(payload, "/reason").as_deref() {
            Some("cancelled") => ToolCallStatus::Cancelled,
            _ => ToolCallStatus::Interrupted,
        }),
        _ => match string_at(acp, "/status").as_deref() {
            Some("pending") => Some(ToolCallStatus::Pending),
            Some("in_progress") => Some(ToolCallStatus::Running),
            Some("completed") => Some(ToolCallStatus::Completed),
            Some("failed") => Some(ToolCallStatus::Failed),
            Some("cancelled") => Some(ToolCallStatus::Cancelled),
            _ => None,
        },
    };
    if let Some(status) = status {
        call.status = status;
    }
    if is_terminal(call.status) && call.ended_at.is_none() {
        call.ended_at = Some(event.created_at.clone());
    }
}

fn is_terminal(status: ToolCallStatus) -> bool {
    !matches!(status, ToolCallStatus::Pending | ToolCallStatus::Running)
}

fn locations_of(acp: &Value) -> Vec<LocationDto> {
    acp.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            Some(LocationDto {
                path: string_at(location, "/path")?,
                line: location.get("line").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// Each diff a call carries: its path, and the lines it adds and removes.
/// A patch counts its `+` and `-` lines; an old and a new text count the
/// lines between their common start and their common end.
fn diffs_of(acp: &Value) -> Vec<(Option<String>, u64, u64)> {
    acp.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("diff"))
        .map(|entry| {
            let path = string_at(entry, "/path");
            if let Some(patch) = string_at(entry, "/patch/text") {
                let lines = patch.lines();
                let added = lines
                    .clone()
                    .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                    .count() as u64;
                let removed = lines
                    .filter(|line| line.starts_with('-') && !line.starts_with("---"))
                    .count() as u64;
                return (path, added, removed);
            }
            let old = string_at(entry, "/oldText").unwrap_or_default();
            let new = string_at(entry, "/newText").unwrap_or_default();
            let old: Vec<&str> = old.lines().collect();
            let new: Vec<&str> = new.lines().collect();
            let start = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
            let end = old[start..]
                .iter()
                .rev()
                .zip(new[start..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            (
                path,
                (new.len() - start - end) as u64,
                (old.len() - start - end) as u64,
            )
        })
        .collect()
}

/// The start of what a call gave back: the text of its content, else its
/// raw output.
fn preview_of(acp: &Value) -> Option<String> {
    let text: Vec<String> = acp
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| string_at(entry, "/type").as_deref() == Some("content"))
        .filter_map(|entry| string_at(entry, "/content/text"))
        .filter(|text| !text.is_empty())
        .collect();
    let whole = if !text.is_empty() {
        text.join("\n")
    } else {
        match acp.get("rawOutput") {
            None | Some(Value::Null) => return None,
            Some(Value::String(text)) => text.clone(),
            Some(Value::Object(object))
                if object.contains_key("stdout") || object.contains_key("stderr") =>
            {
                ["stdout", "stderr"]
                    .into_iter()
                    .filter_map(|key| object.get(key)?.as_str())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Some(raw) => raw.to_string(),
        }
    };
    let lines: Vec<&str> = whole.lines().take(PREVIEW_LINES).collect();
    let preview: String = lines.join("\n").chars().take(PREVIEW_CHARS).collect();
    (!preview.is_empty()).then_some(preview)
}

/// What a call is about: the command, the path, the pattern in its path or
/// the URL, by its kind.
fn subject_of(kind: Option<&str>, input: &Value, locations: &[LocationDto]) -> Option<String> {
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| string_at(input, &format!("/{key}")))
    };
    let first = locations.first();
    match kind {
        Some("execute") => match input.get("command") {
            Some(Value::String(command)) => command.lines().next().map(str::to_string),
            Some(Value::Array(parts)) => Some(
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        },
        Some("read" | "edit" | "delete" | "move") => {
            let path = field(&["path", "file_path", "filePath", "file"])
                .or_else(|| first.map(|location| location.path.clone()))?;
            let line = input.get("line").and_then(Value::as_u64).or_else(|| {
                first
                    .filter(|location| location.path == path)
                    .and_then(|location| location.line)
            });
            Some(match line {
                Some(line) => format!("{path}:{line}"),
                None => path,
            })
        }
        Some("search") => {
            let pattern = field(&["pattern", "query", "regex"])?;
            match field(&["path", "file_path", "directory", "cwd"])
                .or_else(|| first.map(|location| location.path.clone()))
            {
                Some(path) => Some(format!("{pattern} in {path}")),
                None => Some(pattern),
            }
        }
        Some("fetch") => field(&["url"]),
        _ => first.map(|location| location.path.clone()),
    }
}

fn permission_options(payload: &Value) -> Vec<PermissionOptionDto> {
    payload
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let id = string_at(option, "/optionId")?;
            Some(PermissionOptionDto {
                name: string_at(option, "/name").unwrap_or_else(|| id.clone()),
                kind: string_at(option, "/kind"),
                id,
            })
        })
        .collect()
}

/// Whether a reply is the daemon's own allow: an option chosen with no
/// person at the console that is not a denial.
fn daemon_allowed(payload: &Value, options: &[PermissionOptionDto]) -> bool {
    let Some(id) = string_at(payload, "/option_id") else {
        return false;
    };
    if string_at(payload, "/decided_by").as_deref() == Some("console")
        || string_at(payload, "/label").as_deref() == Some("deny")
    {
        return false;
    }
    match options
        .iter()
        .find(|option| option.id == id)
        .and_then(|option| option.kind.as_deref())
    {
        Some(kind) => kind.starts_with("allow"),
        None => true,
    }
}

fn commands_of(payload: &Value) -> Vec<CommandDto> {
    payload
        .get("available_commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| {
            Some(CommandDto {
                name: string_at(command, "/name")?,
                description: string_at(command, "/description").unwrap_or_default(),
                hint: string_at(command, "/input/hint"),
            })
        })
        .collect()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use ariadne_api::events::AgentEventDto;
    use ariadne_api::trajectory::{
        ActivityDto, ContentBlockDto, MAIN_LANE, PermissionState, QuestionState, SessionStateDto,
        StepDto, TrajectoryOpDto, TurnState,
    };
    use ariadne_api::usage::TokenUsageDto;
    use ariadne_core::SessionStatus;
    use serde_json::{Value, json};

    use super::{Fold, STEP_WINDOW};

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
            let ops = self.fold.apply(&event);
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

        let (opened, first) = s.event("agent_message_chunk", json!({"text": "Half "}));
        let (_, second) = s.event("agent_message_chunk", json!({"text": "done."}));
        let (_, whole) = s.event("agent_message", json!({"text": "Half done."}));

        assert!(
            first.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step: StepDto::Message(m) }
                    if m.id == opened && m.streaming && m.content == text("Half "))),
            "{first:?}"
        );
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
                    if m.id == opened && !m.streaming && m.content == text("Half done."))),
            "{whole:?}"
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
            "agent_request",
            json!({"request_id": "req-1", "hint": "pick one",
                   "question": {"text": "Which?", "options": [{"id": "a", "label": "A"}]}}),
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
            "agent_request.resolved",
            json!({"request_id": "req-1", "resolution": "answered", "answer": {"text": "A"}}),
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
        let StepDto::Permission(answered) = &s
            .fold
            .steps(&snapshot.turns[2].id, MAIN_LANE, None, 10)
            .unwrap()
            .steps[0]
        else {
            panic!("the permission step");
        };
        assert_eq!(answered.state, PermissionState::Answered);
        assert_eq!(answered.answer.as_ref().unwrap().label, "Allow");
        let page = s
            .fold
            .steps(&snapshot.turns[0].id, MAIN_LANE, None, 10)
            .unwrap();
        assert!(
            matches!(&page.steps[0], StepDto::Question(q) if q.state == QuestionState::Answered)
        );
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

    /// The steps of a turn come in pages, oldest first, before a step.
    #[test]
    fn the_steps_of_a_turn_come_in_pages() {
        let mut s = Session::new();
        let turn = s.prompt("Read");
        for n in 0..5 {
            s.call(&format!("call-{n}"), "read", "completed", "a.rs");
        }
        s.stop();

        let last = s.fold.steps(&turn, MAIN_LANE, None, 2).unwrap();
        let ids: Vec<String> = last
            .steps
            .iter()
            .map(|step| match step {
                StepDto::ToolCall(call) => call.tool_call_id.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(ids, ["call-3", "call-4"]);
        assert_eq!(last.earlier_steps, 3);

        let StepDto::ToolCall(oldest) = &last.steps[0] else {
            unreachable!()
        };
        let before = s.fold.steps(&turn, MAIN_LANE, Some(&oldest.id), 2).unwrap();
        assert_eq!(before.steps.len(), 2);
        assert_eq!(before.earlier_steps, 1);
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
}
