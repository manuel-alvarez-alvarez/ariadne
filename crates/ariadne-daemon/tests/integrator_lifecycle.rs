//! The integrator's lifecycle on a local-only repository.
//!
//! The whole loop, driven by the scheduler over a real git repository: the
//! approvals hand the task to an integrator, which takes the branch over in a
//! worktree of its own — the engineer's is released, since a branch can only
//! be checked out once — lands it, and reports the merge. And the other way
//! out of it: a change the integrator will not land goes back to the engineer
//! as a round of requested changes, and comes round again once it is approved.
//!
//! No tmux and no agent CLI: `tmux` is a stub script that answers "no such
//! session" and records what it was asked for, so the sessions here are rows
//! and spawn plans rather than panes. `git` is real, and so is the merge the
//! daemon verifies before accepting it — the "integrator" doing the rebase,
//! the squash and the fast-forward is the test itself, running the commands
//! its briefing tells the agent to run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde::de::DeserializeOwned;
use tower::ServiceExt;

use ariadne_api::SESSION_HEADER;
use ariadne_api::reviews::ReviewDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{Actor, AgentKind, ReviewVerdict, Role, SessionStatus, TaskStatus};
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{
    AgentSession, NewGoal, NewProfile, NewRepository, NewReview, NewTask, SessionFilter, Store,
    Task,
};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

struct Harness {
    store: Store,
    router: Router,
    launcher: Arc<Launcher>,
    sched_tx: tokio::sync::mpsc::UnboundedSender<SchedEvent>,
    dir: tempfile::TempDir,
    _bus: EventBus,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    // No sleep inhibition: nothing here runs long enough to matter, and the
    // scheduler is the point.
    let sched_tx = scheduler::start(store.clone(), launcher.clone(), false);
    let state = AppState {
        store: store.clone(),
        started_at: std::time::Instant::now(),
        launcher: launcher.clone(),
        sched_tx: Some(sched_tx.clone()),
        events: bus.clone(),
        logs: LogBuffer::new(),
    };
    Harness {
        router: http::router(state),
        store,
        launcher,
        sched_tx,
        dir,
        _bus: bus,
    }
}

/// A `tmux` with no sessions that records every command it is given.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$1\" in\n\
         \x20 has-session) exit 1 ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("tmux-commands.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

/// Run a git (or shell) command in `dir`, failing the test if it does not.
fn sh(dir: &Path, cmd: &str) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "command failed in {}: {cmd}",
        dir.display()
    );
}

