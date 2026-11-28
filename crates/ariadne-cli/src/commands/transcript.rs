//! What `ariadne session logs`, `ariadne task logs` and a piped `ariadne
//! attach` print: the typed turns of a session's trajectory (040).
//!
//! The daemon folds the turns. [`ariadne_console::trajectory`] holds them
//! on this side and writes them as text. This module chooses which turns
//! and steps to show, reads them, and keeps a follow from printing an item
//! twice.

use std::collections::{HashMap, HashSet};
use std::fmt::Display;

use anyhow::Result;
use ariadne_api::trajectory::{StepDto, TrajectoryQuery, TrajectoryStepsQuery, TurnDto, TurnState};
use ariadne_client::Client;
use ariadne_console::Since;
use ariadne_console::trajectory::{
    Printer, Touched, View, kind, settled, step_at, step_id, step_turn,
};
use chrono::TimeZone;

/// The kinds of step `--kind` takes.
pub(crate) const STEP_KINDS: [&str; 9] = [
    "message",
    "thought",
    "tool_call",
    "permission",
    "question",
    "note",
    "error",
    "subagent",
    "compaction",
];

/// The most turns, and steps, one read of the daemon answers (040, rules 41
/// and 44).
pub(crate) const TURN_PAGE: u32 = 50;
const STEP_PAGE: u32 = 200;

/// Which turns and steps a log shows.
#[derive(Debug, Clone, Default)]
pub(crate) struct Filters {
    /// The last N turns.
    pub(crate) tail: Option<usize>,
    /// The turns and steps at or after an instant.
    pub(crate) since: Option<Since>,
    /// The steps of these kinds. Empty is every kind.
    pub(crate) kinds: Vec<String>,
}

impl Filters {
    pub(crate) fn keeps_step(&self, step: &StepDto) -> bool {
        (self.kinds.is_empty() || self.kinds.iter().any(|wanted| wanted == kind(step)))
            && self
                .since
                .as_ref()
                .is_none_or(|since| since.includes(step_at(step)))
    }

    /// A turn is at or after `since` when it started, or ended, at or after
    /// it, or still runs.
    fn keeps_turn(&self, turn: &TurnDto) -> bool {
        self.since.as_ref().is_none_or(|since| {
            turn.state == TurnState::Running
                || since.includes(&turn.started_at)
                || turn
                    .ended_at
                    .as_deref()
                    .is_some_and(|ended| since.includes(ended))
        })
    }

    /// The turns `since` keeps, and then the last `tail` of them.
    fn turns(&self, mut turns: Vec<TurnDto>) -> Vec<TurnDto> {
        turns.retain(|turn| self.keeps_turn(turn));
        if let Some(tail) = self.tail {
            turns.drain(..turns.len().saturating_sub(tail));
        }
        turns
    }

    /// Whether an older page can hold a turn these filters keep.
    pub(crate) fn needs_older(&self, turns: &[TurnDto]) -> bool {
        let short = self
            .tail
            .is_none_or(|tail| turns.iter().filter(|turn| self.keeps_turn(turn)).count() < tail);
        let within = match (&self.since, turns.first()) {
            (Some(since), Some(oldest)) => since.includes(&oldest.started_at),
            _ => true,
        };
        short && within
    }
}

/// A turn as `--format json` prints it: `steps` are the steps given, all
/// of them, or none, and `earlier_steps` counts the rest. `attention` keeps
/// the steps the filters keep.
pub(crate) fn json_turn(turn: &TurnDto, steps: Option<Vec<StepDto>>, filters: &Filters) -> TurnDto {
    let mut turn = turn.clone();
    turn.attention.retain(|step| filters.keeps_step(step));
    match steps {
        Some(steps) => {
            turn.steps = steps;
            turn.earlier_steps = 0;
        }
        None => {
            turn.earlier_steps += turn.steps.len() as u64;
            turn.steps.clear();
        }
    }
    turn
}

