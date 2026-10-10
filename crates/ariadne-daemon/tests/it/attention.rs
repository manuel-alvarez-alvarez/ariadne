//! `GET /v1/attention`: the authoritative "Needs attention" list, and its
//! first producer, recovery.

use ariadne_api::attention::{
    AttentionCause, AttentionListDto, AttentionProducer, AttentionSubjectKind,
};
use ariadne_core::{Actor, AttentionReason, Seat, SessionStatus, TaskStatus};
use ariadne_store::NewSession;

use crate::common::test_pin;
use crate::common::{Harness, TIMEOUT, eventually, harness, with_forge};

/// Confirm, directly through the store's own persisted evidence, that this
/// goal's orchestrator has had a turn on exactly this task's current
/// failure — the same state `acp::serve_with_input` writes once a real
/// `Delivery::GoalAttention` turn ends. Setup for tests about causes other
/// than the orchestrator gate itself, which has its own dedicated tests
/// below (`a_failed_tasks_item_waits_for_its_orchestrator_to_actually_have_a_turn_on_it`
/// and its siblings).
async fn confirm_told(h: &Harness, goal_id: &str, task: &ariadne_store::Task) {
    h.store
        .confirm_goal_orchestrator_answered(goal_id, &[(task.id.clone(), task.updated_at.clone())])
        .await
        .unwrap();
}

/// Nothing stuck answers an empty, `complete` list, and the route is in the
/// API document under the `attention` tag.
#[tokio::test]
async fn an_empty_daemon_answers_an_empty_complete_list() {
    let h = harness().await;
    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
    assert!(list.complete);
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;
    assert_eq!(doc["paths"]["/v1/attention"]["get"]["tags"][0], "attention");
}

/// A task failed on the daemon's own descriptor-limit words is a `resource`
/// item naming that task, with the one action that clears it.
#[tokio::test]
async fn a_descriptor_limit_failure_is_a_resource_item() {
    let h = harness().await;
    let cast = h.cast().await;
    let failed = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &failed).await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.reason, AttentionCause::Resource);
    assert_eq!(
        item.summary,
        ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON
    );
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Task);
    assert_eq!(item.affected[0].id, cast.task.id);
}

/// Two tasks failed on the same descriptor-limit words are one item, not
/// two: the shared cause is one machine out of descriptors, and both tasks
/// are named in its `affected` list.
#[tokio::test]
async fn two_tasks_sharing_a_descriptor_limit_failure_are_one_grouped_item() {
    let h = harness().await;
    let first = h.cast().await;
    let second = h
        .task_on(&first.goal, &first.repo, "second", test_pin())
        .await;
    let mut failed_tasks = Vec::new();
    for task_id in [&first.task.id, &second.id] {
        let failed = h
            .store
            .transition_task(
                task_id,
                TaskStatus::Failed,
                Actor::Daemon,
                Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
                None,
            )
            .await
            .unwrap();
        failed_tasks.push((failed.id, failed.updated_at));
    }
    h.store
        .confirm_goal_orchestrator_answered(&first.goal.id, &failed_tasks)
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1, "{:?}", list.items);
    let affected: Vec<&str> = list.items[0]
        .affected
        .iter()
        .map(|subject| subject.id.as_str())
        .collect();
    assert_eq!(affected.len(), 2);
    assert!(affected.contains(&first.task.id.as_str()));
    assert!(affected.contains(&second.id.as_str()));
}

/// A task failed for an unrelated reason is not dropped: `failed` is
/// terminal, so the daemon will never retry it on its own, and it is its
/// own `unknown` item naming that one task, carrying the reason it
/// actually ended with.
#[tokio::test]
async fn a_task_failed_for_another_reason_is_its_own_unknown_item() {
    let h = harness().await;
    let cast = h.cast().await;
    let failed = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the agent could not be started"),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &failed).await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.reason, AttentionCause::Unknown);
    assert_eq!(item.summary, "the agent could not be started");
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].id, cast.task.id);
}