/// The same, for what a test reads back out of the repository.
fn out(dir: &Path, cmd: &str) -> String {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed in {}: {cmd}",
        dir.display()
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Wait for what the scheduler was supposed to do, rather than guessing at how
/// long a reconciliation takes.
async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

impl Harness {
    fn repo_path(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    /// A goal on a real repository, active, with one task on it and the
    /// built-in profiles behind it. Returns the task and the reviewer's
    /// profile id.
    async fn task(&self) -> (Task, String) {
        self.task_named("Render the board", &[]).await
    }

    async fn task_named(&self, title: &str, depends_on: &[String]) -> (Task, String) {
        let repo_path = self.repo_path();
        let repo_id = if repo_path.exists() {
            self.store.list_repositories().await.unwrap()[0].id.clone()
        } else {
            std::fs::create_dir_all(&repo_path).unwrap();
            sh(
                &repo_path,
                "git init -q -b main && echo v1 > file.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm 'chore: init'",
            );
            self.store
                .create_repository(NewRepository {
                    path: repo_path.display().to_string(),
                    base_branch: "main".into(),
                    description: None,
                })
                .await
                .unwrap()
                .id
        };

        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let integrator = self.profile("integrator", Role::Integrator).await;
        let goal = match self.store.list_goals(&[]).await.unwrap().first() {
            Some(goal) => goal.clone(),
            None => {
                let planner = self.profile("planner", Role::Planner).await;
                let goal = self
                    .store
                    .create_goal(NewGoal {
                        title: "Ship the board".into(),
                        description: "desc".into(),
                        planner_profile_id: planner,
                        max_tasks: None,
                        required_approvals: 1,
                        repository_ids: vec![repo_id.clone()],
                    })
                    .await
                    .unwrap();
                self.store
                    .set_goal_status(&goal.id, ariadne_core::GoalStatus::Active)
                    .await
                    .unwrap();
                self.store.get_goal(&goal.id).await.unwrap()
            }
        };

        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id,
                repo_id,
                title: title.into(),
                description: "Do the thing.".into(),
                engineer_profile_id: engineer,
                integrator_profile_id: integrator,
                reviewer_profile_ids: vec![reviewer.clone()],
                depends_on: depends_on.to_vec(),
            })
            .await
            .unwrap();
        (task, reviewer)
    }

    /// A profile of `role`, reused across the tasks of a test.
    async fn profile(&self, name: &str, role: Role) -> String {
        if let Ok(existing) = self.store.get_profile_by_name(name).await {
            return existing.id;
        }
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                // Pinned: the internal session id a resume needs is one the
                // Claude adapter chooses at spawn, so the resume paths here are
                // the ones a real session takes.
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                prompts: vec![],
            })
            .await
            .unwrap()
            .id
    }

    fn notify(&self, task_id: &str) {
        self.sched_tx
            .send(SchedEvent::TaskChanged(task_id.to_string()))
            .unwrap();
    }

    async fn status(&self, task_id: &str) -> TaskStatus {
        self.store.get_task(task_id).await.unwrap().status()
    }

    async fn sessions(&self, task_id: &str, role: Role) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
            .into_iter()
            .filter(|s| s.role() == role)
            .collect()
    }

    /// The session of `role` that is up on the task, if there is one.
    ///
    /// `running` rather than merely live: a row is created before its agent is
    /// launched, and a test that reads what an agent was started with has to
    /// wait for the launch that wrote it down.
    async fn live_session(&self, task_id: &str, role: Role) -> Option<AgentSession> {
        self.sessions(task_id, role)
            .await
            .into_iter()
            .find(|s| s.status() == SessionStatus::Running)
    }

    /// What the agent of `session_id` was launched with: the briefing, the
    /// resume instruction and all, as it travels in the spawn plan.
    fn launched_argv(&self, session_id: &str) -> String {
        let path = self
            .launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        SpawnPlanFile::from_json(&raw).unwrap().argv.join(" ")
    }

    /// Walk a fresh task to an integrator working on it: the engineer commits
    /// something, the reviewer approves, the scheduler does the rest. Returns
    /// the engineer's worktree (now released) and the integrator's session.
    async fn hand_to_the_integrator(&self, task: &Task, reviewer: &str) -> (PathBuf, AgentSession) {
        self.notify(&task.id);
        eventually("the engineer to be spawned", async || {
            self.status(&task.id).await == TaskStatus::InProgress
                && self.live_session(&task.id, Role::Engineer).await.is_some()
        })
        .await;

        let engineer_worktree = PathBuf::from(
            self.store
                .get_task(&task.id)
                .await
                .unwrap()
                .worktree_path
                .unwrap(),
        );
        sh(
            &engineer_worktree,
            "echo change > feature.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm 'wip: the change'",
        );
        self.approve(task, reviewer).await;

        eventually("the integrator to take the task over", async || {
            self.status(&task.id).await == TaskStatus::Integrating
                && self
                    .live_session(&task.id, Role::Integrator)
                    .await
                    .is_some()
        })
        .await;
        let integrator = self
            .live_session(&task.id, Role::Integrator)
            .await
            .expect("a live integrator session");
        (engineer_worktree, integrator)
    }

    /// The engineer asks for review and the reviewer approves it.
    async fn approve(&self, task: &Task, reviewer: &str) {
        let task = self
            .store
            .transition_task(
                &task.id,
                TaskStatus::UnderReview,
                Actor::Engineer,
                None,
                None,
            )
            .await
            .unwrap();
        self.store
            .create_review(NewReview {
                task_id: task.id.clone(),
                round: task.review_round,
                reviewer_profile_id: reviewer.to_string(),
                session_id: None,
                verdict: ReviewVerdict::Approve,
                body: Some("looks right".into()),
            })
            .await
            .unwrap();
        self.notify(&task.id);
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, body.to_vec())
    }

    async fn json<T: DeserializeOwned>(&self, request: Request<Body>, expected: StatusCode) -> T {
        let (status, body) = self.send(request).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }
}

