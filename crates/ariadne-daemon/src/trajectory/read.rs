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

/// How many turns older than the ones a read shows it keeps for the
/// attention they hold: a permission or a question that still waits, or an
/// error that a diagnosis may still name. Twenty keeps a day of failed
/// turns, and keeps the scan of their heads short.
const OUTSIDE_TURNS: usize = 20;

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
}

/// What a turn shows whatever its state: its prompt, its answer, and its
/// attention with the events that changed it.
fn essentials(turn: &TurnIndex, prompt: bool) -> Vec<String> {
    let main = turn.lanes.get(MAIN_LANE);
    let mut ids: Vec<String> = Vec::new();
    if prompt {
        ids.push(turn.id.clone());
    }
    ids.extend(turn.answer.clone());
    for id in &turn.attention {
        ids.push(id.clone());
        ids.extend(main.and_then(|lane| lane.follows.get(id)).cloned());
    }
    ids
}

/// The last `STEP_WINDOW` steps of a lane, with the newest event of each.
fn window(turn: &TurnIndex, lane: &str) -> Vec<String> {
    let Some(index) = turn.lanes.get(lane) else {
        return Vec::new();
    };
    let skip = index.openers.len().saturating_sub(STEP_WINDOW);
    index.openers[skip..]
        .iter()
        .flat_map(|id| std::iter::once(id.clone()).chain(index.follows.get(id).cloned()))
        .collect()
}

