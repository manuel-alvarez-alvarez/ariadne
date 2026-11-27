//! The client view of a trajectory: a snapshot, and each update applied to
//! it in its revision order.

use ariadne_api::trajectory::{
    ContentBlockDto, PermissionState, PlanDto, QuestionState, SessionStateDto, StepDto,
    SubagentStepDto, TrajectoryOpDto, TrajectorySnapshotDto, TrajectoryUpdateDto, TurnDto,
    TurnState,
};

/// A session's turns as the daemon holds them, kept current by its updates.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    rev: u64,
    session: SessionStateDto,
    turns: Vec<TurnDto>,
    subagents: Vec<SubagentStepDto>,
    plans: Vec<PlanDto>,
}

/// What one op of an update changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Touched {
    Turn(String),
    /// Text streamed onto a step, or a step gone.
    Step {
        turn_id: String,
        step_id: String,
    },
    /// A step opened or changed, as the update sent it. It names the step
    /// also where the view holds no place for it: a closed turn holds no
    /// steps, and an answer takes a step out of `attention`.
    Item(Box<StepDto>),
    Plan(String),
    Subagent(String),
    Session,
}

/// An update whose revision does not follow the view's. The client missed
/// a revision, and reads a new snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub expected: u64,
    pub got: u64,
}

impl std::fmt::Display for Gap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "expected revision {}, got revision {}",
            self.expected, self.got
        )
    }
}

impl std::error::Error for Gap {}

