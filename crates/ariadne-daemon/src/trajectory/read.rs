//! Which stored events a fold opens on (040).
//!
//! A session that ran for a day holds tens of thousands of events, and a
//! tool end averages ten kilobytes. So a read walks the heads of the events
//! it needs — their kind, lane and facts, and no payload — and folds them
//! once to learn what each turn holds. Then it picks the few events whose
//! payloads a client sees — the prompts, the answers, what waits on a person,
//! the last steps of the running turn, the subagents and the plans — and
//! loads at most the budget of them. Every query of one read stops at the
//! one newest id the read began with.

use std::collections::{BTreeMap, BTreeSet};

use ariadne_api::events::AgentEventDto;
use ariadne_api::trajectory::{MAIN_LANE, SessionStateDto};
use ariadne_core::SessionStatus;
use ariadne_core::event_facts::EventFacts;
use ariadne_store::{AgentEvent, EventHead, Store, StoreError};

use super::fold::{Fold, STEP_WINDOW, TurnIndex};

/// The most full event payloads one read loads: an open of a trajectory, a
/// page of older turns, or a page of a turn's steps. Five hundred events
/// are about five megabytes at the ten-kilobyte average of a tool end,
/// which a client opens on at once.
pub(crate) const READ_EVENTS: usize = 500;

/// How many turns with an error older than the ones a read shows it holds,
/// for a diagnosis that may still name the error. Twenty keeps a day of
/// failed turns, and keeps the scan of their heads short. A turn with a
/// permission or a question that waits is held whatever its age.
const ERROR_TURNS: usize = 20;

/// The event kind that opens a turn.
const PROMPT: &str = "user_prompt_submit";

/// The kinds a read pairs over the whole session: a reply answers the
/// newest pending permission, and a resolution closes its question, wherever
/// in the session they are.
const PAIR_KINDS: &[&str] = &[
    "permission_request",
    "permission.replied",
    "question",
    "question.resolved",
    "agent_request",
    "agent_request.resolved",
    "session.error",
];

/// One input of a fold, in id order.
#[derive(Debug)]
pub(crate) enum Item {
    /// An event the read loaded whole.
    Full(Box<AgentEvent>),
    /// An event the read loaded whole that comes before every turn it holds,
    /// with the turn it belongs to.
    Seed(Box<AgentEvent>, String),
    /// An event of a turn the fold holds, read without its payload.
    Head(EventHead),
    /// A permission or a question outside every turn the fold holds.
    Pair(EventHead),
}

impl Item {
    fn id(&self) -> &str {
        match self {
            Self::Full(event) | Self::Seed(event, _) => &event.id,
            Self::Head(head) | Self::Pair(head) => &head.id,
        }
    }
}

/// What one read took.
#[derive(Debug, Default)]
pub(crate) struct Read {
    /// Oldest first.
    pub(crate) items: Vec<Item>,
    /// The newest stored id the read stopped at.
    pub(crate) upper: Option<String>,
    /// The oldest turn the read shows: the turns before it are outside.
    pub(crate) first_turn: Option<String>,
    /// Whether turns older than `first_turn` exist.
    pub(crate) has_older: bool,
    /// The prompt after the read, which a page folds to close its last turn
    /// and then drops.
    pub(crate) successor: Option<String>,
}

impl Read {
    /// How many full payloads the read loaded.
    #[cfg(test)]
    pub(crate) fn loaded(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, Item::Full(_) | Item::Seed(..)))
            .count()
    }

    /// Fold the read, and the live events that joined it, into a fold that
    /// has seen nothing yet.
    pub(crate) fn fold_into(self, fold: &mut Fold, live: Vec<AgentEventDto>) {
        let mut live = live.into_iter().peekable();
        for item in self.items {
            while let Some(event) = live.next_if(|event| event.id.as_str() < item.id()) {
                fold.apply(&event, false);
            }
            match item {
                Item::Full(event) => {
                    fold.apply(&crate::http::convert::event_dto(*event), true);
                }
                Item::Seed(event, turn) => {
                    fold.apply_in(&turn, &crate::http::convert::event_dto(*event));
                }
                Item::Head(head) => fold.apply_head(&head),
                Item::Pair(head) => fold.pair_head(&head),
            }
        }
        for event in live {
            fold.apply(&event, false);
        }
        if let Some(first) = &self.first_turn {
            fold.retire_before(first);
        }
        if let Some(successor) = &self.successor {
            fold.drop_turn(successor);
        }
        fold.set_has_older(self.has_older);
    }
}

/// A fold that learns what turns hold, from heads alone.
fn scratch(session_id: &str) -> Fold {
    Fold::new(
        session_id,
        SessionStateDto {
            status: SessionStatus::Running,
            attention_reason: None,
            running_turn_id: None,
            title: None,
            mode: None,
            config_options: Vec::new(),
            context: None,
            usage: Default::default(),
            cost: None,
            commands: Vec::new(),
        },
    )
}

/// The starts of a session's turns up to `upper`: each prompt, and the
/// session's first event where it comes before the first prompt.
async fn turn_starts(
    store: &Store,
    session_id: &str,
    upper: &str,
) -> Result<Vec<EventHead>, StoreError> {
    let mut starts = store
        .session_event_heads(session_id, None, Some(upper), &[PROMPT])
        .await?;
    if let Some(oldest) = store.oldest_session_event_id(session_id).await?
        && starts.first().is_none_or(|first| first.id > oldest)
    {
        let first = store
            .session_event_heads(session_id, Some(&oldest), Some(&oldest), &[])
            .await?;
        starts.splice(0..0, first);
    }
    Ok(starts)
}

/// The ids a read loads, under one budget.
struct Picks {
    ids: BTreeSet<String>,
    budget: usize,
}

