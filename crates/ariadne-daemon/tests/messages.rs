//! Integration tests for addressing a conversation message.
//!
//! A message may name one addressee, the way a task names its profiles: a
//! profile id, a profile name, or the literal `"user"`. What each thread
//! accepts is who works in it — the planner in a goal's planning thread, and
//! the engineer, the reviewers and the planner in a task's — so that a
//! message never names someone who is not there to read it. Anything else is refused
//! with a sentence naming the addressees that would have worked.
//!
//! No tmux and no agent CLI: nothing here launches anything, the rows are
//! seeded through the store and only the message endpoints are exercised.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::error::ErrorBody;
use ariadne_api::messages::MessageDto;
use ariadne_core::{AgentKind, AuthorRole, RecipientKind, Role};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{Goal, NewGoal, NewProfile, NewRepository, NewTask, Profile, Store, Task};

struct Harness {
    store: Store,
    router: Router,
    #[allow(dead_code)]
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: TmuxManager::default(),
        git: GitManager,
    });
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher,
        sched_tx: None,
        events: bus,
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        dir,
    }
}

/// The people of one goal with one task: everyone a thread of it can address.
struct Cast {
    goal: Goal,
    task: Task,
    planner: Profile,
    engineer: Profile,
    reviewer: Profile,
}

impl Harness {
    async fn profile(&self, name: &str, role: Role) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: Some(format!("You are {name}.")),
            })
            .await
            .unwrap()
    }

    async fn cast(&self) -> Cast {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        // Named by one test and refused there: an addressee is checked
        // against the thread, not against the profiles that exist.
        self.profile("outsider", Role::Reviewer).await;
        let repo = self
            .store
            .create_repository(NewRepository {
                path: "/tmp/ariadne-messages-test".into(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Goal".into(),
                description: "desc".into(),
                planner_profile_id: planner.id.clone(),
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: self.store.list_goal_repositories(&goal.id).await.unwrap()[0]
                    .id
                    .clone(),
                title: "Task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id.clone(),
                reviewer_profile_ids: vec![reviewer.id.clone()],
                depends_on: vec![],
            })
            .await
            .unwrap();
        Cast {
            goal,
            task,
            planner,
            engineer,
            reviewer,
        }
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    /// Send a request expected to succeed with `expected` and decode its body.
    async fn json<T: DeserializeOwned>(&self, request: Request<Body>, expected: StatusCode) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }

    /// Post a message, expecting it to be accepted.
    async fn post_message(&self, uri: &str, body: serde_json::Value) -> MessageDto {
        self.json(post_json(uri, body), StatusCode::CREATED).await
    }

    /// Post a message, expecting the addressee to be refused.
    async fn refused(&self, uri: &str, body: serde_json::Value) -> String {
        let envelope: ErrorBody = self
            .json(post_json(uri, body), StatusCode::BAD_REQUEST)
            .await;
        assert_eq!(envelope.error.code, "invalid_request");
        envelope.error.message
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn post_json(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// A profile is addressed by name or by id, and the resolved addressee comes
/// back on the message — with the profile's name, so a client renders it
/// without a lookup of its own.
#[tokio::test]
async fn a_task_message_addresses_a_participant_by_name_or_by_id() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);

    let by_name = h
        .post_message(
            &uri,
            serde_json::json!({"body": "have a look", "to": "reviewer"}),
        )
        .await;
    let recipient = by_name.recipient.expect("addressed");
    assert_eq!(recipient.kind, RecipientKind::Profile);
    assert_eq!(
        recipient.profile_id.as_deref(),
        Some(cast.reviewer.id.as_str())
    );
    assert_eq!(recipient.profile_name.as_deref(), Some("reviewer"));

    let by_id = h
        .post_message(
            &uri,
            serde_json::json!({"body": "back to you", "to": cast.engineer.id}),
        )
        .await;
    let recipient = by_id.recipient.expect("addressed");
    assert_eq!(
        recipient.profile_id.as_deref(),
        Some(cast.engineer.id.as_str())
    );
    assert_eq!(recipient.profile_name.as_deref(), Some("engineer"));

    // The planner of the goal takes part in every one of its task threads.
    let to_planner = h
        .post_message(
            &uri,
            serde_json::json!({"body": "blocked", "to": "planner"}),
        )
        .await;
    assert_eq!(
        to_planner.recipient.and_then(|r| r.profile_id).as_deref(),
        Some(cast.planner.id.as_str())
    );

    // The user is addressed by the literal, and carries no profile.
    let to_user = h
        .post_message(
            &uri,
            serde_json::json!({"body": "a question", "to": "user"}),
        )
        .await;
    let recipient = to_user.recipient.expect("addressed");
    assert_eq!(recipient.kind, RecipientKind::User);
    assert_eq!(recipient.profile_id, None);
    assert_eq!(recipient.profile_name, None);

    // And saying nothing addresses the thread.
    let unaddressed = h
        .post_message(&uri, serde_json::json!({"body": "thinking out loud"}))
        .await;
    assert!(unaddressed.recipient.is_none());

    // Every one of them reads back the same way from the thread.
    let thread: Vec<MessageDto> = h.json(get(&uri), StatusCode::OK).await;
    assert_eq!(
        thread
            .iter()
            .map(|m| m
                .recipient
                .as_ref()
                .map(|r| (r.kind, r.profile_name.as_deref())))
            .collect::<Vec<_>>(),
        vec![
            Some((RecipientKind::Profile, Some("reviewer"))),
            Some((RecipientKind::Profile, Some("engineer"))),
            Some((RecipientKind::Profile, Some("planner"))),
            Some((RecipientKind::User, None)),
            None,
        ]
    );
}

/// A task thread reaches the people working on the task. A profile that is
/// none of them is refused, and the refusal names the ones that would work.
#[tokio::test]
async fn a_task_thread_refuses_a_profile_that_takes_no_part_in_it() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);

    let message = h
        .refused(&uri, serde_json::json!({"body": "psst", "to": "outsider"}))
        .await;
    assert_eq!(
        message,
        "outsider takes no part in this thread; address one of: \
         engineer, reviewer, planner, user"
    );
}