impl View {
    pub fn new(snapshot: TrajectorySnapshotDto) -> Self {
        Self {
            rev: snapshot.rev,
            session: snapshot.session,
            turns: snapshot.turns,
            subagents: snapshot.subagents,
            plans: snapshot.plans,
        }
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn session(&self) -> &SessionStateDto {
        &self.session
    }

    /// Oldest first.
    pub fn turns(&self) -> &[TurnDto] {
        &self.turns
    }

    pub fn turn(&self, id: &str) -> Option<&TurnDto> {
        self.turns.iter().find(|turn| turn.id == id)
    }

    pub fn subagents(&self) -> &[SubagentStepDto] {
        &self.subagents
    }

    pub fn plans(&self) -> &[PlanDto] {
        &self.plans
    }

    /// Whether the session is gone and no turn runs: nothing more comes.
    pub fn ended(&self) -> bool {
        !self.session.status.is_live() && self.session.running_turn_id.is_none()
    }

    /// The options of the newest permission that waits, if one does.
    pub fn waiting_permission(&self) -> Option<&ariadne_api::trajectory::PermissionStepDto> {
        self.turns
            .iter()
            .rev()
            .flat_map(|turn| turn.attention.iter().chain(turn.steps.iter().rev()))
            .find_map(|step| match step {
                StepDto::Permission(permission) if permission.state == PermissionState::Pending => {
                    Some(permission)
                }
                _ => None,
            })
    }

    /// Apply one update. An update that does not follow the view's revision
    /// changes nothing and is a [`Gap`].
    pub fn apply(&mut self, update: TrajectoryUpdateDto) -> Result<Vec<Touched>, Gap> {
        let mut touched = Vec::new();
        self.apply_each(update, |_, one| touched.push(one))?;
        Ok(touched)
    }

    /// Apply one update an op at a time, and show the view to `each` after
    /// each op. A client that prints a step sees it before a later op of the
    /// same update closes its turn and drops it.
    pub fn apply_each(
        &mut self,
        update: TrajectoryUpdateDto,
        mut each: impl FnMut(&View, Touched),
    ) -> Result<(), Gap> {
        let expected = self.rev + 1;
        if update.rev != expected {
            return Err(Gap {
                expected,
                got: update.rev,
            });
        }
        self.rev = update.rev;
        for op in update.ops {
            if let Some(touched) = self.op(op) {
                each(self, touched);
            }
        }
        Ok(())
    }

    fn op(&mut self, op: TrajectoryOpDto) -> Option<Touched> {
        match op {
            TrajectoryOpDto::TurnUpsert { turn } => Some(self.upsert_turn(turn)),
            TrajectoryOpDto::ItemUpsert { step } => self.upsert_step(step),
            TrajectoryOpDto::TextAppend {
                turn_id,
                item_id,
                text,
            } => {
                let turn = self.turn_mut(&turn_id)?;
                let step = turn
                    .steps
                    .iter_mut()
                    .find(|step| step_id(step) == item_id)?;
                let content = match step {
                    StepDto::Message(message) => &mut message.content,
                    StepDto::Thought(thought) => &mut thought.content,
                    _ => return None,
                };
                append(content, &text);
                Some(Touched::Step {
                    turn_id,
                    step_id: item_id,
                })
            }
            TrajectoryOpDto::ItemRemove { turn_id, item_id } => {
                let turn = self.turn_mut(&turn_id)?;
                turn.steps.retain(|step| step_id(step) != item_id);
                turn.attention.retain(|step| step_id(step) != item_id);
                Some(Touched::Step {
                    turn_id,
                    step_id: item_id,
                })
            }
            TrajectoryOpDto::PlanUpsert { plan } => {
                let id = plan.plan_id.clone();
                match self.plans.iter_mut().find(|held| held.plan_id == id) {
                    Some(held) => *held = plan,
                    None => self.plans.push(plan),
                }
                Some(Touched::Plan(id))
            }
            TrajectoryOpDto::SubagentUpsert { subagent } => {
                let id = subagent.subagent_id.clone();
                match self
                    .subagents
                    .iter_mut()
                    .find(|held| held.subagent_id == id)
                {
                    Some(held) => *held = subagent,
                    None => self.subagents.push(subagent),
                }
                Some(Touched::Subagent(id))
            }
            TrajectoryOpDto::SessionPatch { session } => {
                self.session = session;
                Some(Touched::Session)
            }
        }
    }

    /// A running turn keeps the steps the view holds, since an upsert
    /// carries none. A closed turn drops them.
    fn upsert_turn(&mut self, mut turn: TurnDto) -> Touched {
        let id = turn.id.clone();
        match self.turns.iter_mut().find(|held| held.id == id) {
            Some(held) => {
                if turn.state == TurnState::Running && turn.steps.is_empty() {
                    turn.steps = std::mem::take(&mut held.steps);
                }
                *held = turn;
            }
            None => {
                let at = self
                    .turns
                    .iter()
                    .position(|held| held.id > id)
                    .unwrap_or(self.turns.len());
                self.turns.insert(at, turn);
            }
        }
        Touched::Turn(id)
    }

    /// A step joins the steps of its turn while the turn runs, and replaces
    /// its copy in `attention` whatever the state of the turn. An answer
    /// takes a permission or a question out of `attention`.
    fn upsert_step(&mut self, step: StepDto) -> Option<Touched> {
        let id = step_id(&step).to_string();
        let Some(turn) = self.turn_mut(step_turn(&step)) else {
            return Some(Touched::Item(Box::new(step)));
        };
        let touched = Touched::Item(Box::new(step.clone()));
        if turn.state == TurnState::Running {
            match turn.steps.iter_mut().find(|held| step_id(held) == id) {
                Some(held) => *held = step.clone(),
                None => turn.steps.push(step.clone()),
            }
        }
        let waits = waits(&step);
        match turn.attention.iter().position(|held| step_id(held) == id) {
            Some(at) if waits || matches!(step, StepDto::Error(_)) => turn.attention[at] = step,
            Some(at) => {
                turn.attention.remove(at);
            }
            None if waits => turn.attention.insert(0, step),
            None => {}
        }
        Some(touched)
    }

    fn turn_mut(&mut self, id: &str) -> Option<&mut TurnDto> {
        self.turns.iter_mut().find(|turn| turn.id == id)
    }
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

/// Streamed text joins the last text block, as the daemon joins adjacent
/// text blocks (040, rule 28).
fn append(content: &mut Vec<ContentBlockDto>, more: &str) {
    match content.last_mut() {
        Some(ContentBlockDto::Text { text }) => text.push_str(more),
        _ => content.push(ContentBlockDto::Text {
            text: more.to_string(),
        }),
    }
}

/// The id of a step: the stored event that opened it.
pub fn step_id(step: &StepDto) -> &str {
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

/// The turn a step belongs to.
pub fn step_turn(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.turn_id,
        StepDto::Thought(step) => &step.turn_id,
        StepDto::ToolCall(step) => &step.turn_id,
        StepDto::Permission(step) => &step.turn_id,
        StepDto::Question(step) => &step.turn_id,
        StepDto::Note(step) => &step.turn_id,
        StepDto::Error(step) => &step.turn_id,
        StepDto::Subagent(step) => &step.turn_id,
        StepDto::Compaction(step) => &step.turn_id,
    }
}

/// When a step opened.
pub fn step_at(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.created_at,
        StepDto::Thought(step) => &step.created_at,
        StepDto::ToolCall(step) => &step.created_at,
        StepDto::Permission(step) => &step.created_at,
        StepDto::Question(step) => &step.created_at,
        StepDto::Note(step) => &step.created_at,
        StepDto::Error(step) => &step.created_at,
        StepDto::Subagent(step) => &step.created_at,
        StepDto::Compaction(step) => &step.created_at,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::trajectory::fixtures::{snapshot, step, turn, update};

    fn texts(view: &View, turn_id: &str) -> Vec<String> {
        view.turn(turn_id)
            .unwrap()
            .steps
            .iter()
            .map(|step| match step {
                StepDto::Message(message) => crate::trajectory::text(&message.content),
                other => step_id(other).to_string(),
            })
            .collect()
    }

    #[test]
    fn a_view_applies_each_op_of_an_update_to_its_snapshot() {
        let mut view = View::new(snapshot(
            4,
            vec![turn(
                "t1",
                "running",
                json!({"steps": [
                    step("message", "m1", "t1", json!({"content": [{"type": "text", "text": "Half "}], "streaming": true})),
                    step("note", "n1", "t1", json!({"text": "started"})),
                ]}),
            )],
        ));

        let touched = view
            .apply(update(
                5,
                json!([
                    {"op": "text_append", "turn_id": "t1", "item_id": "m1", "text": "done."},
                    {"op": "item_upsert", "step": step("tool_call", "c1", "t1", json!({"status": "running"}))},
                    {"op": "item_remove", "turn_id": "t1", "item_id": "n1"},
                    {"op": "plan_upsert", "plan": {"plan_id": "p1", "variant": "entries",
                        "entries": [{"content": "Test", "status": "pending", "priority": null}],
                        "markdown": null, "path": null, "turn_id": "t1", "removed": false,
                        "updated_at": "2026-11-27T10:00:00Z"}},
                    {"op": "session_patch", "session": snapshot(0, vec![]).session},
                ]),
            ))
            .unwrap();

        assert_eq!(view.rev(), 5);
        assert_eq!(texts(&view, "t1"), ["Half done.", "c1"]);
        assert_eq!(view.plans().len(), 1);
        assert_eq!(touched.len(), 5, "{touched:?}");
    }

    #[test]
    fn an_update_that_skips_a_revision_is_a_gap_and_changes_nothing() {
        let mut view = View::new(snapshot(4, vec![turn("t1", "running", json!({}))]));
        let before = view.clone();

        let gap = view
            .apply(update(
                6,
                json!([{"op": "item_upsert", "step": step("note", "n1", "t1", json!({}))}]),
            ))
            .unwrap_err();

        assert_eq!(
            gap,
            Gap {
                expected: 5,
                got: 6
            }
        );
        assert_eq!(view, before);
    }

    #[test]
    fn a_turn_upsert_keeps_the_steps_of_a_running_turn_and_drops_those_of_a_closed_one() {
        let mut view = View::new(snapshot(
            0,
            vec![turn(
                "t1",
                "running",
                json!({"steps": [step("note", "n1", "t1", json!({}))]}),
            )],
        ));

        view.apply(update(
            1,
            json!([{"op": "turn_upsert", "turn": turn("t1", "running", json!({"earlier_steps": 0}))}]),
        ))
        .unwrap();
        assert_eq!(texts(&view, "t1"), ["n1"]);

        view.apply(update(
            2,
            json!([
                {"op": "turn_upsert", "turn": turn("t1", "completed", json!({"earlier_steps": 1}))},
                {"op": "turn_upsert", "turn": turn("t2", "running", json!({}))},
            ]),
        ))
        .unwrap();
        assert!(texts(&view, "t1").is_empty());
        let ids: Vec<_> = view.turns().iter().map(|turn| turn.id.as_str()).collect();
        assert_eq!(ids, ["t1", "t2"]);
    }

    #[test]
    fn apply_each_shows_a_step_before_a_later_op_closes_its_turn() {
        let mut view = View::new(snapshot(0, vec![turn("t1", "running", json!({}))]));
        let mut seen = Vec::new();

        view.apply_each(
            update(
                1,
                json!([
                    {"op": "item_upsert", "step": step("note", "n1", "t1", json!({}))},
                    {"op": "turn_upsert", "turn": turn("t1", "completed", json!({}))},
                ]),
            ),
            |view, _| seen.push(texts(view, "t1")),
        )
        .unwrap();

        assert_eq!(seen, [vec!["n1".to_string()], vec![]]);
    }

    #[test]
    fn an_answer_on_a_closed_turn_names_the_step_it_takes_out_of_attention() {
        let mut view = View::new(snapshot(
            0,
            vec![turn(
                "t1",
                "completed",
                json!({"attention": [step("question", "q1", "t1", json!({}))]}),
            )],
        ));
        let answered = step(
            "question",
            "q1",
            "t1",
            json!({"state": "answered", "answer": "base: next"}),
        );

        let touched = view
            .apply(update(
                1,
                json!([{"op": "item_upsert", "step": answered.clone()}]),
            ))
            .unwrap();

        assert!(view.turn("t1").unwrap().attention.is_empty());
        assert_eq!(
            touched,
            [Touched::Item(Box::new(
                serde_json::from_value(answered).unwrap()
            ))]
        );
    }

    #[test]
    fn an_answer_takes_a_permission_out_of_attention() {
        let pending = step("permission", "p1", "t1", json!({"state": "pending"}));
        let mut view = View::new(snapshot(
            0,
            vec![turn(
                "t1",
                "running",
                json!({"attention": [pending.clone()], "steps": [pending]}),
            )],
        ));
        assert_eq!(view.waiting_permission().unwrap().id, "p1");

        view.apply(update(
            1,
            json!([{"op": "item_upsert", "step": step("permission", "p1", "t1", json!({
                "state": "answered",
                "answer": {"option_id": "yes", "label": "Allow", "decided_by": "console"}
            }))}]),
        ))
        .unwrap();

        assert!(view.turn("t1").unwrap().attention.is_empty());
        assert!(view.waiting_permission().is_none());
    }
}
