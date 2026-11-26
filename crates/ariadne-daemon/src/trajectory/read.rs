//! Which stored events a fold opens on (040).
//!
//! A session that ran for a day holds tens of thousands of events, and a
//! tool end averages ten kilobytes. So a read counts and lists ids first,
//! which read no payload, and reads whole events only for a bounded window
//! of whole turns.

use ariadne_store::{AgentEvent, Store, StoreError};

/// The most full event payloads one read takes from the store: an open of a
/// trajectory, a page of older turns, or a page of a turn's steps. Five
/// hundred events are about five megabytes at the ten-kilobyte average of a
/// tool end, which a client opens on at once.
pub(crate) const READ_EVENTS: i64 = 500;

/// The event kind that opens a turn.
const PROMPT: &str = "user_prompt_submit";

/// The kinds of the events that open a step of the main lane: what a turn
/// cut by the read counts as its earlier steps.
const STEP_KINDS: &[&str] = &[
    "agent_message",
    "agent_thought",
    "pre_tool_use",
    "permission_request",
    "agent_request",
    "session.error",
    "compaction_update",
];

/// What one read took.
#[derive(Debug)]
pub(crate) struct Window {
    /// Oldest first.
    pub(crate) events: Vec<AgentEvent>,
    /// Whether events older than the window exist.
    pub(crate) has_older: bool,
    /// A turn larger than the read: its id, and how many steps the read left
    /// out of it before the events it holds.
    pub(crate) cut: Option<(String, u64)>,
}

/// The newest `turns` whole turns of a session below `before`, as many of
/// them as fit in `budget` events. Where the newest turn alone does not fit,
/// its prompt and its newest events.
pub(crate) async fn read_turns(
    store: &Store,
    session_id: &str,
    before: Option<&str>,
    turns: usize,
    budget: i64,
) -> Result<Window, StoreError> {
    let turns = turns.max(1);
    let starts = store
        .session_event_ids(session_id, PROMPT, before, turns as i64 + 1)
        .await?;
    // The candidate starts, the largest window first: with fewer turns than
    // asked for, the window starts at the session's first event.
    let mut candidates: Vec<Option<String>> = Vec::new();
    if starts.len() <= turns {
        candidates.push(None);
    }
    let kept = starts.len().min(turns);
    candidates.extend(starts[..kept].iter().rev().cloned().map(Some));
    for from in &candidates {
        let count = total(store, session_id, from.as_deref(), before).await?;
        if count <= budget as u64 {
            let events = store
                .list_session_window(session_id, from.as_deref(), before, budget)
                .await?;
            return Ok(Window {
                has_older: older(store, session_id, from.as_deref()).await?,
                events,
                cut: None,
            });
        }
    }
    read_range(
        store,
        session_id,
        starts.first().map(String::as_str),
        before,
        budget,
    )
    .await
}

/// One turn of a session: the events from `from` on and below `before`.
/// A turn larger than `budget` is read as its prompt and its newest events.
pub(crate) async fn read_range(
    store: &Store,
    session_id: &str,
    from: Option<&str>,
    before: Option<&str>,
    budget: i64,
) -> Result<Window, StoreError> {
    let has_older = older(store, session_id, from).await?;
    let mut tail = store
        .list_session_window(session_id, from, before, budget)
        .await?;
    let Some(from) = from else {
        return Ok(Window {
            events: tail,
            has_older,
            cut: None,
        });
    };
    if tail.first().is_some_and(|event| event.id == from) {
        return Ok(Window {
            events: tail,
            has_older,
            cut: None,
        });
    }
    // The read cut the turn: its prompt comes first, then the newest events.
    tail = store
        .list_session_window(session_id, Some(from), before, budget - 1)
        .await?;
    let first = tail.first().map(|event| event.id.clone());
    let cut = store
        .count_session_window(session_id, Some(from), first.as_deref())
        .await?
        .into_iter()
        .filter(|(kind, _)| STEP_KINDS.contains(&kind.as_str()))
        .map(|(_, count)| count)
        .sum();
    let mut events = Vec::with_capacity(tail.len() + 1);
    events.extend(store.get_event(from).await?);
    events.extend(tail);
    Ok(Window {
        events,
        has_older,
        cut: Some((from.to_string(), cut)),
    })
}