/// Two tasks failed for different, unrelated reasons stay two separate
/// `unknown` items: there is no reliable evidence they share a cause, so
/// they are never guessed into one group (009, "unknown causes remain
/// separate").
#[tokio::test]
async fn two_tasks_failed_for_different_reasons_stay_separate_unknown_items() {
    let h = harness().await;
    let first = h.cast().await;
    let second = h
        .task_on(&first.goal, &first.repo, "second", test_pin())
        .await;
    let failed_first = h
        .store
        .transition_task(
            &first.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the agent could not be started"),
            None,
        )
        .await
        .unwrap();
    let failed_second = h
        .store
        .transition_task(
            &second.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();
    h.store
        .confirm_goal_orchestrator_answered(
            &first.goal.id,
            &[
                (failed_first.id, failed_first.updated_at),
                (failed_second.id, failed_second.updated_at),
            ],
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 2, "{:?}", list.items);
    assert!(
        list.items
            .iter()
            .all(|item| item.reason == AttentionCause::Unknown)
    );
}

/// An exhausted session on a model the catalog does not rank has no
/// automatic switch to try, so it is a `quota` item naming that session —
/// and the session's console is where it is answered. Its summary names
/// how many switches it already spent and why the next one did not run.
#[tokio::test]
async fn an_exhausted_session_on_an_unranked_model_is_a_quota_item() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: "stub:no-such-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&session.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Exhausted)
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.producer, AttentionProducer::Recovery);
    assert_eq!(item.reason, AttentionCause::Quota);
    assert!(
        item.summary.contains("no other model is available"),
        "{}",
        item.summary
    );
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Session);
    assert_eq!(item.affected[0].id, session.id);
    // A task-less session's affected label names its seat and its own
    // switch count, not the model — the model is already the grouping key.
    assert_eq!(item.affected[0].label, "orchestrator (0 switch(es))");
    assert_eq!(
        item.target,
        ariadne_api::attention::AttentionTarget::Console {
            session_id: session.id.clone()
        }
    );
}

/// An exhausted session's own task names the affected entry, not a second
/// copy of the model, so a user can tell two sessions on the same model
/// apart by the work each was running.
#[tokio::test]
async fn a_quota_items_affected_entry_names_the_sessions_task() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: Some(cast.task.id.clone()),
            seat: Some(Seat::Agent),
            task_agent_id: Some(cast.agents[0].id.clone()),
            model: "stub:no-such-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&session.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Exhausted)
        .await
        .unwrap();
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    assert_eq!(
        list.items[0].affected[0].label,
        format!("{} (0 switch(es))", cast.task.id)
    );
}

/// A session nobody is waiting on — one whose task moved past the column it
/// ran — raises no quota item even while exhausted, the same rule 009 gives
/// every other attention reason (`attention::work_is_active`).
#[tokio::test]
async fn an_exhausted_session_nobody_is_waiting_on_raises_no_quota_item() {
    let h = harness().await;
    let cast = h.cast().await;
    let session = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: Some(cast.task.id.clone()),
            seat: Some(Seat::Agent),
            task_agent_id: Some(cast.agents[0].id.clone()),
            model: "stub:no-such-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&session.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Exhausted)
        .await
        .unwrap();
    // The task never moved to `in_progress` on this agent's column, so the
    // session's work is nobody's business any more.

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
}

/// An enabled forge integration whose last fetch failed because its CLI is
/// not on the daemon's PATH is a `configuration` item naming the
/// repository, pointing at its forge settings.
#[tokio::test]
async fn a_missing_forge_cli_is_a_configuration_item() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge(&h, &repo).await;
    let error = "`gh` is not installed on the daemon's PATH; install it, or set gh_bin \
                 in config.toml";
    h.store
        .set_forge_fetch_error(&repo.id, Some(error))
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.reason, AttentionCause::Configuration);
    assert_eq!(item.summary, error);
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Repository);
    assert_eq!(item.affected[0].id, repo.id);
}

/// A forge fetch error the daemon's own reactive sign-in check confirmed
/// is the forge CLI being signed out is an `access` item naming the host:
/// signing back in is the fix wherever that host's repositories are.
#[tokio::test]
async fn a_signed_out_forge_cli_is_an_access_item() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge(&h, &repo).await;
    let error = format!(
        "{}: gh: HTTP 401: Bad credentials",
        ariadne_daemon::forge::poll::FORGE_SIGNED_OUT
    );
    h.store
        .set_forge_fetch_error(&repo.id, Some(&error))
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.reason, AttentionCause::Access);
    assert!(item.summary.contains("github.com"), "{}", item.summary);
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Repository);
    assert_eq!(item.affected[0].id, repo.id);
}