impl Picks {
    /// Take all of `ids`, or none of them where they do not fit.
    fn take(&mut self, ids: impl IntoIterator<Item = String>) -> bool {
        let fresh: BTreeSet<String> = ids
            .into_iter()
            .filter(|id| !self.ids.contains(id))
            .collect();
        if self.ids.len() + fresh.len() > self.budget {
            return false;
        }
        self.ids.extend(fresh);
        true
    }

    fn full(&self) -> bool {
        self.ids.len() >= self.budget
    }
}

/// The rows a turn shows whatever its state: its prompt and its answer.
fn mandatory(turn: &TurnIndex, prompt: bool) -> Vec<String> {
    let mut ids = Vec::new();
    if prompt {
        ids.push(turn.id.clone());
    }
    if let (Some(lane), Some(answer)) = (turn.lanes.get(MAIN_LANE), &turn.answer) {
        ids.extend(lane.rows(answer));
    }
    ids
}

/// Each attention step of a turn, with the rows that show it: the actions
/// that wait on a person, or the errors.
fn attention(turn: &TurnIndex, errors: bool) -> Vec<Vec<String>> {
    let Some(lane) = turn.lanes.get(MAIN_LANE) else {
        return Vec::new();
    };
    let ids = if errors { &turn.errors } else { &turn.actions };
    ids.iter().map(|id| lane.rows(id)).collect()
}

/// The last `STEP_WINDOW` steps of a lane, newest first, each with the rows
/// that show it.
fn window(turn: &TurnIndex, lane: &str) -> Vec<Vec<String>> {
    let Some(index) = turn.lanes.get(lane) else {
        return Vec::new();
    };
    index
        .openers
        .iter()
        .rev()
        .take(STEP_WINDOW)
        .map(|opener| index.rows(&opener.step))
        .collect()
}

/// Take a window's steps newest first, as far as the budget goes.
fn take_window(picks: &mut Picks, steps: Vec<Vec<String>>) {
    for rows in steps {
        if !picks.take(rows) {
            return;
        }
    }
}

/// The newest `turns` turns of a session below `before`, or with `before`
/// unset its newest turns. With `lane`, the newest turns that hold steps of
/// that lane, each with its last steps of it.
pub(crate) async fn read_turns(
    store: &Store,
    session_id: &str,
    before: Option<&str>,
    turns: usize,
    lane: Option<&str>,
    budget: usize,
) -> Result<Read, StoreError> {
    let Some(upper) = store.newest_session_event_id(session_id, before).await? else {
        return Ok(Read::default());
    };
    read_turns_upto(store, session_id, &upper, turns, lane, budget).await
}

/// [`read_turns`] at the bound `upper`: no event above it is in the read,
/// whatever the store holds by the time a query of it runs.
pub(crate) async fn read_turns_upto(
    store: &Store,
    session_id: &str,
    upper: &str,
    turns: usize,
    lane: Option<&str>,
    budget: usize,
) -> Result<Read, StoreError> {
    let starts = turn_starts(store, session_id, upper).await?;
    if starts.is_empty() {
        return Ok(Read::default());
    }
    let pairs = store
        .session_event_heads(session_id, None, Some(upper), PAIR_KINDS)
        .await?;
    let turn_of = |id: &str| starts.iter().rposition(|start| start.id.as_str() <= id);
    // The turns that may show: every turn, or the turns of the lane.
    let candidates: Vec<usize> = match lane {
        None => (0..starts.len()).collect(),
        Some(lane) => {
            let ids = store.session_lane_ids(session_id, lane, upper).await?;
            let found: BTreeSet<usize> = ids.iter().filter_map(|id| turn_of(id)).collect();
            found.into_iter().collect()
        }
    };
    let shown = &candidates[candidates.len() - candidates.len().min(turns.max(1))..];
    let mut held: BTreeSet<usize> = shown.iter().copied().collect();
    held.extend(attention_turns(&starts, &pairs, shown.first().copied()));
    let mut heads = Vec::new();
    for (first, last) in runs(&held) {
        let upto = match starts.get(last + 1) {
            Some(next) => {
                store
                    .newest_session_event_id(session_id, Some(&next.id))
                    .await?
            }
            None => Some(upper.to_string()),
        };
        heads.extend(
            store
                .session_event_heads(session_id, Some(&starts[first].id), upto.as_deref(), &[])
                .await?,
        );
    }

    let mut fold = scratch(session_id);
    feed(&mut fold, &heads, &pairs);
    let index: BTreeMap<String, TurnIndex> = fold
        .index()
        .into_iter()
        .map(|turn| (turn.id.clone(), turn))
        .collect();
    let prompt = |at: usize| starts[at].kind == PROMPT;
    let mut picks = Picks {
        ids: BTreeSet::new(),
        budget,
    };
    // The newest turn first, with its step window reserved: then each older
    // turn's prompt and answer, as far as the budget goes.
    let mut first_shown = None;
    for (rank, &at) in shown.iter().rev().enumerate() {
        let Some(turn) = index.get(&starts[at].id) else {
            continue;
        };
        if !picks.take(mandatory(turn, prompt(at))) {
            break;
        }
        match lane {
            Some(lane) => take_window(&mut picks, window(turn, lane)),
            None if rank == 0 && turn.running => {
                take_window(&mut picks, window(turn, MAIN_LANE));
            }
            None => {}
        }
        first_shown = Some(at);
    }
    let first_shown = first_shown.unwrap_or(shown[shown.len() - 1]);
    // What waits on a person, then the errors, each step on its own, newest
    // turn first.
    for errors in [false, true] {
        for turn in index.values().rev() {
            for rows in attention(turn, errors) {
                picks.take(rows);
            }
        }
    }
    let plans = store
        .session_event_heads(session_id, None, Some(upper), &["plan"])
        .await?;
    for rows in first_and_newest_per_lane(&plans) {
        picks.take(rows);
    }
    for (subagent, rows) in fold.subagent_rows() {
        if picks.full() {
            break;
        }
        let first = store
            .first_subagent_event(session_id, &subagent, upper)
            .await?;
        picks.take(first.map(|head| head.id).into_iter().chain(rows));
    }

    let loaded = store
        .events_by_ids(&picks.ids.into_iter().collect::<Vec<_>>())
        .await?;
    let first_turn = starts[first_shown].id.clone();
    Ok(Read {
        items: merge(heads, pairs, loaded, &starts),
        upper: Some(upper.to_string()),
        has_older: candidates.first().is_some_and(|&first| first < first_shown),
        first_turn: Some(first_turn),
        successor: None,
    })
}