/// The planning thread is the planner's: the agents of a task are addressed in
/// that task's thread, where which task is meant is not in question.
#[tokio::test]
async fn a_goal_thread_addresses_only_its_planner_or_the_user() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/goals/{}/messages", cast.goal.id);

    let to_planner = h
        .post_message(
            &uri,
            serde_json::json!({"body": "how is it going", "to": "planner"}),
        )
        .await;
    assert_eq!(
        to_planner.recipient.and_then(|r| r.profile_id).as_deref(),
        Some(cast.planner.id.as_str())
    );
    let to_user = h
        .post_message(
            &uri,
            serde_json::json!({"body": "a question", "to": "user"}),
        )
        .await;
    assert_eq!(to_user.recipient.map(|r| r.kind), Some(RecipientKind::User));

    let message = h
        .refused(
            &uri,
            serde_json::json!({"body": "start please", "to": "engineer"}),
        )
        .await;
    assert_eq!(
        message,
        "engineer takes no part in this thread; address one of: planner, user"
    );
}

/// An addressee no profile answers to is a mistake in the caller, not a
/// message quietly posted to nobody.
#[tokio::test]
async fn an_unknown_addressee_is_refused_naming_the_ones_that_would_work() {
    let h = harness().await;
    let cast = h.cast().await;

    let message = h
        .refused(
            &format!("/v1/tasks/{}/messages", cast.task.id),
            serde_json::json!({"body": "hello?", "to": "nobody"}),
        )
        .await;
    assert_eq!(
        message,
        "no profile has the id or name nobody; address one of: \
         engineer, reviewer, planner, user"
    );
}

/// A task the user cancels says so in its own thread, addressed to them: an
/// ending is the one moment there is no agent left to notice it.
#[tokio::test]
async fn a_cancelled_task_tells_the_user_it_ended_and_why() {
    let h = harness().await;
    let cast = h.cast().await;
    let uri = format!("/v1/tasks/{}/messages", cast.task.id);

    let _: serde_json::Value = h
        .json(
            post_json(
                &format!("/v1/tasks/{}/cancel", cast.task.id),
                serde_json::json!({}),
            ),
            StatusCode::OK,
        )
        .await;

    let thread: Vec<MessageDto> = h.json(get(&uri), StatusCode::OK).await;
    let told: Vec<&MessageDto> = thread
        .iter()
        .filter(|m| {
            m.recipient
                .as_ref()
                .is_some_and(|r| r.kind == RecipientKind::User)
        })
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].author_role, AuthorRole::System);
    assert!(told[0].body.contains(&cast.task.title), "{}", told[0].body);
    assert!(
        told[0].body.contains("cancelled by user"),
        "the notice does not carry the reason: {}",
        told[0].body
    );
}