/// The newest `turns` turns of a session below `before`, or with `before`
/// unset its newest turns. With `lane`, each turn carries its last steps of
/// that lane.
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
    let starts = turn_starts(store, session_id, &upper).await?;
    let Some(first_start) = starts.first() else {
        return Ok(Read::default());
    };
    let shown = starts.len().min(turns.max(1));
    let kept_from = starts[starts.len() - shown].id.clone();
    let pairs = store
        .session_event_heads(session_id, None, Some(&upper), PAIR_KINDS)
        .await?;
    let mut heads = Vec::new();
    for (start, end) in outside_starts(&starts, &pairs, &kept_from) {
        let upto = match end {
            Some(end) => {
                store
                    .newest_session_event_id(session_id, Some(&end))
                    .await?
            }
            None => Some(upper.clone()),
        };
        heads.extend(
            store
                .session_event_heads(session_id, Some(&start), upto.as_deref(), &[])
                .await?,
        );
    }
    heads.extend(
        store
            .session_event_heads(session_id, Some(&kept_from), Some(&upper), &[])
            .await?,
    );

    let mut fold = scratch(session_id);
    feed(&mut fold, &heads, &pairs);
    let index = fold.index();
    let prompts: BTreeSet<&str> = starts
        .iter()
        .filter(|start| start.kind == PROMPT)
        .map(|start| start.id.as_str())
        .collect();

    let mut picks = Picks {
        ids: BTreeSet::new(),
        budget,
    };
    picks.take(fold.subagent_events());
    picks.take(first_subagent_events(store, session_id, &fold, &upper).await?);
    let plans = store
        .session_event_heads(session_id, None, Some(&upper), &["plan"])
        .await?;
    picks.take(newest_per_lane(&plans));
    // The shown turns, newest first, as far as the budget goes.
    let mut first_turn = None;
    for turn in index.iter().rev().filter(|turn| turn.id >= kept_from) {
        let mut ids = essentials(turn, prompts.contains(turn.id.as_str()));
        match lane {
            Some(lane) => ids.extend(window(turn, lane)),
            None if turn.running => ids.extend(window(turn, MAIN_LANE)),
            None => {}
        }
        if !picks.take(ids) {
            break;
        }
        first_turn = Some(turn.id.clone());
    }
    let first_turn = first_turn.unwrap_or_else(|| starts[starts.len() - 1].id.clone());
    // The attention of the turns before it, as far as the budget goes.
    for turn in index.iter().rev().filter(|turn| turn.id < first_turn) {
        let ids: Vec<String> = essentials(turn, false)
            .into_iter()
            .filter(|id| Some(id) != turn.answer.as_ref())
            .collect();
        picks.take(ids);
    }

    let loaded = store
        .events_by_ids(&picks.ids.into_iter().collect::<Vec<_>>())
        .await?;
    Ok(Read {
        items: merge(heads, pairs, loaded, &starts),
        upper: Some(upper),
        has_older: first_turn.as_str() > first_start.id.as_str(),
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
/// before the step `before`, or the last ones. None where no turn opens on
/// `turn_id`.
pub(crate) async fn read_steps(
    store: &Store,
    session_id: &str,
    turn_id: &str,
    lane: &str,
    before: Option<&str>,
    limit: usize,
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
    let older: Vec<&String> = index
        .openers
        .iter()
        .filter(|id| before.is_none_or(|before| id.as_str() < before))
        .collect();
    let skip = older.len().saturating_sub(limit);
    let page: Vec<String> = older[skip..].iter().map(|id| (*id).clone()).collect();
    let mut picks: Vec<String> = page
        .iter()
        .flat_map(|id| std::iter::once(id.clone()).chain(index.follows.get(id).cloned()))
        .collect();
    if starts[at].kind == PROMPT {
        picks.push(turn_id.to_string());
    }
    picks.extend(fold.subagent_events());
    picks.extend(first_subagent_events(store, session_id, &fold, &upper).await?);
    picks.sort();
    picks.dedup();
    let loaded = store.events_by_ids(&picks).await?;
    Ok(Some(StepsRead {
        read: Read {
            items: merge(heads, pairs, loaded, &starts),
            upper: Some(upper),
            first_turn: Some(turn_id.to_string()),
            has_older: at > 0,
            successor: None,
        },
        page,
        earlier: skip as u64,
    }))
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

/// The first event of each subagent the fold knows, where the session
/// holds one before the fold's: what the subagent's step is, and the turn it
/// belongs to.
async fn first_subagent_events(
    store: &Store,
    session_id: &str,
    fold: &Fold,
    upper: &str,
) -> Result<Vec<String>, StoreError> {
    let mut ids = Vec::new();
    for subagent in fold.subagent_ids() {
        if let Some(first) = store
            .first_subagent_event(session_id, &subagent, upper)
            .await?
        {
            ids.push(first.id);
        }
    }
    Ok(ids)
}

/// The newest plan of each lane.
fn newest_per_lane(plans: &[EventHead]) -> Vec<String> {
    let mut newest: BTreeMap<Option<&str>, &str> = BTreeMap::new();
    for plan in plans {
        newest.insert(plan.lane.as_deref(), &plan.id);
    }
    newest.into_values().map(str::to_string).collect()
}

/// The turns before `kept_from` that hold attention — a permission or a
/// question that waits, or an error — at most [`OUTSIDE_TURNS`] of the
/// newest, oldest first, each with the start of the turn after it.
fn outside_starts(
    starts: &[EventHead],
    pairs: &[EventHead],
    kept_from: &str,
) -> Vec<(String, Option<String>)> {
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
    let mut turns: BTreeSet<usize> = BTreeSet::new();
    for id in pending.into_iter().chain(open.into_values()).chain(errors) {
        if id >= kept_from {
            continue;
        }
        if let Some(at) = starts.iter().rposition(|start| start.id.as_str() <= id) {
            turns.insert(at);
        }
    }
    let skip = turns.len().saturating_sub(OUTSIDE_TURNS);
    turns
        .into_iter()
        .skip(skip)
        .map(|at| {
            (
                starts[at].id.clone(),
                starts.get(at + 1).map(|next| next.id.clone()),
            )
        })
        .collect()
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

        let page = read_steps(&store, &session, &turn, MAIN_LANE, Some(&calls[20]), 10)
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
            read_steps(&store, &session, &calls[3], MAIN_LANE, None, 10)
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
}