/// A page of the whole turns older than the turn `before`. The prompt of
/// `before` closes the last of them as it closed it in the session, and the
/// page drops the turn it opens.
pub(crate) async fn read_page(
    store: &Store,
    session_id: &str,
    before: &str,
    turns: usize,
    lane: Option<&str>,
    budget: usize,
) -> Result<Read, StoreError> {
    let mut read = read_turns(store, session_id, Some(before), turns, lane, budget).await?;
    let successor = store
        .session_event_heads(session_id, Some(before), Some(before), &[])
        .await?;
    if let Some(head) = successor.into_iter().next() {
        read.successor = Some(head.id.clone());
        read.items.push(Item::Head(head));
    }
    Ok(read)
}

/// One page of the steps of a lane of the turn `turn_id`.
pub(crate) struct StepsRead {
    pub(crate) read: Read,
    /// The steps of the page, by id, oldest first.
    pub(crate) page: Vec<String>,
    /// How many steps of the lane come before the page.
    pub(crate) earlier: u64,
}

/// One page of the steps of the turn `turn_id` in a lane: the `limit` steps
/// before the step `before` in the order the lane opened them, or its last
/// ones, as many as the budget loads. None where no turn opens on
/// `turn_id`; an empty page where `before` names no step of the lane.
pub(crate) async fn read_steps(
    store: &Store,
    session_id: &str,
    turn_id: &str,
    lane: &str,
    before: Option<&str>,
    limit: usize,
    budget: usize,
) -> Result<Option<StepsRead>, StoreError> {
    let Some(newest) = store.newest_session_event_id(session_id, None).await? else {
        return Ok(None);
    };
    let starts = turn_starts(store, session_id, &newest).await?;
    let Some(at) = starts.iter().position(|start| start.id == turn_id) else {
        return Ok(None);
    };
    let upper = match starts.get(at + 1) {
        Some(next) => {
            store
                .newest_session_event_id(session_id, Some(&next.id))
                .await?
        }
        None => Some(newest),
    };
    let Some(upper) = upper else {
        return Ok(None);
    };
    let heads = store
        .session_event_heads(session_id, Some(turn_id), Some(&upper), &[])
        .await?;
    let pairs = store
        .session_event_heads(session_id, None, Some(&upper), PAIR_KINDS)
        .await?;
    let mut fold = scratch(session_id);
    feed(&mut fold, &heads, &pairs);
    let Some(turn) = fold.index().into_iter().find(|turn| turn.id == turn_id) else {
        return Ok(None);
    };
    let index = turn.lanes.get(lane).cloned().unwrap_or_default();
    let end = match before {
        None => index.openers.len(),
        // A cursor that names no step of the lane is an empty page.
        Some(before) => index
            .openers
            .iter()
            .position(|opener| opener.step == before)
            .unwrap_or_default(),
    };
    let start = end.saturating_sub(limit);
    let mut picks = Picks {
        ids: BTreeSet::new(),
        budget,
    };
    if starts[at].kind == PROMPT {
        picks.take([turn_id.to_string()]);
    }
    let subagents: BTreeMap<String, Vec<String>> = fold.subagent_rows().into_iter().collect();
    // The page's steps newest first, as many as the budget loads: a page cut
    // short keeps its newest steps and counts the rest as earlier.
    let mut kept = 0;
    for opener in index.openers[start..end].iter().rev() {
        let mut rows = index.rows(&opener.step);
        if let Some(child) = turn
            .lanes
            .get(lane)
            .and_then(|_| subagent_of(&subagents, &opener.step))
        {
            if let Some(first) = store
                .first_subagent_event(session_id, child, &upper)
                .await?
            {
                rows.push(first.id);
            }
            rows.extend(subagents[child].iter().cloned());
        }
        if !picks.take(rows) {
            break;
        }
        kept += 1;
    }
    let page: Vec<String> = index.openers[end - kept..end]
        .iter()
        .map(|opener| opener.step.clone())
        .collect();
    let loaded = store
        .events_by_ids(&picks.ids.into_iter().collect::<Vec<_>>())
        .await?;
    Ok(Some(StepsRead {
        read: Read {
            items: merge(heads, pairs, loaded, &starts),
            upper: Some(upper),
            first_turn: Some(turn_id.to_string()),
            has_older: at > 0,
            successor: None,
        },
        page,
        earlier: (end - kept) as u64,
    }))
}

/// The subagent whose step `step` is, by the rows the fold keeps of it.
fn subagent_of<'a>(subagents: &'a BTreeMap<String, Vec<String>>, step: &str) -> Option<&'a str> {
    subagents
        .iter()
        .find(|(_, rows)| rows.first().is_some_and(|row| row == step))
        .map(|(id, _)| id.as_str())
}

/// Fold heads in id order, with the pairing heads outside them.
fn feed(fold: &mut Fold, heads: &[EventHead], pairs: &[EventHead]) {
    let held: BTreeSet<&str> = heads.iter().map(|head| head.id.as_str()).collect();
    let mut pairs = pairs
        .iter()
        .filter(|pair| !held.contains(pair.id.as_str()))
        .peekable();
    for head in heads {
        while let Some(pair) = pairs.next_if(|pair| pair.id < head.id) {
            fold.pair_head(pair);
        }
        fold.apply_head(head);
    }
    for pair in pairs {
        fold.pair_head(pair);
    }
}

