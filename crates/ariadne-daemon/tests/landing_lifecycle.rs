//! What an approved task does, driven by the scheduler over a real git
//! repository.
//!
//! The approvals leave the task with the engineer that wrote it: the same
//! session, the same worktree, briefed with the landing instructions its
//! repository's merge strategy names. From there it has two ways out, and
//! both are the engineer's own — `mark_merged` once the change is on the base
//! branch, and `request_review` for a revision the people on a published
//! request asked for, which the Ariadne reviewers judge like any other round.
//!
//! No tmux and no agent CLI: `tmux` is a stub script that answers "no such
//! session" and records what it was asked for, so the sessions here are rows
//! and spawn plans rather than panes. `git` is real, and so is the merge the
//! daemon verifies before accepting it — the agent doing the rebase, the
//! squash and the fast-forward is the test itself, running the commands its
//! briefing tells the agent to run.

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
use ariadne_core::{
    Actor, AgentKind, MergeStrategy, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{
    AgentSession, NewGoal, NewProfile, NewRepository, NewReview, NewSession, NewTask,
    SessionFilter, Store, Task,
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

    /// Switch the repository over to publishing, which changes what the
    /// engineer is briefed to do and what the daemon will accept as a merge.
    async fn publish_instead(&self) {
        let repo = self.store.list_repositories().await.unwrap().remove(0);
        self.store
            .update_repository(
                &repo.id,
                ariadne_store::RepositoryUpdate {
                    merge_strategy: Some(MergeStrategy::PullRequest),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
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
                    merge_strategy: MergeStrategy::Direct,
                })
                .await
                .unwrap()
                .id
        };

        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
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
                system_prompt: Some(format!("You are {name}.")),
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

    /// Walk a fresh task to the engineer landing it: the engineer commits
    /// something, the reviewer approves, the scheduler does the rest. Returns
    /// the engineer's worktree — which it never gave up — and the session that
    /// has been briefed to land the change.
    async fn walk_to_approved(&self, task: &Task, reviewer: &str) -> (PathBuf, AgentSession) {
        self.notify(&task.id);
        eventually("the engineer to be spawned", async || {
            self.status(&task.id).await == TaskStatus::InProgress
                && self.live_session(&task.id, Role::Engineer).await.is_some()
        })
        .await;
        let writing = self
            .live_session(&task.id, Role::Engineer)
            .await
            .expect("a live engineer session");

        let worktree = PathBuf::from(
            self.store
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
        self.approve(task, reviewer).await;

        eventually("the engineer to be briefed to land it", async || {
            self.status(&task.id).await == TaskStatus::Approved
                && self
                    .live_session(&task.id, Role::Engineer)
                    .await
                    .is_some_and(|s| {
                        self.launched_argv(&s.id)
                            .contains(&format!("# Land task: {}", task.title))
                    })
        })
        .await;
        let landing = self
            .live_session(&task.id, Role::Engineer)
            .await
            .expect("a live engineer session");
        assert_eq!(
            landing.id, writing.id,
            "the session that wrote the change is the one landing it"
        );
        (worktree, landing)
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

/// The whole of it, the way `direct` says: the approvals leave the task with
/// its engineer — same session, same worktree, briefed to land it —
/// rebase-squash-fast-forward, `mark_merged` accepted, cleanup, dependents
/// woken.
#[tokio::test]
async fn an_approved_task_is_landed_by_its_own_engineer() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let (dependent, _) = h
        .task_named(
            "Use what the first one built",
            std::slice::from_ref(&task.id),
        )
        .await;
    let (worktree, engineer) = h.walk_to_approved(&task, &reviewer).await;

    // Nobody took the branch: the worktree the change was written in is still
    // the task's, still on the branch, and still on disk.
    assert!(worktree.exists(), "the engineer lost its worktree");
    assert_eq!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .worktree_path
            .as_deref(),
        Some(worktree.display().to_string().as_str())
    );
    assert_eq!(
        out(&worktree, "git rev-parse --abbrev-ref HEAD"),
        task.branch
    );

    // And the briefing it was picked up with names the strategy it is to
    // follow, rather than leaving it to guess.
    let argv = h.launched_argv(&engineer.id);
    assert!(
        argv.contains("merge strategy is **direct**"),
        "the landing briefing does not name the repository's strategy: {argv}"
    );

    // What the briefing tells it to do, done: rebase, squash, fast-forward.
    sh(&worktree, "git rebase -q main");
    sh(
        &worktree,
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
                &engineer.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));

    // Cleanup takes the worktree with it, and the task that was waiting on
    // this one starts.
    eventually("the cleanup and the dependent task", async || {
        !worktree.exists()
            && matches!(
                h.status(&dependent.id).await,
                TaskStatus::Ready | TaskStatus::InProgress
            )
    })
    .await;
}

/// A merge nobody made is refused: the daemon checks the branch really is on
/// the base branch of the primary checkout before it believes the sha.
#[tokio::test]
async fn a_merge_that_never_happened_is_refused() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let (worktree, engineer) = h.walk_to_approved(&task, &reviewer).await;

    let sha = out(&worktree, "git rev-parse HEAD");
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &engineer.id,
            serde_json::json!({"to": "merged", "merge_commit": sha}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let message = String::from_utf8_lossy(&body);
    assert!(message.contains("merge not verified"), "{message}");
    assert_eq!(h.status(&task.id).await, TaskStatus::Approved);
}

/// The other way out of `approved`: the people reading a published request
/// asked for something, the engineer made it, and the revision goes back to
/// the Ariadne reviewers like any other round — from `approved`, which is
/// where a task being landed sits.
#[tokio::test]
async fn a_revision_of_a_published_request_goes_back_to_the_reviewers() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let (_worktree, engineer) = h.walk_to_approved(&task, &reviewer).await;

    // The request it published is recorded by the engineer, and only by it.
    const URL: &str = "https://github.com/owner/repo/pull/12";
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &engineer.id,
                serde_json::json!({"url": URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_url.as_deref(), Some(URL));

    // The URL and nothing else: telling the user where the request is belongs
    // to the engineer that opened it, so recording one writes no message of
    // the daemon's own into the thread.
    assert!(
        h.store
            .list_task_messages(&task.id, None, 100)
            .await
            .unwrap()
            .is_empty(),
        "recording a request wrote a message into the thread"
    );
    let reviewer_session = h
        .store
        .create_session(NewSession {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Reviewer,
            profile_id: reviewer.clone(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            tmux_session: "ariadne-test-rev".into(),
            worktree_path: None,
            review_round: Some(1),
        })
        .await
        .unwrap();
    let (status, refusal) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &reviewer_session.id,
            serde_json::json!({"url": URL}),
        ))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only its engineer records it"
    );
    let refusal = String::from_utf8_lossy(&refusal);
    assert!(refusal.contains("only the engineer"), "{refusal}");

    // And the revision it made for them is reviewed like any other round.
    let round = h.store.get_task(&task.id).await.unwrap().review_round;
    let revised: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &engineer.id,
                serde_json::json!({
                    "to": "under_review",
                    "reason": "answered every comment on the request",
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(revised.status, TaskStatus::UnderReview);
    assert_eq!(revised.review_round, round + 1);
    assert_eq!(
        revised.pr_url.as_deref(),
        Some(URL),
        "the request it is a revision of is still the task's"
    );

    // The reviewers judge it, and the approval hands it back to the engineer
    // to finish landing.
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: revised.review_round,
            reviewer_profile_id: reviewer.clone(),
            session_id: None,
            verdict: ReviewVerdict::Approve,
            body: Some("the answers read right".into()),
        })
        .await
        .unwrap();
    h.notify(&task.id);
    eventually("the task to come back to its engineer", async || {
        h.status(&task.id).await == TaskStatus::Approved
    })
    .await;

    // One round of verdicts per reviewer, both rounds readable.
    let reviews: Vec<ReviewDto> = h
        .json(
            Request::get(format!("/v1/tasks/{}/reviews", task.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    assert_eq!(reviews.len(), 2, "{reviews:?}");
    assert!(reviews.iter().all(|r| r.reviewer_profile_id == reviewer));
}

/// A request the forge squashed leaves no branch on the base at all, so what
/// the daemon checks there is the other half of the engineer's last step: the
/// sha it reports is on the base branch of the primary checkout.
#[tokio::test]
async fn a_squashed_request_lands_on_the_sha_the_engineer_fast_forwarded_to() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    h.publish_instead().await;
    let (worktree, engineer) = h.walk_to_approved(&task, &reviewer).await;

    // The briefing says which half of the procedure applies.
    assert!(
        h.launched_argv(&engineer.id)
            .contains("merge strategy is **pull_request**"),
        "the engineer was not briefed to publish it"
    );

    // What a squash merge on the forge leaves behind, reproduced with git: a
    // commit on the base that no branch points at, and a task branch that is
    // not its ancestor.
    let repo = h.repo_path();
    sh(
        &repo,
        &format!(
            "git merge -q --squash {} && \
             git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it (#12)'",
            task.branch
        ),
    );
    let sha = out(&repo, "git rev-parse main");
    assert_ne!(sha, out(&worktree, "git rev-parse HEAD"));

    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &engineer.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(sha.as_str()));
}

/// And a sha that is not on the base branch is still refused, which is what
/// keeps the reported one worth anything.
#[tokio::test]
async fn a_published_task_cannot_report_a_sha_the_base_branch_has_never_seen() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    h.publish_instead().await;
    let (worktree, engineer) = h.walk_to_approved(&task, &reviewer).await;

    // The tip of the branch: real, and nowhere near the base branch.
    let sha = out(&worktree, "git rev-parse HEAD");
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &engineer.id,
            serde_json::json!({"to": "merged", "merge_commit": sha}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal = String::from_utf8_lossy(&body);
    assert!(refusal.contains("merge not verified"), "{refusal}");
    assert_eq!(h.status(&task.id).await, TaskStatus::Approved);
}
