//! `GET /v1/attention`: the authoritative "Needs attention" list, and its
//! first producer, recovery.

use ariadne_api::attention::{AttentionCause, AttentionListDto, AttentionSubjectKind};
use ariadne_core::{Actor, AttentionReason, Seat, SessionStatus, TaskStatus};
use ariadne_store::NewSession;

use crate::common::test_pin;
use crate::common::{harness, with_forge};

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
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items.len(), 1);
    let item = &list.items[0];
    assert_eq!(item.cause, AttentionCause::Resource);
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
    for task_id in [&first.task.id, &second.id] {
        h.store
            .transition_task(
                task_id,
                TaskStatus::Failed,
                Actor::Daemon,
                Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
                None,
            )
            .await
            .unwrap();
    }

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

/// A task failed for an unrelated reason raises no resource item: the
/// words have to be the daemon's own descriptor-limit ones, not any
/// failure.
#[tokio::test]
async fn a_task_failed_for_another_reason_raises_no_resource_item() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some("the agent could not be started"),
            None,
        )
        .await
        .unwrap();

    let list: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(list.items, Vec::new());
}

/// An exhausted session on a model the catalog does not rank has no
/// automatic switch to try, so it is a `quota` item naming that session —
/// and the session's console is where it is answered.
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
    assert_eq!(item.cause, AttentionCause::Quota);
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Session);
    assert_eq!(item.affected[0].id, session.id);
    assert_eq!(
        item.target,
        ariadne_api::attention::AttentionTarget::Console {
            session_id: session.id.clone()
        }
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
    assert_eq!(item.cause, AttentionCause::Configuration);
    assert_eq!(item.summary, error);
    assert_eq!(item.affected.len(), 1);
    assert_eq!(item.affected[0].kind, AttentionSubjectKind::Repository);
    assert_eq!(item.affected[0].id, repo.id);
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
/// fresh one each time.
#[tokio::test]
async fn a_recovery_items_id_is_stable_across_two_reads() {
    let h = harness().await;
    let cast = h.cast().await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Failed,
            Actor::Daemon,
            Some(ariadne_daemon::scheduler::DESCRIPTOR_LIMIT_REASON),
            None,
        )
        .await
        .unwrap();

    let first: AttentionListDto = h.get("/v1/attention").await;
    let second: AttentionListDto = h.get("/v1/attention").await;
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].id, second.items[0].id);
}
