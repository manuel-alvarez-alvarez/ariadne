//! What the API says a task's and a goal's agents run on.
//!
//! The pins live on `tasks`, `task_reviewers` and `goals`, and the launcher
//! spawns from them — but a surface that keeps reading the profile still shows
//! the wrong answer the moment that profile is edited, which is exactly when
//! the question gets asked. So these check the DTOs the CLI and the web read:
//! the engineer's pin, one pin per reviewer slot and the planner's, all of them
//! surviving a profile moved to another agent and another model.
//!
//! No tmux, no git, no agent CLI: nothing here launches anything.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::goals::GoalDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::{AgentKind, Role};
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{
    Goal, NewGoal, NewProfile, NewRepository, NewTask, Profile, ProfileUpdate, Store, Task,
};

struct Harness {
    store: Store,
    router: Router,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
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
        events: EventBus::new(),
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        dir,
    }
}

impl Harness {
    async fn profile(
        &self,
        name: &str,
        role: Role,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
    ) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind,
                model: model.map(str::to_string),
                system_prompt: format!("You are {name}."),
                prompts: vec![],
            })
            .await
            .unwrap()
    }

    /// Move a profile onto another agent CLI and another model, the edit every
    /// pin here has to survive.
    async fn move_profile(&self, id: &str, agent_kind: AgentKind, model: &str) {
        self.store
            .update_profile(
                id,
                ProfileUpdate {
                    agent_kind: Some(Some(agent_kind)),
                    model: Some(Some(model.into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> T {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }
}

/// A seeded goal and task, with the reviewer profiles the slots were cut from.
struct Seeded {
    goal: Goal,
    task: Task,
    strict: Profile,
    auto: Profile,
}

/// A goal and a task, each agent on a different agent CLI and model so no
/// assertion can pass by reading somebody else's pin. The reviewers are two:
/// one pinned to a model, one left on the agent's default and on auto.
async fn seeded(h: &Harness) -> Seeded {
    let planner = h
        .profile(
            "planner",
            Role::Planner,
            Some(AgentKind::ClaudeCode),
            Some("opus"),
        )
        .await;
    let engineer = h
        .profile(
            "engineer",
            Role::Engineer,
            Some(AgentKind::Codex),
            Some("gpt-5"),
        )
        .await;
    let strict = h
        .profile(
            "strict",
            Role::Reviewer,
            Some(AgentKind::ClaudeCode),
            Some("sonnet"),
        )
        .await;
    let auto = h.profile("auto", Role::Reviewer, None, None).await;

    // Never cloned into a worktree: nothing here reaches git.
    let repo = h
        .store
        .create_repository(NewRepository {
            path: h.dir.path().join("repo").display().to_string(),
            base_branch: "main".into(),
            description: None,
            merge_strategy: Default::default(),
        })
        .await
        .unwrap();
    let goal = h
        .store
        .create_goal(NewGoal {
            title: "Model switching".into(),
            description: String::new(),
            planner_profile_id: planner.id.clone(),
            max_tasks: None,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
        })
        .await
        .unwrap();
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "Surfaces".into(),
            description: String::new(),
            engineer_profile_id: engineer.id.clone(),
            reviewer_profile_ids: vec![strict.id.clone(), auto.id.clone()],
            depends_on: vec![],
        })
        .await
        .unwrap();

    // Every profile moves, after the goal and the task were created from them.
    h.move_profile(&planner.id, AgentKind::Opencode, "grok")
        .await;
    h.move_profile(&engineer.id, AgentKind::ClaudeCode, "haiku")
        .await;
    h.move_profile(&strict.id, AgentKind::Codex, "gpt-5-mini")
        .await;
    h.move_profile(&auto.id, AgentKind::Codex, "gpt-5-mini")
        .await;
    Seeded {
        goal,
        task,
        strict,
        auto,
    }
}

#[tokio::test]
async fn a_task_carries_the_engineer_pin_its_profile_no_longer_has() {
    let h = harness().await;
    let Seeded { task, .. } = seeded(&h).await;

    let dto: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert_eq!(dto.agent_kind, Some(AgentKind::Codex));
    assert_eq!(dto.model.as_deref(), Some("gpt-5"));
}

/// Each slot answers for itself: two reviewers moved onto the same agent and
/// model still read back as what each was assigned with, in review order.
#[tokio::test]
async fn a_task_carries_one_pin_per_reviewer_slot() {
    let h = harness().await;
    let Seeded {
        task, strict, auto, ..
    } = seeded(&h).await;

    let dto: TaskDto = h.get(&format!("/v1/tasks/{}", task.id)).await;
    let pins: Vec<_> = dto
        .reviewers
        .iter()
        .map(|r| (r.profile_id.as_str(), r.agent_kind, r.model.as_deref()))
        .collect();
    assert_eq!(
        pins,
        // The second was created on auto with no model, and auto is a pin
        // like any other: it stays auto, not the agent the profile moved to.
        vec![
            (
                strict.id.as_str(),
                Some(AgentKind::ClaudeCode),
                Some("sonnet")
            ),
            (auto.id.as_str(), None, None),
        ]
    );
}

#[tokio::test]
async fn a_goal_carries_the_planner_pin_its_profile_no_longer_has() {
    let h = harness().await;
    let Seeded { goal, .. } = seeded(&h).await;

    let dto: GoalDto = h.get(&format!("/v1/goals/{}", goal.id)).await;
    assert_eq!(dto.agent_kind, Some(AgentKind::ClaudeCode));
    assert_eq!(dto.model.as_deref(), Some("opus"));
}

/// The list is what the board reads, and it goes through the same conversion:
/// a pin that only the single-task route carried would be a hole in the UI.
#[tokio::test]
async fn the_task_list_carries_the_pins_too() {
    let h = harness().await;
    let Seeded { task, .. } = seeded(&h).await;

    let listed: Vec<TaskDto> = h.get("/v1/tasks").await;
    let found = listed.iter().find(|t| t.id == task.id).expect("the task");
    assert_eq!(found.agent_kind, Some(AgentKind::Codex));
    assert_eq!(found.model.as_deref(), Some("gpt-5"));
    assert_eq!(found.reviewers.len(), 2);
}