/// End to end, through the real forge poll worker and a scripted CLI
/// rather than a fetch error written in by hand: a `pr list` that fails
/// while `auth status` also fails is read by `forge/poll.rs` as the CLI
/// being signed out, confirmed reactively rather than guessed from the
/// list error alone, and reaches `GET /v1/attention` as an `access` item.
#[tokio::test]
async fn a_forge_cli_signed_out_mid_fetch_is_confirmed_reactively_and_reaches_the_list() {
    use crate::common::eventually;
    use crate::common::forge::{answer, stub_forge_cli};

    let stub = stub_forge_cli(serde_json::json!([
        answer(&["pr", "list"], 1, ""),
        {
            "args": ["api", "user"],
            "exit": 1,
            "stderr": "gh: HTTP 401: Bad credentials (https://api.github.com/user)",
        },
    ]));
    let h = harness().forge_cli(&stub).await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge(&h, &repo).await;
    h.state.forge_poll.wake(&repo.id);

    eventually(
        crate::common::TIMEOUT,
        "the access item to be raised",
        || async {
            let list: AttentionListDto = h.get("/v1/attention").await;
            list.items
                .iter()
                .any(|item| item.reason == AttentionCause::Access)
        },
    )
    .await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    let item = list
        .items
        .iter()
        .find(|item| item.reason == AttentionCause::Access)
        .unwrap();
    assert!(item.summary.contains("github.com"), "{}", item.summary);
    assert_eq!(item.affected[0].id, repo.id);
}

/// A forge fetch error that does not name a missing CLI raises nothing:
/// `forge/poll.rs` keeps retrying it forever with no budget to spend, so
/// there is no evidence here that recovery has given up — only that it is
/// still trying, which this list never raises for (009).
#[tokio::test]
async fn a_forge_fetch_error_that_names_no_missing_cli_raises_nothing() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    with_forge(&h, &repo).await;
    h.store
        .set_forge_fetch_error(&repo.id, Some("gh: rate limit exceeded, try again later"))
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
}

/// A disabled integration's fetch error is nobody's business: a user who
/// turned the integration off is not asking the daemon to fix it.
#[tokio::test]
async fn a_disabled_forge_integrations_fetch_error_raises_nothing() {
    let h = harness().await;
    let repo = h.repository(&h.git_repo("repo")).await;
    crate::common::with_forge_enabled(&h, &repo, false).await;
    h.store
        .set_forge_fetch_error(
            &repo.id,
            Some("`gh` is not installed on the daemon's PATH; install it, or set gh_bin in config.toml"),
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
}

/// The same recovery item answers the same id across two reads: a client
/// polling, or reconnecting after a restart, sees one row rather than a
/// fresh one each time. The id is derived purely from the shared cause
/// (the model, the fixed descriptor-limit reason, the forge CLI name) and
/// never from a counter or a timestamp, so it does not depend on anything
/// a daemon restart would lose.
#[tokio::test]
async fn a_recovery_items_id_is_stable_across_two_reads() {
    let h = harness().await;
    let cast = h.cast().await;
    let failed = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &failed).await;

    let first: AttentionListDto = h.get("/v1/attention").await;
    let second: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, second.items[0].id);
    assert_eq!(first.items[0].id, "recovery:resource:descriptor-limit");
}