/// The turns a log shows, oldest first, each with its steps when `steps`
/// asks for them. Older pages are read as far as the filters reach.
pub(crate) async fn read(
    client: &Client,
    session_id: &str,
    filters: &Filters,
    steps: bool,
) -> Result<Vec<(TurnDto, Option<Vec<StepDto>>)>> {
    let newest = client
        .trajectory(
            session_id,
            &TrajectoryQuery {
                limit: Some(TURN_PAGE),
                ..TrajectoryQuery::default()
            },
        )
        .await?;
    let mut turns = older(
        client,
        session_id,
        &newest.turns,
        newest.has_older,
        |turns: &[TurnDto]| filters.needs_older(turns),
    )
    .await?;
    turns.extend(newest.turns);
    let mut out = Vec::new();
    for mut turn in filters.turns(turns) {
        turn.attention.retain(|step| filters.keeps_step(step));
        let steps = match steps {
            true => Some(
                all_steps(client, session_id, &turn.id)
                    .await?
                    .into_iter()
                    .filter(|step| filters.keeps_step(step))
                    .collect(),
            ),
            false => None,
        };
        out.push((turn, steps));
    }
    Ok(out)
}

/// The turns older than `newer`, oldest first, a page at a time, for as
/// long as `wants` asks for more of the turns read so far, these and
/// `newer` together.
pub(crate) async fn older(
    client: &Client,
    session_id: &str,
    newer: &[TurnDto],
    mut has_older: bool,
    wants: impl Fn(&[TurnDto]) -> bool,
) -> Result<Vec<TurnDto>> {
    let mut turns: Vec<TurnDto> = Vec::new();
    while has_older {
        let read: Vec<TurnDto> = turns.iter().chain(newer).cloned().collect();
        if !wants(&read) {
            break;
        }
        let Some(oldest) = read.first() else { break };
        let page = client
            .trajectory(
                session_id,
                &TrajectoryQuery {
                    before: Some(oldest.id.clone()),
                    limit: Some(TURN_PAGE),
                    subagent: None,
                },
            )
            .await?;
        if page.turns.is_empty() {
            break;
        }
        has_older = page.has_older;
        turns.splice(0..0, page.turns);
    }
    Ok(turns)
}

/// Every step of a turn, oldest first, a page at a time.
pub(crate) async fn all_steps(
    client: &Client,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<StepDto>> {
    all_steps_of(client, session_id, turn_id, None).await
}

/// Every step of a turn in the lane of `subagent`, or in the session's own
/// lane where it names none, oldest first, a page at a time.
pub(crate) async fn all_steps_of(
    client: &Client,
    session_id: &str,
    turn_id: &str,
    subagent: Option<&str>,
) -> Result<Vec<StepDto>> {
    let mut steps = Vec::new();
    let mut before = None;
    loop {
        let page = client
            .trajectory_steps(
                session_id,
                turn_id,
                &TrajectoryStepsQuery {
                    before,
                    limit: Some(STEP_PAGE),
                    subagent: subagent.map(str::to_string),
                },
            )
            .await?;
        let Some(first) = page.steps.first() else {
            break;
        };
        before = Some(step_id(first).to_string());
        steps.splice(0..0, page.steps);
        if page.earlier_steps == 0 {
            break;
        }
    }
    Ok(steps)
}

/// How a follow writes what it prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// The block form of `session logs`.
    Blocks,
    /// One `kind · summary` line for each item, for a piped `attach`.
    Lines,
    /// One typed turn in each line when it closes, with its steps when the
    /// follow prints steps.
    Json,
}

/// Prints each item of a followed trajectory one time, across every
/// snapshot a reconnect brings.
///
/// A turn prints its prompt when it opens and its activity and answer when
/// it closes. A step prints when it is settled, and a permission or a
/// question prints again, one line, when it closes.
pub(crate) struct Follower<Tz> {
    layout: Layout,
    steps: bool,
    filters: Filters,
    printer: Printer<Tz>,
    admitted: bool,
    skipped: HashSet<String>,
    opened: HashSet<String>,
    /// The turns that ran when they opened here: their steps printed live.
    live: HashSet<String>,
    closed: HashSet<String>,
    printed: HashSet<String>,
    answered: HashSet<String>,
    /// The settled steps of each open turn, for the JSON line of its close.
    held: HashMap<String, Vec<StepDto>>,
    /// The permissions and questions printed while they waited, by id, with
    /// their turn: a snapshot reads their turn's stored steps, so a close
    /// that came while the stream was down prints too.
    waiting: HashMap<String, String>,
    /// The line that names each opened turn, over a close printed after it.
    owners: HashMap<String, String>,
    /// The steps an update sent before the header of their turn: they print
    /// under the turn when it comes, open or closed.
    orphans: HashMap<String, Vec<StepDto>>,
}