/// The heads, the pairing heads outside them, and the loaded events in
/// their places, in id order.
fn merge(
    heads: Vec<EventHead>,
    pairs: Vec<EventHead>,
    loaded: Vec<AgentEvent>,
    starts: &[EventHead],
) -> Vec<Item> {
    let held: BTreeSet<String> = heads.iter().map(|head| head.id.clone()).collect();
    let mut loaded: BTreeMap<String, AgentEvent> = loaded
        .into_iter()
        .map(|event| (event.id.clone(), event))
        .collect();
    let mut items: Vec<Item> = heads
        .into_iter()
        .map(|head| match loaded.remove(&head.id) {
            Some(event) => Item::Full(Box::new(event)),
            None => Item::Head(head),
        })
        .collect();
    items.extend(
        pairs
            .into_iter()
            .filter(|pair| !held.contains(&pair.id))
            .map(|pair| match loaded.remove(&pair.id) {
                Some(event) => Item::Full(Box::new(event)),
                None => Item::Pair(pair),
            }),
    );
    // A plan or a subagent's first event outside the turns a read holds is
    // a plan or a subagent the snapshot shows all the same, in the turn it
    // came in.
    items.extend(loaded.into_values().map(|event| {
        let turn = starts
            .iter()
            .rfind(|start| start.id <= event.id)
            .map_or_else(|| event.id.clone(), |start| start.id.clone());
        Item::Seed(Box::new(event), turn)
    }));
    items.sort_by(|a, b| a.id().cmp(b.id()));
    items
}

/// The first and the newest plan of each lane: the first names the plan,
/// and the newest says what it holds.
fn first_and_newest_per_lane(plans: &[EventHead]) -> Vec<Vec<String>> {
    let mut lanes: BTreeMap<Option<&str>, (String, String)> = BTreeMap::new();
    for plan in plans {
        lanes
            .entry(plan.lane.as_deref())
            .and_modify(|(_, newest)| *newest = plan.id.clone())
            .or_insert_with(|| (plan.id.clone(), plan.id.clone()));
    }
    lanes
        .into_values()
        .map(|(first, newest)| vec![first, newest])
        .collect()
}

/// Contiguous runs of turn indices, as first and last.
fn runs(turns: &BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &at in turns {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == at => *last = at,
            _ => runs.push((at, at)),
        }
    }
    runs
}

/// The turns before `first_shown` that hold attention: every turn with a
/// permission or a question that waits, and the newest [`ERROR_TURNS`]
/// turns with an error.
fn attention_turns(
    starts: &[EventHead],
    pairs: &[EventHead],
    first_shown: Option<usize>,
) -> BTreeSet<usize> {
    let mut pending: Vec<&str> = Vec::new();
    let mut open: BTreeMap<String, &str> = BTreeMap::new();
    let mut errors: Vec<&str> = Vec::new();
    for pair in pairs {
        let facts: EventFacts = serde_json::from_str(&pair.facts).unwrap_or_default();
        match pair.kind.as_str() {
            "permission_request" => pending.push(&pair.id),
            "permission.replied" => {
                pending.pop();
            }
            "question" | "agent_request" => {
                if (pair.kind == "question" || facts.asks)
                    && let Some(key) = facts.question.or(facts.request)
                {
                    open.insert(key, &pair.id);
                }
            }
            "question.resolved" | "agent_request.resolved" => {
                if let Some(key) = facts.question.or(facts.request) {
                    open.remove(&key);
                }
            }
            "session.error" => errors.push(&pair.id),
            _ => {}
        }
    }
    let first_shown = first_shown.unwrap_or(starts.len());
    let turn_of = |id: &str| {
        starts
            .iter()
            .rposition(|start| start.id.as_str() <= id)
            .filter(|&at| at < first_shown)
    };
    let mut turns: BTreeSet<usize> = pending
        .into_iter()
        .chain(open.into_values())
        .filter_map(turn_of)
        .collect();
    let failed: BTreeSet<usize> = errors.into_iter().filter_map(turn_of).collect();
    turns.extend(failed.into_iter().rev().take(ERROR_TURNS));
    turns
}

#[cfg(test)]
mod tests {
    use ariadne_api::trajectory::{MAIN_LANE, QuestionState, StepDto, TrajectoryOpDto, TurnState};
    use ariadne_store::{NewAgentEvent, Store};
    use serde_json::{Value, json};

    use super::{Read, read_page, read_steps, read_turns, scratch};
    use crate::trajectory::Fold;