/// The bounds of the turn `turn_id`: its first event, and the prompt of the
/// next turn.
pub(crate) async fn turn_bounds(
    store: &Store,
    session_id: &str,
    turn_id: &str,
) -> Result<Option<String>, StoreError> {
    store
        .next_session_event_id(session_id, PROMPT, turn_id)
        .await
}

async fn total(
    store: &Store,
    session_id: &str,
    from: Option<&str>,
    before: Option<&str>,
) -> Result<u64, StoreError> {
    Ok(store
        .count_session_window(session_id, from, before)
        .await?
        .into_iter()
        .map(|(_, count)| count)
        .sum())
}

async fn older(store: &Store, session_id: &str, from: Option<&str>) -> Result<bool, StoreError> {
    match from {
        None => Ok(false),
        Some(from) => Ok(total(store, session_id, None, Some(from)).await? > 0),
    }
}

#[cfg(test)]
mod tests {
    use ariadne_store::{NewAgentEvent, Store};
    use serde_json::json;

    use super::read_turns;

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

    async fn commit(store: &Store, session: &str, kind: &str) -> String {
        store
            .create_event(NewAgentEvent {
                session_id: Some(session.into()),
                task_id: None,
                kind: kind.into(),
                payload: json!({"text": "x"}),
            })
            .await
            .unwrap()
            .id
    }

    /// A turn of a prompt and `events - 1` messages; answers its prompt's id.
    async fn turn(store: &Store, session: &str, events: usize) -> String {
        let prompt = commit(store, session, "user_prompt_submit").await;
        for _ in 1..events {
            commit(store, session, "agent_message").await;
        }
        prompt
    }

    /// An open of many turns reads whole turns up to the bound, and starts
    /// on a prompt; the turns it leaves out are older.
    #[tokio::test]
    async fn the_open_of_a_long_session_reads_a_bounded_count_of_whole_turns() {
        let (store, session, _dir) = session().await;
        let mut prompts = Vec::new();
        for _ in 0..12 {
            prompts.push(turn(&store, &session, 10).await);
        }

        let window = read_turns(&store, &session, None, 100, 35).await.unwrap();

        assert_eq!(
            window.events.len(),
            30,
            "three whole turns fit in 35 events"
        );
        assert_eq!(
            window.events[0].id, prompts[9],
            "the window starts on a prompt"
        );
        assert!(window.has_older);
        assert!(window.cut.is_none());
    }

    /// A turn larger than the bound is read as its prompt and its newest
    /// events, and the read counts the steps it left out.
    #[tokio::test]
    async fn a_turn_larger_than_the_bound_is_its_prompt_and_its_newest_events() {
        let (store, session, _dir) = session().await;
        turn(&store, &session, 3).await;
        let prompt = turn(&store, &session, 50).await;

        let window = read_turns(&store, &session, None, 100, 20).await.unwrap();

        assert_eq!(window.events.len(), 20, "the bound, and no more");
        assert_eq!(window.events[0].id, prompt, "the prompt first");
        assert_eq!(
            window.cut,
            Some((prompt, 30)),
            "the thirty messages left out"
        );
        assert!(window.has_older);
    }

    /// A page `before` a turn holds the turns older than it.
    #[tokio::test]
    async fn a_page_before_a_turn_holds_the_turns_older_than_it() {
        let (store, session, _dir) = session().await;
        let first = turn(&store, &session, 2).await;
        let second = turn(&store, &session, 2).await;
        let third = turn(&store, &session, 2).await;

        let window = read_turns(&store, &session, Some(&third), 1, 100)
            .await
            .unwrap();

        let ids: Vec<&str> = window.events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids[0], second);
        assert_eq!(ids.len(), 2);
        assert!(window.has_older, "the first turn is older still: {first}");
    }
}