impl<Tz> Follower<Tz>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    pub(crate) fn new(layout: Layout, steps: bool, filters: Filters, printer: Printer<Tz>) -> Self {
        Self {
            layout,
            // The line form prints every item.
            steps: steps || layout == Layout::Lines,
            filters,
            printer,
            admitted: false,
            skipped: HashSet::new(),
            opened: HashSet::new(),
            live: HashSet::new(),
            closed: HashSet::new(),
            printed: HashSet::new(),
            answered: HashSet::new(),
            held: HashMap::new(),
            waiting: HashMap::new(),
            owners: HashMap::new(),
            orphans: HashMap::new(),
        }
    }

    /// Whether older turns than `turns`, the oldest first, can hold what
    /// this follow prints. Before the first snapshot that is what `--tail`
    /// and `--since` keep. After it, a turn older than every turn this
    /// follow has seen: one that opened and closed while the stream was
    /// down.
    pub(crate) fn wants_older(&self, turns: &[TurnDto]) -> bool {
        if !self.admitted {
            return self.filters.needs_older(turns);
        }
        turns.first().is_some_and(|oldest| {
            !self.opened.contains(&oldest.id) && !self.skipped.contains(&oldest.id)
        })
    }

    /// Take a snapshot in, with the `older` turns read before it. The first
    /// one chooses the turns `--tail` and `--since` keep. Answers the turns
    /// whose stored steps the caller reads for [`Follower::snapshot`]: those
    /// not yet closed here, where the follow prints steps in the block or
    /// JSON form.
    pub(crate) fn admit(&mut self, older: &[TurnDto], view: &View) -> Vec<String> {
        let turns: Vec<&TurnDto> = older.iter().chain(view.turns()).collect();
        if !self.admitted {
            self.admitted = true;
            let kept: HashSet<_> = self
                .filters
                .turns(turns.iter().map(|turn| (*turn).clone()).collect())
                .into_iter()
                .map(|turn| turn.id)
                .collect();
            self.skipped = turns
                .iter()
                .filter(|turn| !kept.contains(&turn.id))
                .map(|turn| turn.id.clone())
                .collect();
        }
        let mut wanted: Vec<String> = Vec::new();
        if self.steps && self.layout != Layout::Lines {
            wanted.extend(
                turns
                    .iter()
                    .filter(|turn| {
                        !self.skipped.contains(&turn.id) && !self.closed.contains(&turn.id)
                    })
                    .map(|turn| turn.id.clone()),
            );
        }
        for turn_id in self.waiting.values() {
            if !wanted.contains(turn_id) {
                wanted.push(turn_id.clone());
            }
        }
        wanted
    }

    /// What a snapshot adds: the `older` turns read before it and every turn
    /// of it, with the stored steps read for each, of which each item prints
    /// once.
    pub(crate) fn snapshot(
        &mut self,
        older: &[TurnDto],
        view: &View,
        stored: &HashMap<String, Vec<StepDto>>,
    ) -> String {
        let mut out = String::new();
        let mut swept = HashSet::new();
        for turn in older.iter().chain(view.turns()) {
            swept.insert(turn.id.as_str());
            let stored = stored.get(&turn.id).map(Vec::as_slice).unwrap_or_default();
            out.push_str(&self.sweep(turn, stored));
        }
        // A turn the snapshot does not hold can still close a waiting step.
        for (turn_id, steps) in stored {
            if swept.contains(turn_id.as_str()) {
                continue;
            }
            for step in steps {
                if self.waiting.contains_key(step_id(step)) {
                    out.push_str(&self.step(step));
                }
            }
        }
        out
    }

    /// What an applied update adds, for the turns it touched. A step an
    /// update sent joins the sweep of its turn, so the close of a step that
    /// the view no longer holds, on a closed turn, prints too.
    pub(crate) fn update(&mut self, view: &View, touched: &[Touched]) -> String {
        let mut turns: Vec<(&str, Vec<StepDto>)> = Vec::new();
        for touched in touched {
            let (id, step) = match touched {
                Touched::Turn(id) => (id.as_str(), None),
                Touched::Step { turn_id, .. } => (turn_id.as_str(), None),
                Touched::Item(step) => (step_turn(step), Some(step.as_ref().clone())),
                _ => continue,
            };
            match turns.iter_mut().find(|(held, _)| *held == id) {
                Some((_, steps)) => steps.extend(step),
                None => turns.push((id, step.into_iter().collect())),
            }
        }
        let mut out = String::new();
        for (id, steps) in turns {
            match view.turn(id) {
                Some(turn) => out.push_str(&self.sweep(turn, &steps)),
                // A turn the view does not hold yet, or no longer: a step of
                // it waits for its header, and it can close a waiting step.
                None => {
                    let orphans = self.orphans.entry(id.to_string()).or_default();
                    for step in &steps {
                        match orphans
                            .iter_mut()
                            .find(|held| step_id(held) == step_id(step))
                        {
                            Some(held) => *held = step.clone(),
                            None => orphans.push(step.clone()),
                        }
                    }
                    for step in &steps {
                        if self.waiting.contains_key(step_id(step)) {
                            out.push_str(&self.step(step));
                        }
                    }
                }
            }
        }
        out
    }

    fn sweep(&mut self, turn: &TurnDto, stored: &[StepDto]) -> String {
        if self.skipped.contains(&turn.id) {
            return String::new();
        }
        let orphans = self.orphans.remove(&turn.id).unwrap_or_default();
        let stored: Vec<StepDto> = orphans.into_iter().chain(stored.iter().cloned()).collect();
        let stored = stored.as_slice();
        let mut out = String::new();
        if self.opened.insert(turn.id.clone()) {
            self.owners
                .insert(turn.id.clone(), self.printer.owner(turn));
            if turn.state == TurnState::Running {
                self.live.insert(turn.id.clone());
            }
            out.push_str(&match self.layout {
                Layout::Blocks => self.printer.opened(turn),
                Layout::Lines => self.printer.opened_line(turn),
                Layout::Json => String::new(),
            });
        }
        let mut attention: Vec<&StepDto> = turn.attention.iter().collect();
        attention.sort_by(|a, b| step_at(a).cmp(step_at(b)));
        let shown: Vec<&StepDto> = match self.steps {
            true => stored.iter().chain(&turn.steps).chain(attention).collect(),
            // The stored steps add the close of a step printed while it
            // waited, and nothing else.
            false => attention
                .into_iter()
                .chain(
                    stored
                        .iter()
                        .filter(|step| self.waiting.contains_key(step_id(step))),
                )
                .collect(),
        };
        for step in shown {
            if step_turn(step) == turn.id && self.filters.keeps_step(step) {
                out.push_str(&self.step(step));
            }
        }
        if turn.state != TurnState::Running && self.closed.insert(turn.id.clone()) {
            out.push_str(&match self.layout {
                Layout::Blocks => self.printer.closed(turn, self.steps),
                Layout::Lines => self
                    .printer
                    .closed_line(turn, !self.live.contains(&turn.id)),
                Layout::Json => {
                    let steps = self
                        .steps
                        .then(|| self.held.remove(&turn.id).unwrap_or_default());
                    json_line(&json_turn(turn, steps, &self.filters))
                }
            });
        }
        out
    }

    fn step(&mut self, step: &StepDto) -> String {
        if !settled(step) {
            return String::new();
        }
        let id = step_id(step).to_string();
        if self.layout == Layout::Json {
            // A JSON follow prints a step in the line of its turn: it keeps
            // the newest form of each settled step until the turn closes.
            if self.steps {
                let held = self.held.entry(step_turn(step).to_string()).or_default();
                match held.iter_mut().find(|held| step_id(held) == id) {
                    Some(held) => *held = step.clone(),
                    None => held.push(step.clone()),
                }
            }
            return String::new();
        }
        let closing = self.printer.answered(step).is_some();
        let asks = matches!(step, StepDto::Permission(_) | StepDto::Question(_));
        if closing {
            self.waiting.remove(&id);
        }
        if self.printed.insert(id.clone()) {
            if closing {
                self.answered.insert(id);
            } else if asks {
                self.waiting.insert(id, step_turn(step).to_string());
            }
            return match self.layout {
                Layout::Blocks => self.printer.step(step),
                Layout::Lines => self.printer.step_line(step),
                Layout::Json => String::new(),
            };
        }
        if closing && self.answered.insert(id) {
            return match self.layout {
                // A close after its turn closed prints whole: under a later
                // turn, an answer line alone would not say what it answers.
                Layout::Blocks if self.closed.contains(step_turn(step)) => {
                    let owner = self
                        .owners
                        .get(step_turn(step))
                        .cloned()
                        .unwrap_or_else(|| format!("  in the turn {}:\n", step_turn(step)));
                    format!("{owner}{}", self.printer.step(step))
                }
                Layout::Blocks => self.printer.answered(step).unwrap_or_default(),
                Layout::Lines => self.printer.answered_line(step).unwrap_or_default(),
                Layout::Json => String::new(),
            };
        }
        String::new()
    }
}