    /// A session of its own in a store of its own.
    async fn session() -> (Store, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let repository = store
            .create_repository(ariadne_store::NewRepository {
                default_workflow: None,
                path: "/tmp/probe".into(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await
            .unwrap();
        let goal = store
            .create_goal(ariadne_store::NewGoal {
                workflow: None,
                issue_url: None,
                title: "probe".into(),
                description: String::new(),
                repository_ids: vec![repository.id.clone()],
                pin: ariadne_store::AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            })
            .await
            .unwrap();
        let session = store
            .create_session(ariadne_store::NewSession {
                goal_id: Some(goal.id.clone()),
                task_id: None,
                seat: Some(ariadne_core::Seat::Orchestrator),
                task_agent_id: None,
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: None,
                pull_request_id: None,
            })
            .await
            .unwrap()
            .id;
        (store, session, dir)
    }

    async fn commit(store: &Store, session: &str, kind: &str, payload: Value) -> String {
        store
            .create_event(NewAgentEvent {
                session_id: Some(session.into()),
                task_id: None,
                kind: kind.into(),
                payload,
            })
            .await
            .unwrap()
            .id
    }

    async fn prompt(store: &Store, session: &str) -> String {
        commit(store, session, "user_prompt_submit", json!({"text": "Go"})).await
    }

    async fn call(store: &Store, session: &str, id: &str, kind: &str, status: &str) -> String {
        let opened = commit(
            store,
            session,
            "pre_tool_use",
            json!({"tool_name": "Edit", "acp": {"toolCallId": id, "kind": kind, "status": "pending"}}),
        )
        .await;
        commit(
            store,
            session,
            "post_tool_use",
            json!({"tool_name": "Edit", "acp": {"toolCallId": id, "kind": kind, "status": status,
                                                "locations": [{"path": format!("{id}.rs")}],
                                                "content": [{"type": "content",
                                                             "content": {"type": "text",
                                                                         "text": "x".repeat(2000)}}]}}),
        )
        .await;
        opened
    }

    async fn stop(store: &Store, session: &str) {
        commit(store, session, "stop", json!({"stop_reason": "end_turn"})).await;
    }

    fn folded(session: &str, read: Read) -> Fold {
        let mut fold = scratch(session);
        read.fold_into(&mut fold, Vec::new());
        fold
    }

    /// An open of many turns loads at most the budget, and starts on a
    /// prompt; the turns it leaves out are older.
    #[tokio::test]
    async fn the_open_of_a_long_session_reads_a_bounded_count_of_payloads() {
        let (store, session, _dir) = session().await;
        let mut prompts = Vec::new();
        for turn in 0..12 {
            prompts.push(prompt(&store, &session).await);
            for n in 0..4 {
                call(
                    &store,
                    &session,
                    &format!("c-{turn}-{n}"),
                    "read",
                    "completed",
                )
                .await;
            }
            commit(
                &store,
                &session,
                "agent_message",
                json!({"text": format!("answer {turn}")}),
            )
            .await;
            stop(&store, &session).await;
        }

        let read = read_turns(&store, &session, None, 50, None, 10)
            .await
            .unwrap();
        assert!(
            read.loaded() <= 10,
            "the budget, and no more: {}",
            read.loaded()
        );
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 50, None);

        assert_eq!(
            snapshot.turns.len(),
            5,
            "five prompts and five answers fit in ten"
        );
        assert_eq!(snapshot.turns[0].id, prompts[7]);
        assert!(snapshot.has_older);
        let last = snapshot.turns.last().unwrap();
        assert_eq!(last.activity.tool_calls, 4);
        assert_eq!(
            last.answer,
            vec![ariadne_api::trajectory::ContentBlockDto::Text {
                text: "answer 11".into()
            }]
        );
    }

    /// A closed turn far larger than the budget keeps its exact counts and
    /// its answer, though both come long before its newest events.
    #[tokio::test]
    async fn an_oversized_closed_turn_keeps_its_answer_and_exact_activity() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        call(&store, &session, "edit-1", "edit", "completed").await;
        call(&store, &session, "edit-2", "edit", "failed").await;
        commit(&store, &session, "agent_message", json!({"text": "Done."})).await;
        for _ in 0..300 {
            commit(&store, &session, "agent_thought", json!({"text": "think"})).await;
        }
        stop(&store, &session).await;

        let read = read_turns(&store, &session, None, 20, None, 20)
            .await
            .unwrap();
        assert!(read.loaded() <= 20, "{}", read.loaded());
        let fold = folded(&session, read);
        let shown = &fold.snapshot(0, 20, None).turns[0];

        assert_eq!(shown.id, turn);
        assert_eq!(shown.state, TurnState::Completed);
        assert_eq!(shown.activity.tool_calls, 2);
        assert_eq!(shown.activity.files_changed, 1);
        assert_eq!(shown.activity.failed, 1);
        assert_eq!(shown.activity.thoughts, 300);
        assert_eq!(shown.earlier_steps, 303);
        assert_eq!(
            shown.answer,
            vec![ariadne_api::trajectory::ContentBlockDto::Text {
                text: "Done.".into()
            }]
        );
    }

    /// A running turn whose newest events are plan updates still carries
    /// its last steps: the window is picked by steps, not by events.
    #[tokio::test]
    async fn a_running_turns_window_is_its_last_steps_whatever_follows_them() {
        let (store, session, _dir) = session().await;
        prompt(&store, &session).await;
        let mut calls = Vec::new();
        for n in 0..60 {
            calls.push(call(&store, &session, &format!("c-{n}"), "read", "completed").await);
        }
        for _ in 0..600 {
            commit(
                &store,
                &session,
                "plan",
                json!({"entries": [{"content": "a", "status": "pending"}]}),
            )
            .await;
        }

        let read = read_turns(&store, &session, None, 20, None, 500)
            .await
            .unwrap();
        assert!(read.loaded() <= 500);
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 20, None);
        let turn = &snapshot.turns[0];

        assert_eq!(turn.state, TurnState::Running);
        let ids: Vec<&str> = turn.steps.iter().map(crate::trajectory::step_id).collect();
        let expected: Vec<&str> = calls[10..].iter().map(String::as_str).collect();
        assert_eq!(ids, expected, "the last fifty calls");
        assert_eq!(turn.earlier_steps, 10);
        assert_eq!(snapshot.plans.len(), 1, "the newest plan");
    }

    /// A page of a turn's steps comes from its cursor, past what an open
    /// holds, and a page asks for a turn that exists.
    #[tokio::test]
    async fn the_steps_route_pages_by_its_cursor_and_names_a_turn() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        let mut calls = Vec::new();
        for n in 0..120 {
            calls.push(call(&store, &session, &format!("c-{n}"), "read", "completed").await);
        }
        stop(&store, &session).await;