/// A request from an agent session: the header the daemon reads its identity
/// from, exactly as the MCP server sends it.
fn as_session(uri: &str, session_id: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(SESSION_HEADER, session_id)
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// The whole local path: approvals reached, engineer released, integrator
/// spawned on the branch in a worktree of its own, rebase-squash-fast-forward,
/// `mark_merged` accepted, cleanup, dependents woken.
#[tokio::test]
async fn an_approved_task_is_landed_by_its_integrator() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let (dependent, _) = h
        .task_named(
            "Use what the first one built",
            std::slice::from_ref(&task.id),
        )
        .await;
    let (engineer_worktree, integrator) = h.hand_to_the_integrator(&task, &reviewer).await;

    // The engineer is out of the way: its session is over and its worktree —
    // which held the branch — is gone, off the task row as well as off disk.
    assert!(
        !engineer_worktree.exists(),
        "the engineer's worktree still holds the branch"
    );
    assert!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .is_none()
    );
    for session in h.sessions(&task.id, Role::Engineer).await {
        assert!(!session.status().is_live(), "an engineer is still running");
    }

    // The integrator has the branch, in a worktree named for its part.
    let integrator_worktree = PathBuf::from(integrator.worktree_path.clone().unwrap());
    assert!(
        integrator_worktree.ends_with(format!(
            "{}-int",
            task.id[task.id.len() - 8..].to_lowercase()
        )),
        "{}",
        integrator_worktree.display()
    );
    assert_eq!(
        out(&integrator_worktree, "git rev-parse --abbrev-ref HEAD"),
        task.branch
    );
    let argv = h.launched_argv(&integrator.id);
    assert!(
        argv.contains(&format!("# Integrate task: {}", task.title)),
        "the integration briefing is what it was started on: {argv}"
    );

    // What the briefing tells it to do, done: rebase, squash, fast-forward.
    sh(&integrator_worktree, "git rebase -q main");
    sh(
        &integrator_worktree,
        "git reset --soft main && \
         git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
    );
    let repo = h.repo_path();
    sh(&repo, &format!("git merge -q --ff-only {}", task.branch));
    let sha = out(&repo, "git rev-parse main");

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &integrator.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));

    // Cleanup takes the integrator's worktree with it, and the task that was
    // waiting on this one starts.
    eventually("the cleanup and the dependent task", async || {
        !integrator_worktree.exists()
            && matches!(
                h.status(&dependent.id).await,
                TaskStatus::Ready | TaskStatus::InProgress
            )
    })
    .await;
}