/// An exhausted session already switched to a successor raises no quota
/// item: recovery already acted, and the row left flagged is not the one
/// still waiting on a person.
#[tokio::test]
async fn an_exhausted_session_already_switched_raises_no_quota_item() {
    let h = harness().await;
    let cast = h.cast().await;
    let old = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: "stub:no-such-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&old.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&old.id, AttentionReason::Exhausted)
        .await
        .unwrap();
    h.store
        .create_switched_session(
            NewSession {
                goal_id: Some(cast.goal.id.clone()),
                task_id: None,
                seat: Some(Seat::Orchestrator),
                task_agent_id: None,
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: None,
                pull_request_id: None,
            },
            &old.id,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
}

/// End to end, through the real scheduler and a stub agent rather than a
/// hand-written store poke: a failed task raises nothing while its
/// orchestrator's turn on it is still running, and raises its item only
/// once that turn actually ends. The stub's own turn is held open on
/// `wait_for` for exactly this window, so the read taken while it is still
/// running proves suppression during the gap between delivery and
/// completion — not merely before delivery and after completion, which a
/// stamp taken at hand-off time would also have passed.
#[tokio::test]
async fn a_failed_tasks_item_stays_suppressed_while_its_orchestrators_turn_on_it_is_still_running()
{
    let h = harness().scheduler().await;
    let gate = h.dir.path().join("goal-attention-turn");
    let mut script = crate::common::acp::script();
    script["prompts"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "updates": [],
            "wait_for": gate.display().to_string(),
            "stop_reason": "end_turn",
        }));
    h.agent.reprogram(script);

    let cast = h.cast().await;
    // `tell_orchestrator` is only reached once the goal is active (009
    // rule 4) — planning only nudges a running orchestrator, it never
    // tells one of a failed task.
    h.store
        .set_goal_status(&cast.goal.id, ariadne_core::GoalStatus::Active)
        .await
        .unwrap();
    let orchestrator = async || {
        h.sessions_of_goal(&cast.goal.id)
            .await
            .into_iter()
            .find(|s| s.seat() == Some(Seat::Orchestrator))
    };
    eventually(TIMEOUT, "the orchestrator to be launched", || async {
        orchestrator().await.is_some()
    })
    .await;
    eventually(TIMEOUT, "the orchestrator's first turn to end", || async {
        orchestrator()
            .await
            .is_some_and(|s| s.status() == SessionStatus::Idle)
    })
    .await;

    let before: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(before.items, Vec::new(), "not told yet, so still trying");

    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    // The prompt was delivered — the turn it started is running — but has
    // not ended: this is exactly the window a stamp taken at hand-off time
    // would already have satisfied.
    eventually(
        TIMEOUT,
        "the orchestrator's turn on the failure to start",
        || async {
            orchestrator()
                .await
                .is_some_and(|s| s.status() == SessionStatus::Running)
        },
    )
    .await;
    let during: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        during.items,
        Vec::new(),
        "mid-turn, so still trying: {:?}",
        during.items
    );

    std::fs::write(&gate, "go").unwrap();

    eventually(
        TIMEOUT,
        "the item to appear once the turn actually ends",
        || async {
            let list: AttentionListDto = h.get("/v1/attention").await;
            list.items.len() == 1
        },
    )
    .await;
}

/// An unrelated turn — one carrying no `Delivery::GoalAttention` at all —
/// confirms nothing when it ends, end to end through the real scheduler
/// and a real console turn: the confirmed-list column stays untouched by
/// it. The loose "any stop on this session promotes whatever is pending"
/// read this list once took would have confirmed a failure the turn never
/// carried; this one cannot, because what is written comes from the
/// delivery that completed, not from the session's ambient status.
#[tokio::test]
async fn an_unrelated_turn_confirms_nothing_it_never_carried() {
    let h = harness().scheduler().await;
    let cast = h.cast().await;
    h.store
        .set_goal_status(&cast.goal.id, ariadne_core::GoalStatus::Active)
        .await
        .unwrap();
    let orchestrator = async || {
        h.sessions_of_goal(&cast.goal.id)
            .await
            .into_iter()
            .find(|s| s.seat() == Some(Seat::Orchestrator))
    };
    eventually(TIMEOUT, "the orchestrator to be launched", || async {
        orchestrator().await.is_some()
    })
    .await;
    eventually(TIMEOUT, "the orchestrator's first turn to end", || async {
        orchestrator()
            .await
            .is_some_and(|s| s.status() == SessionStatus::Idle)
    })
    .await;
    let session = orchestrator().await.unwrap();

    // A console message, not a daemon delivery: its turn carries no
    // `Delivery::GoalAttention`, whatever it ends with.
    let (status, _) = h
        .send(crate::common::post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            serde_json::json!({"text": "hello"}),
        ))
        .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the unrelated turn to end", || async {
        orchestrator()
            .await
            .is_some_and(|s| s.status() == SessionStatus::Idle)
    })
    .await;

    let goal = h.store.get_goal(&cast.goal.id).await.unwrap();
    assert!(
        goal.orchestrator_answered_failed_task_ids.is_none(),
        "an unrelated turn confirmed something: {:?}",
        goal.orchestrator_answered_failed_task_ids
    );
}