        let page = read_steps(
            &store,
            &session,
            &turn,
            MAIN_LANE,
            Some(&calls[20]),
            10,
            500,
        )
        .await
        .unwrap()
        .expect("the turn exists");
        assert_eq!(page.page, calls[10..20]);
        assert_eq!(page.earlier, 10);
        assert!(
            page.read.loaded() <= 21,
            "the page, its updates and its prompt"
        );
        let fold = folded(&session, page.read);
        let held: Vec<String> = fold
            .held_steps(&turn, MAIN_LANE)
            .iter()
            .map(|step| crate::trajectory::step_id(step).to_string())
            .collect();
        assert_eq!(held, calls[10..20]);

        assert!(
            read_steps(&store, &session, &calls[3], MAIN_LANE, None, 10, 500)
                .await
                .unwrap()
                .is_none(),
            "a call opens no turn"
        );
    }

    /// An open question in a turn older than the window, and an error early
    /// in a large turn, stay attention in their own turns; a later answer
    /// changes the question in its turn.
    #[tokio::test]
    async fn attention_outside_the_window_keeps_its_turn() {
        let (store, session, _dir) = session().await;
        let old = prompt(&store, &session).await;
        let question = commit(
            &store,
            &session,
            "question",
            json!({"question": {"id": "q-1", "message": "Which?", "schema": {}}}),
        )
        .await;
        stop(&store, &session).await;
        for _ in 0..5 {
            prompt(&store, &session).await;
            stop(&store, &session).await;
        }
        let big = prompt(&store, &session).await;
        let error = commit(
            &store,
            &session,
            "session.error",
            json!({"message": "boom"}),
        )
        .await;
        prompt(&store, &session).await;
        for n in 0..80 {
            call(&store, &session, &format!("c-{n}"), "read", "completed").await;
        }

        let read = read_turns(&store, &session, None, 3, None, 500)
            .await
            .unwrap();
        let mut fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 3, None);
        assert!(
            snapshot.turns.iter().all(|turn| turn.id != old),
            "the old turn is not shown"
        );
        let failed = snapshot.turns.iter().find(|turn| turn.id == big).unwrap();
        assert_eq!(failed.state, TurnState::Failed);
        assert!(
            matches!(failed.attention.as_slice(), [StepDto::Error(e)] if e.id == error),
            "{:?}",
            failed.attention
        );

        let ops = fold.apply(
            &ariadne_api::events::AgentEventDto {
                id: "zzzz".into(),
                session_id: Some(session.clone()),
                task_id: None,
                kind: "question.resolved".into(),
                payload: json!({"question_id": "q-1", "action": "accept", "content": {"a": "b"}}),
                summary: String::new(),
                created_at: "2026-11-27T00:00:00Z".into(),
            },
            true,
        );
        assert!(
            ops.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step: StepDto::Question(q) }
                    if q.id == question && q.turn_id == old && q.state == QuestionState::Answered)),
            "{ops:?}"
        );
    }

    /// A page before a prompt that cut its predecessor short shows that turn
    /// interrupted, as the session ran it, and not running.
    #[tokio::test]
    async fn a_page_before_an_interrupting_prompt_keeps_the_interruption() {
        let (store, session, _dir) = session().await;
        let first = prompt(&store, &session).await;
        commit(&store, &session, "agent_message", json!({"text": "half"})).await;
        let second = prompt(&store, &session).await;
        stop(&store, &session).await;

        let read = read_page(&store, &session, &second, 5, None, 500)
            .await
            .unwrap();
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 5, None);

        let [turn] = snapshot.turns.as_slice() else {
            panic!("the one older turn: {:?}", snapshot.turns);
        };
        assert_eq!(turn.id, first);
        assert_eq!(turn.state, TurnState::Interrupted);
        assert_eq!(snapshot.session.running_turn_id, None);
    }

    /// A subagent opened before the window and reported done inside it
    /// keeps the id of its first event and the turn of its parent.
    #[tokio::test]
    async fn a_subagent_opened_before_the_window_keeps_its_step_and_parent() {
        let (store, session, _dir) = session().await;
        let parent = prompt(&store, &session).await;
        let child = json!({"id": "child-1", "title": "explorer", "description": "Look"});
        let opened = commit(
            &store,
            &session,
            "subagent_update",
            json!({"subagent": child, "state": "running"}),
        )
        .await;
        stop(&store, &session).await;
        for _ in 0..3 {
            prompt(&store, &session).await;
            stop(&store, &session).await;
        }
        prompt(&store, &session).await;
        commit(
            &store,
            &session,
            "subagent_update",
            json!({"subagent": child, "state": "completed"}),
        )
        .await;

        let read = read_turns(&store, &session, None, 1, None, 500)
            .await
            .unwrap();
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 1, None);

        let [subagent] = snapshot.subagents.as_slice() else {
            panic!("one subagent: {:?}", snapshot.subagents);
        };
        assert_eq!(
            subagent.id, opened,
            "the step is the first event that named it"
        );
        assert_eq!(subagent.turn_id, parent, "in the turn of its parent");
        assert_eq!(subagent.state.as_deref(), Some("completed"));
        assert_eq!(subagent.title, "explorer");
        assert_eq!(
            snapshot.turns[0].activity.subagents, 0,
            "the newest turn started none"
        );
    }

    /// One turn of thousands of events loads no more than the budget.
    #[tokio::test]
    async fn a_turn_of_thousands_of_events_loads_the_budget_at_most() {
        let (store, session, _dir) = session().await;
        prompt(&store, &session).await;
        for n in 0..1500 {
            call(&store, &session, &format!("c-{n}"), "read", "completed").await;
        }

        let read = read_turns(&store, &session, None, 20, None, 500)
            .await
            .unwrap();
        assert!(read.loaded() <= 500, "{}", read.loaded());
        let fold = folded(&session, read);
        let turn = &fold.snapshot(0, 20, None).turns[0];
        assert_eq!(turn.activity.tool_calls, 1500);
        assert_eq!(turn.steps.len(), 50);
        assert_eq!(turn.earlier_steps, 1450);
    }

    /// A session stored before the facts existed reads as exactly as a new
    /// one: the store fills its facts once, when it opens.
    #[tokio::test]
    async fn an_old_session_reads_exactly_once_its_facts_are_filled() {
        let (store, session, dir) = session().await;
        let turn = prompt(&store, &session).await;
        call(&store, &session, "edit-1", "edit", "completed").await;
        call(&store, &session, "edit-2", "edit", "failed").await;
        let question = commit(
            &store,
            &session,
            "question",
            json!({"question": {"id": "q-1", "message": "Which?", "schema": {}}}),
        )
        .await;
        commit(&store, &session, "agent_message", json!({"text": "Done."})).await;
        stop(&store, &session).await;
        let fresh = folded(
            &session,
            read_turns(&store, &session, None, 20, None, 500)
                .await
                .unwrap(),
        )
        .snapshot(0, 20, None);
        store.close().await;

        // As a release before migration 0029 left it: no facts, no fill.
        let path = dir.path().join("test.db");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        sqlx::query("UPDATE agent_events SET lane = NULL, facts = NULL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM store_fills")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let store = Store::open(&path).await.unwrap();
        let old = folded(
            &session,
            read_turns(&store, &session, None, 20, None, 500)
                .await
                .unwrap(),
        )
        .snapshot(0, 20, None);

        assert_eq!(old, fresh, "the old session reads as it did when new");
        let shown = &old.turns[0];
        assert_eq!(shown.id, turn);
        assert_eq!(
            (
                shown.activity.tool_calls,
                shown.activity.failed,
                shown.activity.files_changed
            ),
            (2, 1, 1)
        );
        assert!(
            matches!(shown.attention.as_slice(), [StepDto::Question(q)] if q.id == question),
            "{:?}",
            shown.attention
        );
    }

    /// A read pinned at a bound holds nothing above it, whatever the store
    /// holds by the time its queries run (rule 37).
    #[tokio::test]
    async fn a_read_holds_nothing_above_its_bound() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        commit(&store, &session, "agent_message", json!({"text": "Done."})).await;
        let upper = store
            .newest_session_event_id(&session, None)
            .await
            .unwrap()
            .unwrap();
        let newer = prompt(&store, &session).await;
        commit(&store, &session, "agent_message", json!({"text": "Later."})).await;

        let read = super::read_turns_upto(&store, &session, &upper, 20, None, 500)
            .await
            .unwrap();

        assert_eq!(read.upper.as_deref(), Some(upper.as_str()));
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 20, None);
        let ids: Vec<&str> = snapshot.turns.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [turn.as_str()],
            "the newer turn {newer} is above the bound"
        );
        assert_eq!(
            snapshot.turns[0].answer,
            vec![ariadne_api::trajectory::ContentBlockDto::Text {
                text: "Done.".into()
            }]
        );
    }

    /// A message whose `item_id` is not its event's id keeps its answer in
    /// a read, and pages by its wire id (rules 18, 44).
    #[tokio::test]
    async fn a_message_with_its_own_item_id_reads_and_pages_by_it() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        let first = call(&store, &session, "c-1", "read", "completed").await;
        commit(
            &store,
            &session,
            "agent_message",
            json!({"item_id": "item-a", "text": "Looking."}),
        )
        .await;
        call(&store, &session, "c-2", "read", "completed").await;
        commit(
            &store,
            &session,
            "agent_message",
            json!({"item_id": "item-b", "text": "Done."}),
        )
        .await;
        stop(&store, &session).await;

        let fold = folded(
            &session,
            read_turns(&store, &session, None, 20, None, 500)
                .await
                .unwrap(),
        );
        assert_eq!(
            fold.snapshot(0, 20, None).turns[0].answer,
            vec![ariadne_api::trajectory::ContentBlockDto::Text {
                text: "Done.".into()
            }]
        );

        let page = read_steps(&store, &session, &turn, MAIN_LANE, Some("item-b"), 2, 500)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            page.page[0], "item-a",
            "the cursor is the step's place, not its id's order"
        );
        assert_eq!(page.earlier, 1);
        let fold = folded(&session, page.read);
        let held: Vec<String> = fold
            .held_steps(&turn, MAIN_LANE)
            .iter()
            .map(|step| crate::trajectory::step_id(step).to_string())
            .collect();
        assert!(held.contains(&"item-a".to_string()), "{held:?}");
        assert!(
            !held.contains(&first),
            "the page starts after the first call"
        );
    }

    /// A plan keeps the id of its first event in a cold read (rule 32).
    #[tokio::test]
    async fn a_cold_read_names_a_plan_by_its_first_event() {
        let (store, session, _dir) = session().await;
        prompt(&store, &session).await;
        let first = commit(
            &store,
            &session,
            "plan",
            json!({"entries": [{"content": "a", "status": "pending"}]}),
        )
        .await;
        commit(
            &store,
            &session,
            "plan",
            json!({"entries": [{"content": "a", "status": "completed"}]}),
        )
        .await;

        let fold = folded(
            &session,
            read_turns(&store, &session, None, 20, None, 500)
                .await
                .unwrap(),
        );
        let plans = fold.snapshot(0, 20, None).plans;
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].plan_id, first);
        assert_eq!(plans[0].entries[0].status, "completed");
    }

    /// A turn with more errors than the budget keeps its prompt and its
    /// answer, and as many of its errors as fit (rules 36, 38).
    #[tokio::test]
    async fn a_turn_of_many_errors_keeps_its_prompt_and_answer() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        commit(
            &store,
            &session,
            "agent_message",
            json!({"text": "Trying."}),
        )
        .await;
        for _ in 0..30 {
            commit(
                &store,
                &session,
                "session.error",
                json!({"message": "boom"}),
            )
            .await;
        }

        let read = read_turns(&store, &session, None, 20, None, 10)
            .await
            .unwrap();
        assert!(read.loaded() <= 10);
        let fold = folded(&session, read);
        let shown = &fold.snapshot(0, 20, None).turns[0];
        assert_eq!(shown.id, turn);
        assert_eq!(
            shown.prompt.as_ref().unwrap().content,
            vec![ariadne_api::trajectory::ContentBlockDto::Text { text: "Go".into() }]
        );
        assert_eq!(
            shown.answer,
            vec![ariadne_api::trajectory::ContentBlockDto::Text {
                text: "Trying.".into()
            }]
        );
        assert_eq!(shown.attention.len(), 8, "the errors that fit the rest");
    }

    /// The newest turn's step window comes before the subagents of the
    /// read: many children do not take the running turn's last steps.
    #[tokio::test]
    async fn the_running_turns_window_comes_before_the_subagents() {
        let (store, session, _dir) = session().await;
        prompt(&store, &session).await;
        for n in 0..40 {
            let child = json!({"id": format!("child-{n}"), "title": "child"});
            commit(
                &store,
                &session,
                "subagent_update",
                json!({"subagent": child, "state": "running"}),
            )
            .await;
            commit(
                &store,
                &session,
                "subagent_update",
                json!({"subagent": child, "state": "completed"}),
            )
            .await;
        }
        let mut calls = Vec::new();
        for n in 0..50 {
            calls.push(call(&store, &session, &format!("c-{n}"), "read", "completed").await);
        }

        let read = read_turns(&store, &session, None, 20, None, 110)
            .await
            .unwrap();
        assert!(read.loaded() <= 110);
        let fold = folded(&session, read);
        let turn = &fold.snapshot(0, 20, None).turns[0];
        let ids: Vec<&str> = turn.steps.iter().map(crate::trajectory::step_id).collect();
        let expected: Vec<&str> = calls.iter().map(String::as_str).collect();
        assert_eq!(ids, expected, "the last fifty steps are the calls");
        assert_eq!(turn.activity.subagents, 40);
    }

    /// A question that waits in a turn behind more than twenty failed turns
    /// stays in the read, and its later answer finds it (rule 39).
    #[tokio::test]
    async fn a_question_behind_many_failed_turns_stays_in_the_read() {
        let (store, session, _dir) = session().await;
        let old = prompt(&store, &session).await;
        let question = commit(
            &store,
            &session,
            "question",
            json!({"question": {"id": "q-1", "message": "Which?", "schema": {}}}),
        )
        .await;
        stop(&store, &session).await;
        for _ in 0..21 {
            prompt(&store, &session).await;
            commit(
                &store,
                &session,
                "session.error",
                json!({"message": "boom"}),
            )
            .await;
        }
        prompt(&store, &session).await;

        let read = read_turns(&store, &session, None, 1, None, 500)
            .await
            .unwrap();
        let mut fold = folded(&session, read);
        let ops = fold.apply(
            &ariadne_api::events::AgentEventDto {
                id: "zzzz".into(),
                session_id: Some(session.clone()),
                task_id: None,
                kind: "question.resolved".into(),
                payload: json!({"question_id": "q-1", "action": "decline"}),
                summary: String::new(),
                created_at: "2026-11-27T00:00:00Z".into(),
            },
            true,
        );
        assert!(
            ops.iter().any(|op| matches!(op,
                TrajectoryOpDto::ItemUpsert { step: StepDto::Question(q) }
                    if q.id == question && q.turn_id == old && q.state == QuestionState::Declined)),
            "{ops:?}"
        );
    }

    /// A subagent's turns are chosen by its lane before the limit: a child
    /// that ran before many newer turns still shows (rule 43).
    #[tokio::test]
    async fn a_subagent_query_chooses_the_turns_of_its_lane() {
        let (store, session, _dir) = session().await;
        let parent = prompt(&store, &session).await;
        let child = json!({"id": "child-1", "title": "explorer"});
        commit(
            &store,
            &session,
            "subagent_update",
            json!({"subagent": child, "state": "running"}),
        )
        .await;
        let read_call = commit(
            &store,
            &session,
            "pre_tool_use",
            json!({"subagent": child, "acp": {"toolCallId": "c-1", "kind": "read", "status": "pending"}}),
        )
        .await;
        stop(&store, &session).await;
        for _ in 0..21 {
            prompt(&store, &session).await;
            stop(&store, &session).await;
        }

        let read = read_turns(&store, &session, None, 20, Some("child-1"), 500)
            .await
            .unwrap();
        let fold = folded(&session, read);
        let snapshot = fold.snapshot(0, 20, Some("child-1"));
        let [turn] = snapshot.turns.as_slice() else {
            panic!("the child's one turn: {:?}", snapshot.turns);
        };
        assert_eq!(turn.id, parent);
        assert_eq!(crate::trajectory::step_id(&turn.steps[0]), read_call);
        assert!(!snapshot.has_older, "no older turn holds the child");
    }

    /// A page of a turn's steps loads the budget at most, many subagents or
    /// not, and counts what it leaves before it (rule 36).
    #[tokio::test]
    async fn a_page_of_steps_loads_the_budget_at_most() {
        let (store, session, _dir) = session().await;
        let turn = prompt(&store, &session).await;
        for n in 0..30 {
            let child = json!({"id": format!("child-{n}"), "title": "child"});
            commit(
                &store,
                &session,
                "subagent_update",
                json!({"subagent": child, "state": "running"}),
            )
            .await;
            commit(
                &store,
                &session,
                "subagent_update",
                json!({"subagent": child, "state": "completed"}),
            )
            .await;
        }

        let page = read_steps(&store, &session, &turn, MAIN_LANE, None, 50, 20)
            .await
            .unwrap()
            .unwrap();

        assert!(page.read.loaded() <= 20, "{}", page.read.loaded());
        assert_eq!(
            page.page.len(),
            9,
            "a prompt and nine subagents of two rows each"
        );
        assert_eq!(page.earlier, 21);
    }
}