/// The other way out: a rebase the integrator will not resolve sends the task
/// back to the engineer as a round of requested changes — the branch with it —
/// and the next approval hands it to the very same integrator again.
#[tokio::test]
async fn a_send_back_returns_the_branch_and_the_task_to_the_engineer() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let (_engineer_worktree, integrator) = h.hand_to_the_integrator(&task, &reviewer).await;
    let integrator_worktree = PathBuf::from(integrator.worktree_path.clone().unwrap());

    let sent_back: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/return-to-engineer", task.id),
                &integrator.id,
                serde_json::json!({
                    "summary": "The rebase onto main conflicts.",
                    "changes": ["src/board.rs: reconcile the swimlane layout with main's"],
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(sent_back.status, TaskStatus::ChangesRequested);

    // The engineer reads it exactly as it reads a reviewer's change request:
    // a verdict on the round, and the same resume briefing.
    let reviews: Vec<ReviewDto> = h
        .json(
            Request::get(format!("/v1/tasks/{}/reviews", task.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    let feedback = reviews
        .iter()
        .find(|r| r.verdict == ReviewVerdict::RequestChanges)
        .expect("the send-back is a verdict on the round");
    assert_eq!(feedback.reviewer_profile_id, integrator.profile_id);
    let body = feedback.body.clone().unwrap();
    assert!(body.contains("The rebase onto main conflicts."), "{body}");
    assert!(body.contains("- src/board.rs: reconcile"), "{body}");

    eventually("the engineer to be resumed with the feedback", async || {
        engineer_is_back(&h, &task).await
    })
    .await;
    let engineer = h
        .live_session(&task.id, Role::Engineer)
        .await
        .expect("a live engineer");
    let argv = h.launched_argv(&engineer.id);
    assert!(
        argv.contains("src/board.rs: reconcile the swimlane layout"),
        "the integrator's list reached the engineer: {argv}"
    );
    assert!(
        argv.contains("integrator (integrator)"),
        "and it says who asked: {argv}"
    );

    // The branch went back with it: the integrator's worktree is gone and the
    // engineer has one it can commit in.
    assert!(
        !integrator_worktree.exists(),
        "the integrator still holds the branch"
    );
    let engineer_worktree = PathBuf::from(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .unwrap(),
    );
    sh(
        &engineer_worktree,
        "echo fixed > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'fix: reconcile it'",
    );

    // Round two: approved again, and the same integrator session picks it up
    // rather than a second one starting beside it.
    h.approve(&h.store.get_task(&task.id).await.unwrap(), &reviewer)
        .await;
    eventually("the integrator to be handed the task again", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.live_session(&task.id, Role::Integrator).await.is_some()
    })
    .await;
    let again = h
        .live_session(&task.id, Role::Integrator)
        .await
        .expect("a live integrator");
    assert_eq!(
        again.id, integrator.id,
        "the same integrator session, resumed"
    );
    assert_eq!(h.sessions(&task.id, Role::Integrator).await.len(), 1);
    assert!(
        PathBuf::from(again.worktree_path.clone().unwrap()).exists(),
        "its worktree was taken back from the engineer"
    );
    let argv = h.launched_argv(&again.id);
    assert!(
        argv.contains(&format!("Pick the integration of \"{}\"", task.title)),
        "resumed with the integration resume briefing: {argv}"
    );
}

/// The task is the engineer's again, in a session of its own that is running.
async fn engineer_is_back(h: &Harness, task: &Task) -> bool {
    h.status(&task.id).await == TaskStatus::InProgress
        && h.live_session(&task.id, Role::Engineer).await.is_some()
}

/// Only the integrator of the task, and only while it is being integrated: the
/// send-back is not a way for anyone else to reopen a task.
#[tokio::test]
async fn the_send_back_belongs_to_the_integrator_of_an_integrating_task() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;

    // Before the approvals there is nothing to send back — and the engineer
    // holding the task cannot do it either.
    h.notify(&task.id);
    eventually("the engineer to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/return-to-engineer", task.id),
            &engineer.id,
            serde_json::json!({"summary": "let me out", "changes": []}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );

    // The integrator can, once the task is its own — and not a second time,
    // since by then the task is the engineer's.
    let engineer_worktree = PathBuf::from(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .unwrap(),
    );
    sh(
        &engineer_worktree,
        "echo change > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'wip: the change'",
    );
    h.approve(&task, &reviewer).await;
    eventually("the integrator to take the task over", async || {
        h.live_session(&task.id, Role::Integrator).await.is_some()
    })
    .await;
    let integrator = h.live_session(&task.id, Role::Integrator).await.unwrap();
    let send_back = || {
        as_session(
            &format!("/v1/tasks/{}/return-to-engineer", task.id),
            &integrator.id,
            serde_json::json!({"summary": "the rebase conflicts", "changes": []}),
        )
    };
    let _: TaskDto = h.json(send_back(), StatusCode::OK).await;
    let (status, body) = h.send(send_back()).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        String::from_utf8_lossy(&body).contains("only a task being integrated"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

/// A task nobody has published still waits for its reviewers: a request for
/// review starts one, and the task sits under review until a verdict is in.
///
/// This is the other half of what a published request changes. There, the
/// people reading it are the round's reviewers and the reviewer profiles are
/// skipped; here there is no request, so the approvals are the only thing
/// that can hand the task to its integrator.
#[tokio::test]
async fn an_unpublished_task_still_waits_for_its_reviewers() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    h.notify(&task.id);
    eventually("the engineer to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let worktree = PathBuf::from(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .unwrap(),
    );
    sh(
        &worktree,
        "echo change > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'wip: the change'",
    );

    // Up for review, with nobody having judged it: a reviewer is started for
    // the round, and the task stays where it is.
    let under_review: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &h.live_session(&task.id, Role::Engineer).await.unwrap().id,
                serde_json::json!({"to": "under_review", "reason": "the board renders"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(under_review.status, TaskStatus::UnderReview);
    eventually("the reviewer to be started for the round", async || {
        h.live_session(&task.id, Role::Reviewer).await.is_some()
    })
    .await;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.status(&task.id).await,
        TaskStatus::UnderReview,
        "an unpublished task was approved without a verdict"
    );
    assert!(
        h.sessions(&task.id, Role::Integrator).await.is_empty(),
        "an integrator was handed a task no reviewer has approved"
    );

    // And the verdict is what moves it.
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: h.store.get_task(&task.id).await.unwrap().review_round,
            reviewer_profile_id: reviewer,
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: Some("looks right".into()),
        })
        .await
        .unwrap();
    h.notify(&task.id);
    eventually("the integrator to take the task over", async || {
        h.status(&task.id).await == TaskStatus::Integrating
    })
    .await;
}