/// Confirmation is read by this task's own id, never by whether the goal
/// has *any* confirmed turn at all: a task the orchestrator was told
/// nothing about stays out, the same grace every other untold failure
/// gets, even where the goal's orchestrator has, since being told, had a
/// turn confirming a *different* failure. A looser read — any confirmed
/// turn since the goal's last tell answers for every failure it currently
/// has — would raise this one regardless.
#[tokio::test]
async fn a_failed_tasks_item_still_waits_while_only_a_different_failure_was_confirmed() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: test_pin().model,
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    // A different task's failure was told and confirmed — this task's
    // never was.
    h.store
        .confirm_goal_orchestrator_answered(
            &cast.goal.id,
            &[(
                "01OTHERTASKXXXXXXXXXXXXXXX".into(),
                "2026-01-01T00:00:00Z".into(),
            )],
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "never named, so still trying: {:?}",
        list.items
    );
}

/// A confirmed task id does not survive its own task's retry: once told,
/// confirmed and retried off `failed`, a *second* failure of the same
/// task is a new occurrence — its `updated_at` moved — and the old
/// confirmation, still naming the old one, does not answer for it.
#[tokio::test]
async fn an_old_confirmation_does_not_answer_for_a_tasks_second_failure() {
    let h = harness().await;
    let cast = h.cast().await;
    let first_failure = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &first_failure).await;
    let confirmed: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(confirmed.items.len(), 1, "the first failure is confirmed");

    // Retried off `failed` — the orchestrator's own answer to it — and
    // failed again. The second failure stamps a new `updated_at`.
    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();
    let second_failure = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass again"),
            None,
        )
        .await
        .unwrap();
    assert_ne!(
        first_failure.updated_at, second_failure.updated_at,
        "the retry must have moved the stamp for this to prove anything"
    );

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "the old confirmation must not answer for the new failure: {:?}",
        list.items
    );
}

/// An orchestrated goal with no orchestrator session yet — the moment
/// before `keep_orchestrator`'s first launch — still has automatic
/// recovery coming: a failed task of that goal raises nothing. Absence of
/// a session at all is not evidence of a give-up any more than absence of
/// a *live* one is; only `Goal::orchestrator_given_up_at` is.
#[tokio::test]
async fn a_failed_task_raises_nothing_while_its_goal_awaits_its_orchestrators_first_launch() {
    let h = harness().await;
    let cast = h.cast().await;
    // No orchestrator session exists for this goal at all.
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "the orchestrator's first launch may still be coming: {:?}",
        list.items
    );
}

/// A goal whose orchestrator crashed and has no live session this instant,
/// but has not been given up on, still has automatic recovery coming
/// (`keep_orchestrator`'s own budget, `scheduler::goals`) — so a failed
/// task of that goal raises nothing yet. No *live* orchestrator is not the
/// same as nothing left to try it.
#[tokio::test]
async fn a_failed_task_raises_nothing_while_its_dead_orchestrator_has_not_been_given_up_on() {
    let h = harness().await;
    let cast = h.cast().await;
    let orchestrator = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: test_pin().model,
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status(&orchestrator.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new(), "a relaunch may still be coming");
}

/// The crash flag alone is not a give-up: `retire_disconnected` raises
/// `AttentionReason::Disconnected` on any crash with work still active,
/// well before the spawn-retry budget is spent, and the liveness sweep
/// may still resolve it on its own. A failed task of that goal must not
/// read that flag as proof recovery gave up — only
/// `Goal::orchestrator_given_up_at`, the scheduler's own exhausted-budget
/// decision, is read for that.
#[tokio::test]
async fn a_failed_task_raises_nothing_while_its_orchestrator_merely_crashed_without_giving_up() {
    let h = harness().await;
    let cast = h.cast().await;
    let orchestrator = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: test_pin().model,
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status(&orchestrator.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .set_session_attention(&orchestrator.id, AttentionReason::Disconnected)
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "a crash is not a give-up: {:?}",
        list.items
    );
}