fn json_line(value: &impl serde::Serialize) -> String {
    // A DTO of strings, numbers and lists always serializes.
    format!("{}\n", serde_json::to_string(value).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use ariadne_api::trajectory::TrajectorySnapshotDto;
    use chrono::Utc;
    use serde_json::{Value, json};

    use super::*;

    fn turn(id: &str, state: &str, started_at: &str, extra: Value) -> Value {
        let mut turn = json!({
            "id": id, "lane": "main",
            "prompt": {"id": id, "source": "console", "prompt_kind": null,
                       "content": [{"type": "text", "text": format!("prompt {id}")}]},
            "state": state, "stop_reason": null, "started_at": started_at,
            "ended_at": null,
            "activity": {"steps": 0, "tool_calls": 0, "files_changed": 0, "failed": 0,
                         "cancelled": 0, "thoughts": 0, "subagents": 0},
            "steps": [], "earlier_steps": 0, "attention": [], "attention_more": 0, "notices": [], "notices_more": 0, "answer": []
        });
        turn.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        turn
    }

    fn note(id: &str, turn_id: &str, created_at: &str) -> Value {
        json!({"type": "note", "id": id, "turn_id": turn_id, "lane": "main",
               "created_at": created_at, "origin": "session_start", "severity": "info",
               "text": format!("note {id}")})
    }

    fn view(turns: Vec<Value>) -> View {
        let snapshot: TrajectorySnapshotDto = serde_json::from_value(json!({
            "rev": 0,
            "session": {"status": "running", "attention_reason": null, "running_turn_id": null,
                        "title": null, "mode": null, "config_options": [], "context": null,
                        "usage": {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0},
                        "cost": null, "commands": []},
            "turns": turns, "has_older": false, "subagents": [], "plans": []
        }))
        .unwrap();
        View::new(snapshot)
    }

    fn typed<T: serde::de::DeserializeOwned>(value: Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn tail_keeps_the_last_turns_and_since_the_turns_at_or_after_it() {
        let turns: Vec<TurnDto> = vec![
            typed(turn(
                "t1",
                "completed",
                "2026-11-27T08:00:00Z",
                json!({"ended_at": "2026-11-27T08:10:00Z"}),
            )),
            typed(turn(
                "t2",
                "completed",
                "2026-11-27T09:00:00Z",
                json!({"ended_at": "2026-11-27T10:30:00Z"}),
            )),
            typed(turn(
                "t3",
                "completed",
                "2026-11-27T11:00:00Z",
                json!({"ended_at": "2026-11-27T11:10:00Z"}),
            )),
        ];
        let ids = |turns: Vec<TurnDto>| turns.into_iter().map(|turn| turn.id).collect::<Vec<_>>();

        let tail = Filters {
            tail: Some(2),
            ..Filters::default()
        };
        assert_eq!(ids(tail.turns(turns.clone())), ["t2", "t3"]);

        let since = Filters {
            since: Some("2026-11-27T10:00:00Z".parse().unwrap()),
            ..Filters::default()
        };
        assert_eq!(
            ids(since.turns(turns.clone())),
            ["t2", "t3"],
            "t2 ended after the instant"
        );
        assert!(!since.needs_older(&turns), "t1 started before the instant");
        assert!(tail.needs_older(&turns[2..]), "one turn is short of two");
    }

    #[test]
    fn kind_keeps_the_steps_of_the_kinds_it_names() {
        let filters = Filters {
            kinds: vec!["error".into()],
            ..Filters::default()
        };
        let error: StepDto = typed(
            json!({"type": "error", "id": "e", "turn_id": "t", "lane": "main",
            "created_at": "2026-11-27T10:00:00Z", "text": "boom", "diagnosis": null}),
        );
        let note: StepDto = typed(note("n", "t", "2026-11-27T10:00:00Z"));

        assert!(filters.keeps_step(&error));
        assert!(!filters.keeps_step(&note));
    }

    #[test]
    fn a_follow_prints_each_item_once_across_snapshots() {
        let mut follower =
            Follower::new(Layout::Lines, false, Filters::default(), Printer::new(Utc));
        let first = view(vec![turn(
            "t1",
            "running",
            "2026-11-27T10:00:00Z",
            json!({"steps": [note("n1", "t1", "2026-11-27T10:00:01Z")]}),
        )]);
        follower.admit(&[], &first);
        let before = follower.snapshot(&[], &first, &HashMap::new());

        let mut again = view(vec![turn(
            "t1",
            "running",
            "2026-11-27T10:00:00Z",
            json!({"steps": [note("n1", "t1", "2026-11-27T10:00:01Z"),
                             note("n2", "t1", "2026-11-27T10:00:02Z")]}),
        )]);
        follower.admit(&[], &again);
        let after = follower.snapshot(&[], &again, &HashMap::new());
        let touched = again
            .apply(typed(json!({"rev": 1, "ops": [{"op": "turn_upsert", "turn": turn(
                "t1", "completed", "2026-11-27T10:00:00Z", json!({"ended_at": "2026-11-27T10:01:00Z"})
            )}]})))
            .unwrap();
        let closed = follower.update(&again, &touched);

        assert_eq!(before, "prompt · prompt t1\nnote · note n1\n");
        assert_eq!(after, "note · note n2\n");
        assert_eq!(closed, "turn · completed\n");
    }

    #[test]
    fn a_follow_leaves_out_the_turns_tail_does_not_keep() {
        let filters = Filters {
            tail: Some(1),
            ..Filters::default()
        };
        let mut follower = Follower::new(Layout::Blocks, false, filters, Printer::new(Utc));
        let snapshot = view(vec![
            turn("t1", "completed", "2026-11-27T09:00:00Z", json!({})),
            turn("t2", "running", "2026-11-27T10:00:00Z", json!({})),
        ]);

        follower.admit(&[], &snapshot);
        let out = follower.snapshot(&[], &snapshot, &HashMap::new());

        assert_eq!(out, "10:00:00  USER  prompt t2\n");
    }

    #[test]
    fn a_json_turn_carries_its_steps_or_counts_them() {
        let turn: TurnDto = typed(turn(
            "t1",
            "running",
            "2026-11-27T10:00:00Z",
            json!({"steps": [note("n1", "t1", "2026-11-27T10:00:01Z")], "earlier_steps": 4}),
        ));

        let folded = json_turn(&turn, None, &Filters::default());
        assert!(folded.steps.is_empty());
        assert_eq!(folded.earlier_steps, 5);

        let whole = json_turn(&turn, Some(turn.steps.clone()), &Filters::default());
        assert_eq!(whole.steps.len(), 1);
        assert_eq!(whole.earlier_steps, 0);
    }

    #[test]
    fn a_step_before_a_header_that_comes_closed_prints_in_its_turn() {
        let mut follower = Follower::new(Layout::Json, true, Filters::default(), Printer::new(Utc));
        let mut view = view(vec![]);
        follower.admit(&[], &view);
        let mut out = String::new();

        view.apply_each(
            typed(json!({"rev": 1, "ops": [
                {"op": "item_upsert", "step": note("n0", "t0", "2026-11-27T10:00:00Z")},
                {"op": "turn_upsert", "turn": turn("t0", "interrupted", "2026-11-27T10:00:00Z", json!({}))},
            ]})),
            |view, touched| out.push_str(&follower.update(view, std::slice::from_ref(&touched))),
        )
        .unwrap();

        let turn: TurnDto = serde_json::from_str(out.trim_end()).unwrap();
        assert_eq!(turn.steps.len(), 1, "{out}");
        assert_eq!(step_id(&turn.steps[0]), "n0");
    }

    #[test]
    fn a_diagnosis_that_comes_after_a_printed_error_prints_once() {
        for layout in [Layout::Blocks, Layout::Lines] {
            let error = |diagnosis: Value| {
                json!({"type": "error", "id": "e", "turn_id": "t1", "lane": "main",
                       "created_at": "2026-11-27T10:00:01Z", "text": "boom",
                       "diagnosis": diagnosis})
            };
            let mut follower = Follower::new(layout, true, Filters::default(), Printer::new(Utc));
            let mut view = view(vec![turn(
                "t1",
                "running",
                "2026-11-27T10:00:00Z",
                json!({"steps": [error(Value::Null)]}),
            )]);
            follower.admit(&[], &view);
            let first = follower.snapshot(&[], &view, &HashMap::new());
            assert!(first.contains("boom"), "{first}");
            assert!(!first.contains("quota exhausted"), "{first}");

            let mut out = String::new();
            for rev in 1..=2 {
                view.apply_each(
                    typed(json!({"rev": rev, "ops": [
                        {"op": "item_upsert", "step": error(json!("quota exhausted"))},
                    ]})),
                    |view, touched| {
                        out.push_str(&follower.update(view, std::slice::from_ref(&touched)))
                    },
                )
                .unwrap();
            }
            assert_eq!(out.matches("quota exhausted").count(), 1, "{out}");
        }
    }
}
