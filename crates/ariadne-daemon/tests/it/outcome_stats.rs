//! Integration tests for the outcome stats family (023): the `task_ended`
//! fact a task writes once it reaches `finished`, `cancelled` or `failed`,
//! and the `pick` fact a contested task's settled pick writes.

use crate::common;

use ariadne_api::stats::OutcomeStatsDto;
use ariadne_core::{Actor, MessageKind, Seat, TaskStatus};
use ariadne_store::{AgentPin, AgentSession, NewMessage, NewTask, NewTaskAgent, SessionFilter};

use common::{Harness, eventually, harness, test_pin};
use std::time::Duration;

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Walk a fresh task from `pending` straight to `in_progress`, with no agent
/// started: the sessions here are rows, and the moves are the ones the
/// daemon itself makes between them.
async fn start(h: &Harness, task_id: &str) {
    h.store
        .transition_task(task_id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(task_id, TaskStatus::InProgress, Actor::Daemon, None, None)
        .await
        .unwrap();
}

/// A task that finishes writes one `task_ended` fact: its status, its
/// author's model, and a lead time counted from its own creation.
#[tokio::test]
async fn a_finished_task_writes_one_task_ended_fact() {
    let h = harness().await;
    let cast = h.cast().await;
    start(&h, &cast.task.id).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::UnderReview,
            Actor::Author,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Approved,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Author,
            None,
            Some("abc123"),
        )
        .await
        .unwrap();

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    let fact = &facts[0];
    assert_eq!(fact.model.as_deref(), Some("stub:test-model"));
    assert_eq!(fact.seat.as_deref(), Some("author"));
    assert_eq!(fact.skills, ["coding"]);
    assert_eq!(fact.data["status"], "finished");
    assert_eq!(fact.data["authors"], 1);
    assert_eq!(fact.data["picked"], false);
    assert!(fact.data["lead_time_secs"].as_i64().unwrap() >= 0);
}

/// A task retried after it failed, and that fails again, writes a fact for
/// each of the two endings.
#[tokio::test]
async fn a_retried_task_that_fails_again_writes_two_facts() {
    let h = harness().await;
    let cast = h.cast().await;
    start(&h, &cast.task.id).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Author,
            Some("could not do it"),
            None,
        )
        .await
        .unwrap();
    // The user's own retry, back to the top of the walk.
    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::InProgress,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Author,
            Some("still could not do it"),
            None,
        )
        .await
        .unwrap();

    let facts = h.facts("task_ended").await;
    assert_eq!(facts.len(), 2, "{facts:?}");
    assert!(facts.iter().all(|f| f.data["status"] == "failed"));
}

/// The live author sessions of a task, in no particular order.
async fn live_authors(h: &Harness, task_id: &str) -> Vec<AgentSession> {
    h.store
        .list_sessions(SessionFilter {
            task_id: Some(task_id.to_string()),
            live_only: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .filter(|s| s.seat() == Some(Seat::Author))
        .collect()
}

/// A two-author task writes one `pick` fact once the reviewer settles it: the
/// winner's model, the loser's model, and the one reviewer that picked. The
/// outcomes stat then shows a contest entered for both models, and won for
/// the winner alone.
#[tokio::test]
async fn a_contested_pick_writes_one_fact_the_outcomes_stat_counts() {
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let repo = h.repository(&h.at("repo")).await;
    let goal = h.goal_on(&repo, test_pin()).await;
    let loser_pin = AgentPin {
        model: "stub:model-a".into(),
        effort: None,
    };
    let winner_pin = AgentPin {
        model: "stub:model-b".into(),
        effort: None,
    };
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Contested work".into(),
            description: "do things".into(),
            agents: vec![
                NewTaskAgent::new(Seat::Author, ["coding"], loser_pin.clone()),
                NewTaskAgent::new(Seat::Author, ["coding"], winner_pin.clone()),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
            ],
            depends_on: vec![],
        })
        .await
        .unwrap();
    let authors = h.store.list_task_authors(&task.id).await.unwrap();
    let loser = authors[0].clone();
    let winner = authors[1].clone();
    let reviewer = h
        .store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0);

    h.activate(&goal).await;
    h.notify(&task.id);
    eventually(TIMEOUT, "both authors to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && live_authors(&h, &task.id).await.len() == 2
    })
    .await;
    // The session the pick settles against: the landing that follows it
    // resumes the winner under its own session, which a lookup made after
    // the settlement would find instead of this one.
    let winner_session = live_authors(&h, &task.id)
        .await
        .into_iter()
        .find(|s| s.task_agent_id.as_deref() == Some(winner.id.as_str()))
        .expect("the winner's session");

    h.store
        .transition_task(&task.id, TaskStatus::UnderReview, Actor::Author, None, None)
        .await
        .unwrap();
    for author in [&loser, &winner] {
        h.store
            .send_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                kind: MessageKind::ReviewRequest,
                from_actor: Actor::Author,
                from_agent_id: Some(author.id.clone()),
                from_session: None,
                to_actor: Actor::Reviewer,
                to_agent_id: Some(reviewer.id.clone()),
                body: "an attempt".into(),
            })
            .await
            .unwrap();
        h.store
            .send_message(NewMessage {
                goal_id: task.goal_id.clone(),
                task_id: Some(task.id.clone()),
                kind: MessageKind::Approve,
                from_actor: Actor::Reviewer,
                from_agent_id: Some(reviewer.id.clone()),
                from_session: None,
                to_actor: Actor::Author,
                to_agent_id: Some(author.id.clone()),
                body: "judged".into(),
            })
            .await
            .unwrap();
    }

    h.store
        .record_pick(&task.id, &reviewer.id, &winner.id)
        .await
        .unwrap();
    h.notify(&task.id);
    eventually(TIMEOUT, "the pick to settle", async || {
        h.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Approved
    })
    .await;

    let facts = h.facts("pick").await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0].model.as_deref(), Some(winner_pin.model.as_str()));
    assert_eq!(
        facts[0].session_id.as_deref(),
        Some(winner_session.id.as_str())
    );
    assert_eq!(facts[0].launch_id, winner_session.launch_id);
    assert_eq!(facts[0].data["winner_model"], winner_pin.model);
    assert_eq!(
        facts[0].data["loser_models"],
        serde_json::json!([loser_pin.model])
    );
    assert_eq!(facts[0].data["reviewers"], 1);

    let outcomes: OutcomeStatsDto = h.get("/v1/stats/outcomes").await;
    let winner_row = outcomes
        .items
        .iter()
        .find(|row| row.model == winner_pin.model)
        .expect("the winner's row");
    assert_eq!(
        (winner_row.contests_entered, winner_row.contests_won),
        (1, 1)
    );
    let loser_row = outcomes
        .items
        .iter()
        .find(|row| row.model == loser_pin.model)
        .expect("the loser's row");
    assert_eq!((loser_row.contests_entered, loser_row.contests_won), (1, 0));
}