/// Once `scheduler::goals::orchestrator_could_not_start` has actually
/// given up — the spawn-retry budget spent, `Goal::orchestrator_given_up_at`
/// stamped — a failed task of that goal raises its own item, same as
/// always, *and* the orchestrator's own give-up raises a second,
/// independent item of its own: a task that failed for its own reason and
/// an orchestrator that will not start are two different problems with
/// two different actions, and the task's own item carries no "resume the
/// orchestrator" action for the second one to be dropped in favour of.
#[tokio::test]
async fn a_failed_task_raises_its_item_once_its_orchestrator_is_given_up_on() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: test_pin().model,
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_goal_orchestrator_given_up(&cast.goal.id)
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the tests did not pass"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 2, "{:?}", list.items);
    assert!(
        list.items
            .iter()
            .any(|item| item.affected.iter().any(|a| a.id == cast.task.id)),
        "the task's own item names it: {:?}",
        list.items
    );
    assert!(
        list.items
            .iter()
            .any(|item| item.required_action.contains("resume it")),
        "the orchestrator's own item names its own action: {:?}",
        list.items
    );
}

/// A cancelled goal no longer needs the orchestrator its give-up item was
/// about: the item excludes it, rather than carrying a "resume it"
/// action for work the user has already called off. The mark itself is
/// left as it stood — a later read of the goal's own history still finds
/// it — only the active item is excluded.
#[tokio::test]
async fn a_cancelled_goals_orchestrator_give_up_raises_nothing() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: test_pin().model,
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_goal_orchestrator_given_up(&cast.goal.id)
        .await
        .unwrap();
    h.store
        .set_goal_status(&cast.goal.id, ariadne_core::GoalStatus::Cancelled)
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "a cancelled goal needs no orchestrator any more: {:?}",
        list.items
    );
    assert!(
        h.store
            .get_goal(&cast.goal.id)
            .await
            .unwrap()
            .orchestrator_given_up_at
            .is_some(),
        "the mark itself is history, not deleted"
    );
}

/// A request still open and still asking for review, but whose
/// repository lost its configured review pin, no longer wants a reviewer
/// session either — the same thing `scheduler::pull_requests::end_review`
/// reads to take its session down — and its earlier give-up mark is
/// cleared with it: nothing is trying to start that session any more, so
/// there is nothing left to have given up on.
#[tokio::test]
async fn a_request_whose_review_pin_is_gone_clears_its_reviewers_give_up() {
    let h = harness().scheduler().await;
    let repo = h.repository(&h.at("widgets")).await;
    h.store
        .set_forge_integration(ariadne_store::SetForgeIntegration {
            repository_id: repo.id.clone(),
            kind: ariadne_core::ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some("me".into()),
            review_model: Some(test_pin().model),
            review_effort: None,
        })
        .await
        .unwrap();
    let (pull, _) = h
        .store
        .upsert_pull_request(ariadne_store::NewPullRequest {
            repository_id: repo.id.clone(),
            number: 1,
            url: "https://github.com/acme/widgets/pull/1".into(),
            role: "reviewer".into(),
            origin_task_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_pull_request_reviewer_given_up(&pull.id)
        .await
        .unwrap();
    // Still open, still asking — but the repository's review pin is gone,
    // so nothing wants a reviewer session for it any more either.
    crate::common::forge::seed_live(&h, &pull, "someone", "fix-1", "abc", true);
    h.store
        .set_forge_integration(ariadne_store::SetForgeIntegration {
            repository_id: repo.id.clone(),
            kind: ariadne_core::ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled: true,
            login: Some("me".into()),
            review_model: None,
            review_effort: None,
        })
        .await
        .unwrap();

    h.state.forge_poll.wake(&repo.id);
    h.state
        .sched_tx
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::PullRequestChanged(
            pull.id.clone(),
        ))
        .unwrap();

    eventually(
        TIMEOUT,
        "the reviewer's give-up mark to be cleared",
        || async {
            h.store
                .get_pull_request(&pull.id)
                .await
                .unwrap()
                .reviewer_given_up_at
                .is_none()
        },
    )
    .await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(
        list.items,
        Vec::new(),
        "nothing is trying to start that session any more: {:?}",
        list.items
    );
}

/// A reviewer session's affected label names its seat, the same as an
/// orchestrator's: neither ran a task of its own.
#[tokio::test]
async fn a_quota_items_affected_entry_names_a_reviewer_session_by_seat() {
    let h = harness().await;
    let cast = h.cast().await;
    let (pull, _) = h
        .store
        .upsert_pull_request(ariadne_store::NewPullRequest {
            repository_id: cast.repo.id.clone(),
            number: 1,
            url: "https://github.com/acme/widgets/pull/1".into(),
            role: "reviewer".into(),
            origin_task_id: None,
        })
        .await
        .unwrap();
    crate::common::forge::seed_live(&h, &pull, "someone", "fix-1", "abc", true);
    let session = h
        .store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: Some(Seat::Reviewer),
            task_agent_id: None,
            model: "stub:no-such-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: Some(pull.id.clone()),
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&session.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Exhausted)
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].affected[0].label, "reviewer (0 switch(es))");
}

/// Two sessions exhausted on the same model for two different reasons —
/// one out of switch budget, one out of candidate models — stay two
/// separate items rather than one that could only report one reason and
/// one required action.
#[tokio::test]
async fn two_sessions_on_the_same_model_blocked_for_different_reasons_stay_separate_items() {
    let h = harness().await;
    let cast = h.cast().await;
    let budget_spent = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: "stub:shared-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&budget_spent.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&budget_spent.id, AttentionReason::Exhausted)
        .await
        .unwrap();
    for n in 0..3 {
        h.store
            .create_event(ariadne_store::NewAgentEvent {
                session_id: Some(budget_spent.id.clone()),
                task_id: None,
                kind: "session.switched".into(),
                payload: serde_json::json!({"reason": "exhausted", "n": n}),
            })
            .await
            .unwrap();
    }

    let second = h
        .store
        .create_session(NewSession {
            goal_id: Some(cast.goal.id.clone()),
            task_id: Some(cast.task.id.clone()),
            seat: Some(Seat::Agent),
            task_agent_id: Some(cast.agents[0].id.clone()),
            model: "stub:shared-model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    h.store
        .set_session_status_if_live(&second.id, SessionStatus::Exited, None)
        .await
        .unwrap();
    h.store
        .set_session_attention(&second.id, AttentionReason::Exhausted)
        .await
        .unwrap();
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 2, "{:?}", list.items);
    let budget_item = list
        .items
        .iter()
        .find(|item| item.summary.contains("spent its automatic switch budget"))
        .expect("the budget-spent session has its own item");
    assert_eq!(budget_item.affected[0].id, budget_spent.id);
    let candidate_item = list
        .items
        .iter()
        .find(|item| item.summary.contains("no other model is available"))
        .expect("the no-candidate session has its own item");
    assert_eq!(candidate_item.affected[0].id, second.id);
}

/// Retrying a failed task — the orchestrator's or the user's own answer to
/// it — takes its item down: the task is no longer `failed`, so nothing
/// here names it any more. The transition that recorded the failure is
/// kept all the same: resolving an item is not expected to erase why it
/// was ever raised.
#[tokio::test]
async fn retrying_a_failed_task_removes_its_item_without_losing_the_transition() {
    let h = harness().await;
    let cast = h.cast().await;
    let failed = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &failed).await;
    let before: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(before.items.len(), 1);

    h.store
        .transition_task(&cast.task.id, TaskStatus::Ready, Actor::User, None, None)
        .await
        .unwrap();

    let after: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(after.items, Vec::new());

    let transitions = h.store.list_task_transitions(&cast.task.id).await.unwrap();
    assert!(
        transitions
            .iter()
            .any(|t| t.to_status == TaskStatus::Failed.as_str()
                && t.reason.as_deref() == Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON)),
        "{transitions:?}"
    );
}

/// The same item answers with the same id from a second, independent
/// store connection opened on the same database file — the daemon's own
/// restart opens a fresh connection pool too, and carries none of the
/// first one's in-memory state with it, so an id that survives this
/// survives that.
#[tokio::test]
async fn a_recovery_items_id_is_stable_across_a_fresh_store_connection() {
    let h = harness().await;
    let cast = h.cast().await;
    let failed = h
        .store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();
    confirm_told(&h, &cast.goal.id, &failed).await;

    let first = ariadne_daemon::attention::collect(&h.store, &h.launcher).await;
    let reopened = ariadne_store::Store::open(h.dir.path().join("test.db"))
        .await
        .unwrap();
    let second = ariadne_daemon::attention::collect(&reopened, &h.launcher).await;

    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, second.items[0].id);
}
