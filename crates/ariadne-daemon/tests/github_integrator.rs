//! The GitHub integrator's lifecycle: publish, watch, send back, finish.
//!
//! The same loop as `integrator_lifecycle`, on a repository that is published
//! to a forge instead of landed on the spot. The task branch becomes a pull
//! request, the daemon watches it while humans review it, and what they do to
//! it decides what happens next: an approval is announced to the user once, a
//! comment is written back to the engineer as a round of requested changes,
//! and the merge wakes the integrator to finish the task off the base branch.
//!
//! No tmux and no agent CLI, as in that test — and no GitHub either: `gh` is
//! a stub script that prints the pull request a test wants it to see and
//! records what it was asked. The "integrator" doing the recording and the
//! merging is the test itself, calling the endpoints its briefing tells the
//! agent to call.

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
use ariadne_api::messages::MessageDto;
use ariadne_api::tasks::TaskDto;
use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{
    Actor, AgentKind, AttentionReason, RecipientKind, ReviewVerdict, Role, SessionStatus,
    TaskStatus,
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
    AgentSession, NewGoal, NewProfile, NewRepository, NewReview, NewTask, ProfileUpdate,
    SessionFilter, Store, Task,
};

/// How long a test waits for the scheduler to reach a state.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The pull request every test in here publishes.
const PR_URL: &str = "https://github.com/ariadne/ariadne/pull/12";

/// What the engineer answers the humans with, and what has to reach them
/// unchanged: the summary of the `request_review` that closes a published
/// round is a reply to every comment on the request.
const REPLIES: &str = "Reply to @jon on src/board.rs:42: it allocates once per lane now.\n\
                       Reply to @maria on src/lane.rs:7: renamed to `lane_index`.\n\
                       Reply to @maria: the module stays — it is what makes the lane testable.";

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
    let mut cfg = Config::load(Some(dir.path().join("home"))).unwrap();
    cfg.gh_bin = write_gh_stub(dir.path());
    // Every reconciliation is a poll: what the interval is for is sparing
    // GitHub, and there is no GitHub here.
    cfg.pr_poll_secs = 0;
    let launcher = Arc::new(Launcher {
        cfg: Arc::new(cfg),
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
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

/// A `gh` that answers `pr view` and `api` with whatever JSON the test last
/// wrote, and writes down everything it was asked. No file means no pull
/// request, which is what `gh` itself does about one: a failure on stderr;
/// no review comments file means the empty answer a pull request nobody has
/// written on the diff of gets.
fn write_gh_stub(dir: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("gh-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$*\" in\n\
         \x20 *'pr view'*)\n\
         \x20   if [ -f '{pr}' ]; then cat '{pr}'; else echo 'no pull requests found' >&2; exit 1; fi ;;\n\
         \x20 *api*)\n\
         \x20   if [ -f '{comments}' ]; then cat '{comments}'; else echo '[]'; fi ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("gh-commands.log").display(),
        pr = dir.join("pr.json").display(),
        comments = dir.join("review-comments.json").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin.display().to_string()
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

    /// A goal on a real repository, active, with one task on it landed by the
    /// built-in Integrator. Returns the task and the reviewer's id.
    async fn task(&self) -> (Task, String) {
        let repo_path = self.repo_path();
        std::fs::create_dir_all(&repo_path).unwrap();
        sh(
            &repo_path,
            "git init -q -b main && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm 'chore: init'",
        );
        let repo_id = self
            .store
            .create_repository(NewRepository {
                path: repo_path.display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap()
            .id;

        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let integrator = self.integrator().await;
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

        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id,
                repo_id,
                title: "Render the board".into(),
                description: "Do the thing.".into(),
                engineer_profile_id: engineer,
                integrator_profile_id: integrator,
                reviewer_profile_ids: vec![reviewer.clone()],
                depends_on: vec![],
            })
            .await
            .unwrap();
        (task, reviewer)
    }

    /// The built-in Integrator itself — its prompts are what is under test —
    /// pinned to an agent CLI so that the resume paths here are the ones a
    /// real session takes (the internal session id a resume needs is the
    /// Claude adapter's, chosen at spawn).
    async fn integrator(&self) -> String {
        let profile = self.store.get_profile_by_name("Integrator").await.unwrap();
        self.store
            .update_profile(
                &profile.id,
                ProfileUpdate {
                    agent_kind: Some(Some(AgentKind::ClaudeCode)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        profile.id
    }

    async fn profile(&self, name: &str, role: Role) -> String {
        if let Ok(existing) = self.store.get_profile_by_name(name).await {
            return existing.id;
        }
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
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

    /// What the stub `gh` will answer the next poll with.
    fn pull_request(&self, json: serde_json::Value) {
        std::fs::write(
            self.dir.path().join("pr.json"),
            serde_json::to_string(&json).unwrap(),
        )
        .unwrap();
    }

    /// What the stub `gh api` will answer the next poll with, verbatim: the
    /// pages of review comments as `--paginate` writes them.
    fn review_comments(&self, pages: &str) {
        std::fs::write(self.dir.path().join("review-comments.json"), pages).unwrap();
    }

    /// Everything `gh` has been asked, one invocation per line.
    fn gh_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("gh-commands.log")).unwrap_or_default()
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

    async fn live_session(&self, task_id: &str, role: Role) -> Option<AgentSession> {
        self.sessions(task_id, role)
            .await
            .into_iter()
            .find(|s| s.status() == SessionStatus::Running)
    }

    /// The agent has finished its turn: what the daemon acts on a moved pull
    /// request against is an idle integrator, never one mid-turn.
    async fn goes_idle(&self, session_id: &str) {
        self.store
            .set_session_status(session_id, SessionStatus::Idle)
            .await
            .unwrap();
    }

    /// What the agent of `session_id` was launched with, briefing and all.
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

    /// The instruction alone, without the system prompt every launch carries
    /// beside it: the last of the argv, which is where the adapters put what
    /// the agent is being woken for.
    fn resume_instruction(&self, session_id: &str) -> String {
        let path = self
            .launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        SpawnPlanFile::from_json(&raw)
            .unwrap()
            .argv
            .last()
            .cloned()
            .unwrap_or_default()
    }

    /// The engineer's `request_review`, made the way its MCP tool makes it:
    /// the summary is posted to the thread first — which is where everything
    /// downstream reads it — and then the task goes up for review.
    async fn request_review(&self, task_id: &str, engineer: &str, summary: &str) {
        let _: MessageDto = self
            .json(
                as_session(
                    &format!("/v1/tasks/{task_id}/messages"),
                    engineer,
                    serde_json::json!({"body": format!("Review requested: {summary}")}),
                ),
                StatusCode::CREATED,
            )
            .await;
        let _: TaskDto = self
            .json(
                as_session(
                    &format!("/v1/tasks/{task_id}/transitions"),
                    engineer,
                    serde_json::json!({"to": "under_review", "reason": summary}),
                ),
                StatusCode::OK,
            )
            .await;
    }

    /// What the integrator does at the end of that round: one message to the
    /// user, carrying what the engineer answered.
    async fn post_to_user(&self, task_id: &str, session_id: &str, body: &str) {
        let _: MessageDto = self
            .json(
                as_session(
                    &format!("/v1/tasks/{task_id}/messages"),
                    session_id,
                    serde_json::json!({"body": body, "to": "user"}),
                ),
                StatusCode::CREATED,
            )
            .await;
    }

    /// Walk a fresh task to its integrator working on it, exactly as the
    /// local lifecycle does: the engineer commits, the reviewer approves.
    async fn hand_to_the_integrator(&self, task: &Task, reviewer: &str) -> AgentSession {
        self.notify(&task.id);
        eventually("the engineer to be spawned", async || {
            self.status(&task.id).await == TaskStatus::InProgress
                && self.live_session(&task.id, Role::Engineer).await.is_some()
        })
        .await;
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
        eventually("the integrator to take the task over", async || {
            self.status(&task.id).await == TaskStatus::Integrating
                && self
                    .live_session(&task.id, Role::Integrator)
                    .await
                    .is_some()
        })
        .await;
        self.live_session(&task.id, Role::Integrator)
            .await
            .expect("a live integrator session")
    }

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

    /// Everything written in the task's conversation.
    async fn thread_messages(&self, task_id: &str) -> Vec<MessageDto> {
        self.json(
            Request::get(format!("/v1/tasks/{task_id}/messages?limit=100"))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await
    }

    /// The messages of the task thread that are addressed to the user.
    async fn user_messages(&self, task_id: &str) -> Vec<MessageDto> {
        self.thread_messages(task_id)
            .await
            .into_iter()
            .filter(|m| {
                m.recipient
                    .as_ref()
                    .is_some_and(|r| r.kind == RecipientKind::User)
            })
            .collect()
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

/// An open pull request with nothing on it, in the shape `gh pr view --json`
/// answers with: green, mergeable, and open against `main` at the head every
/// check and conflict on it is keyed by.
fn open_pull_request() -> serde_json::Value {
    serde_json::json!({
        "number": 12,
        "state": "OPEN",
        "mergedAt": null,
        "mergeCommit": null,
        "reviewDecision": "REVIEW_REQUIRED",
        "reviews": [],
        "comments": [],
        "statusCheckRollup": [],
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "baseRefName": "main",
        "headRefOid": HEAD,
    })
}

/// The commit the pull request is open at, until a test pushes a revision.
const HEAD: &str = "abc123";

/// One check run GitHub reports as failed, in the shape the rollup carries an
/// Actions job in.
fn failing_check(name: &str) -> serde_json::Value {
    serde_json::json!({
        "__typename": "CheckRun",
        "name": name,
        "status": "COMPLETED",
        "conclusion": "FAILURE",
        "startedAt": "2026-08-24T09:55:00Z",
        "completedAt": "2026-08-24T09:58:12Z",
        "detailsUrl": format!("https://github.com/ariadne/ariadne/actions/runs/17/job/{name}"),
        "workflowName": "CI",
    })
}

/// An approved pull request whose checks are red at `head`.
fn red_pull_request(head: &str) -> serde_json::Value {
    let mut pr = open_pull_request();
    pr["reviewDecision"] = "APPROVED".into();
    pr["headRefOid"] = head.into();
    pr["statusCheckRollup"] = serde_json::json!([failing_check("test")]);
    pr
}

/// The whole GitHub path: the integrator is briefed to publish rather than to
/// land, records the pull request it opened, and from there the daemon
/// watches it — an approval told to the user once, a comment relayed to the
/// engineer once, and the merge finished off the base branch with a
/// `mark_merged` no ancestor check would have accepted.
#[tokio::test]
async fn a_pull_request_is_watched_from_publication_to_its_merge() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;

    // It was briefed to publish, not to land.
    let argv = h.launched_argv(&integrator.id);
    for expected in [
        "Publish it as a pull request",
        "gh auth status",
        "gh pr create",
        "record_pull_request",
        "land the task locally instead",
    ] {
        assert!(argv.contains(expected), "the briefing has no {expected}");
    }

    // Nothing is asked of GitHub before there is a pull request to ask about.
    h.notify(&task.id);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        h.gh_log(),
        "",
        "gh was called for a task with no pull request"
    );

    // The integrator opens it and reports it. That is the end of its turn.
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_number, Some(12));
    assert_eq!(published.pr_url.as_deref(), Some(PR_URL));

    // Recording it is what tells the user there is one, rather than anything
    // the agent has to remember to say: nothing in Ariadne merges a pull
    // request, so the person who does hears about it as it is opened.
    let opened = h.user_messages(&task.id).await;
    assert_eq!(opened.len(), 1);
    assert!(opened[0].body.contains(PR_URL), "{}", opened[0].body);
    assert!(
        opened[0].body.contains("Pull request #12 is open"),
        "{}",
        opened[0].body
    );
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingInput),
        "and it goes up on the attention strip the user reads it from"
    );
    h.goes_idle(&integrator.id).await;

    // An untouched pull request wakes nobody: the integrator is left idle
    // rather than nudged for not having landed anything, and the review it
    // is still waiting on is not news to tell the user twice.
    h.pull_request(open_pull_request());
    h.notify(&task.id);
    eventually("the pull request to be polled", async || {
        // By URL, which names the repository as well as the number.
        h.gh_log().contains(&format!("pr view {PR_URL}"))
    })
    .await;
    assert_eq!(h.user_messages(&task.id).await.len(), 1);
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "an integrator waiting on humans is not a stalled task"
    );

    // Approved: the user is told once that merging it is theirs to do.
    let mut approved = open_pull_request();
    approved["reviewDecision"] = "APPROVED".into();
    approved["reviews"] = serde_json::json!([{
        "id": "R1", "author": {"login": "maria"}, "body": "", "state": "APPROVED",
    }]);
    h.pull_request(approved);
    h.notify(&task.id);
    eventually(
        "the user to be told the pull request is ready",
        async || h.user_messages(&task.id).await.len() > 1,
    )
    .await;
    let notice = h.user_messages(&task.id).await;
    assert_eq!(notice.len(), 2);
    assert!(notice[1].body.contains(PR_URL), "{}", notice[1].body);
    assert!(
        notice[1].body.contains("ready for you to merge"),
        "{}",
        notice[1].body
    );
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason(),
        Some(AttentionReason::WaitingInput),
        "and it is on the attention strip the user reads it from"
    );
    // Polled again and again, it is still those two.
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(h.user_messages(&task.id).await.len(), 2);

    // The message is said once; the flag is put back as often as it takes.
    // An agent's own events take attention down as it works — which is what
    // happened to every flag raised while the integrator was still publishing
    // — so a poll that finds the request open, unmerged and nobody's but the
    // user's raises it again.
    h.store
        .clear_session_attention(&integrator.id)
        .await
        .unwrap();
    h.notify(&task.id);
    eventually("the attention flag to go back up", async || {
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason()
            == Some(AttentionReason::WaitingInput)
    })
    .await;
    assert_eq!(
        h.user_messages(&task.id).await.len(),
        2,
        "and no message goes with it"
    );

    // Merging is not the integrator's to claim while GitHub says otherwise.
    let branch_tip = out(&h.repo_path(), &format!("git rev-parse {}", task.branch));
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &integrator.id,
            serde_json::json!({"to": "merged", "merge_commit": branch_tip}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        String::from_utf8_lossy(&body).contains("not merged"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    // Merged on GitHub as a squash: a commit the local base does not contain
    // yet, and no ancestor of the task branch anywhere. Reporting it before
    // the local base has caught up is refused too — a task is landed here as
    // well as there.
    let repo = h.repo_path();
    let mut merged_elsewhere = open_pull_request();
    merged_elsewhere["state"] = "MERGED".into();
    merged_elsewhere["mergedAt"] = "2026-08-24T10:00:00Z".into();
    merged_elsewhere["mergeCommit"] = serde_json::json!({"oid": branch_tip});
    h.pull_request(merged_elsewhere);
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &integrator.id,
            serde_json::json!({"to": "merged", "merge_commit": branch_tip}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        String::from_utf8_lossy(&body).contains("does not contain yet"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    sh(
        &repo,
        &format!(
            "git merge --squash -q {branch} && \
             git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
            branch = task.branch
        ),
    );
    let squash = out(&repo, "git rev-parse main");
    assert!(
        !GitManager
            .is_ancestor(&repo, &task.branch, "main")
            .await
            .unwrap(),
        "a squash merge leaves the branch out of the base, which is the point"
    );
    let mut merged = open_pull_request();
    merged["state"] = "MERGED".into();
    merged["mergedAt"] = "2026-08-24T10:00:00Z".into();
    merged["mergeCommit"] = serde_json::json!({"oid": squash});
    h.pull_request(merged);
    h.notify(&task.id);

    eventually(
        "the integrator to be woken to finish the task",
        async || h.launched_argv(&integrator.id).contains("was merged"),
    )
    .await;
    let argv = h.launched_argv(&integrator.id);
    assert!(argv.contains("mark_merged"), "{argv}");

    // And the sha it reports is accepted, though no ancestor check would have.
    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &integrator.id,
                serde_json::json!({"to": "merged", "merge_commit": squash}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.merge_commit.as_deref(), Some(squash.as_str()));
}

/// What humans write on the pull request reaches the engineer exactly once,
/// as a round of requested changes the daemon writes itself — no integrator
/// woken to copy it across — and the revision goes back to the same pull
/// request rather than to a second one.
#[tokio::test]
async fn pull_request_comments_reach_the_engineer_once_each() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;
    h.goes_idle(&integrator.id).await;
    // When it was last launched, and the round the pull request was published
    // from: the comments belong on that round, and they wake nobody here.
    let integrator_launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;
    let round = h.store.get_task(&task.id).await.unwrap().review_round;

    let mut commented = open_pull_request();
    commented["reviewDecision"] = "CHANGES_REQUESTED".into();
    commented["comments"] = serde_json::json!([{
        "id": "C1", "author": {"login": "maria"}, "body": "why a new module?",
    }]);
    commented["reviews"] = serde_json::json!([{
        "id": "R1", "author": {"login": "jon"}, "body": "split src/board.rs up",
        "state": "CHANGES_REQUESTED",
    }]);
    h.pull_request(commented);
    // And what they wrote on the diff, over more pages than one — the shape
    // `gh api --paginate` documents, and the one a pull request people have
    // really been through comes back as.
    h.review_comments(
        r#"[{"id":21,"user":{"login":"jon"},"body":"this allocates per row","path":"src/board.rs","line":42}]
           [{"id":22,"user":{"login":"maria"},"body":"and this name is wrong","path":"src/lane.rs","line":7}]"#,
    );
    h.notify(&task.id);

    // One poll is the whole relay: the engineer is resumed on it, and the
    // task passed through changes_requested to get there.
    eventually("the engineer to be resumed with the comments", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        PR_URL,
        "4 new comments",
        "### maria commented",
        "> why a new module?",
        "### jon requested changes",
        "> split src/board.rs up",
        // The comments on the diff too, both pages of them, each naming the
        // file and line it hangs on.
        "### jon commented on src/board.rs:42",
        "> this allocates per row",
        "### maria commented on src/lane.rs:7",
        "> and this name is wrong",
        "request_review",
    ] {
        assert!(
            argv.contains(quoted),
            "the briefing has no {quoted}: {argv}"
        );
    }

    // The daemon took the task off its integrator itself, saying why.
    let sent_back_by_the_daemon = h
        .store
        .list_task_transitions(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.to_status == "changes_requested")
        .expect("the task went back to its engineer");
    assert_eq!(sent_back_by_the_daemon.from_status, "integrating");
    assert_eq!(sent_back_by_the_daemon.actor, "daemon");
    assert_eq!(
        sent_back_by_the_daemon.reason.as_deref(),
        Some("Pull request #12 was commented on")
    );

    // Written as a round of requested changes on the round the pull request
    // was published from, by no session: the daemon wrote it, not an agent.
    let sent_back: Vec<_> = h
        .store
        .list_reviews(&task.id, Some(round))
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
        .collect();
    assert_eq!(sent_back.len(), 1, "one send-back for one poll");
    assert_eq!(sent_back[0].session_id, None);
    let body = sent_back[0].body.clone().unwrap();
    for quoted in [
        PR_URL,
        "why a new module?",
        "split src/board.rs up",
        "this allocates per row",
        "and this name is wrong",
    ] {
        assert!(
            body.contains(quoted),
            "the send-back has no {quoted}: {body}"
        );
    }

    // And no integrator was woken for any of it.
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .launched_at,
        integrator_launched_at,
        "the integrator was woken to relay the comments: {}",
        h.launched_argv(&integrator.id)
    );
    assert_eq!(
        h.sessions(&task.id, Role::Integrator).await.len(),
        1,
        "and no second one was started for them"
    );
    assert!(
        !h.launched_argv(&integrator.id)
            .contains("why a new module?"),
        "the integrator was told what the humans wrote"
    );

    assert_eq!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .pr_relayed_comments(),
        vec![
            "C1".to_string(),
            "RC21".to_string(),
            "RC22".to_string(),
            "R1".to_string()
        ],
        "every one of them is remembered as relayed, whichever page it came on"
    );

    // The engineer answers every comment and asks for review again. The pull
    // request is published, so the people reading it are this round's
    // reviewers: no reviewer profile is started and no verdict is waited for,
    // and the branch goes straight back to the integrator with the answers on
    // it. Its session died with the send-back, so this is also the other half
    // of the poll's job: a task whose integrator is gone gets one started.
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
        "echo fixed > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'fix: split it up'",
    );
    let reviewer_sessions = h.sessions(&task.id, Role::Reviewer).await.len();
    h.request_review(&task.id, &engineer.id, REPLIES).await;
    eventually("the integrator to be handed the task again", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.resume_instruction(&integrator.id).contains(REPLIES)
    })
    .await;

    // Nobody reviewed it here: the round was closed by the daemon, on the
    // reason it wrote down, with no reviewer started and no verdict recorded.
    assert_eq!(
        h.sessions(&task.id, Role::Reviewer).await.len(),
        reviewer_sessions,
        "a reviewer was started for a round the humans on the pull request are reviewing"
    );
    let approval = h
        .store
        .list_task_transitions(&task.id)
        .await
        .unwrap()
        .into_iter()
        .rev()
        .find(|t| t.to_status == "approved")
        .expect("the round was approved");
    assert_eq!(approval.from_status, "under_review");
    assert_eq!(approval.actor, "daemon");
    assert_eq!(
        approval.reason.as_deref(),
        Some("Pull request #12 is published: its reviewers replace the internal review round")
    );
    let published_round = h.store.get_task(&task.id).await.unwrap().review_round;
    assert!(
        h.store
            .list_reviews(&task.id, Some(published_round))
            .await
            .unwrap()
            .is_empty(),
        "the round wanted a review row of its own"
    );
    assert!(
        h.thread_messages(&task.id).await.iter().any(|m| m
            .body
            .contains("is published, so the humans reviewing it are this round's reviewers")),
        "the task's conversation does not say why the round was approved"
    );

    // And what the integrator was woken with: push the revision to the same
    // pull request, the one way a published branch may be updated, and hand
    // the engineer's replies to the user.
    let instruction = h.resume_instruction(&integrator.id);
    for expected in [
        PR_URL,
        REPLIES,
        "git merge --no-edit <remote>/main",
        &format!("git push <remote> {}", task.branch),
        "`post_message` to \"user\"",
        "never a second one",
    ] {
        assert!(
            instruction.contains(expected),
            "the instruction has no {expected}: {instruction}"
        );
    }
    for never in ["rebase", "--force", "--amend"] {
        assert!(
            !instruction.contains(never),
            "the instruction rewrites what is published with {never}: {instruction}"
        );
    }
    assert_eq!(
        h.sessions(&task.id, Role::Integrator).await.len(),
        1,
        "the same integrator session throughout"
    );

    // Its worktree was cut again from the branch as the engineer left it, so
    // what it pushes is the revision the humans asked for.
    let integrator_worktree = PathBuf::from(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .worktree_path
            .unwrap(),
    );
    assert_eq!(
        out(&integrator_worktree, "git log -1 --format=%s"),
        "fix: split it up",
        "the integrator holds an older tip than the engineer left"
    );

    // Polled again, on the same comments, nothing is relayed a second time:
    // the ids are remembered, so the poll reads a quiet pull request.
    h.goes_idle(&integrator.id).await;
    let engineer_launched_at = h.store.get_session(&engineer.id).await.unwrap().launched_at;
    let polls = h.gh_log().matches("pr view").count();
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        h.gh_log().matches("pr view").count() > polls,
        "the pull request was never polled again"
    );
    let round = h.store.get_task(&task.id).await.unwrap().review_round;
    assert!(
        h.store
            .list_reviews(&task.id, Some(round))
            .await
            .unwrap()
            .iter()
            .all(|r| r.verdict() == ReviewVerdict::Approve),
        "the comments were sent back a second time"
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Integrating);
    assert_eq!(
        h.store.get_session(&engineer.id).await.unwrap().launched_at,
        engineer_launched_at,
        "the engineer was resumed for comments it had already been given"
    );
}

/// The whole of one round on a published pull request: a comment, the
/// engineer's answers, the push that carries them and the two — exactly two —
/// messages the user gets out of it, one from the integrator with the replies
/// in it and one from the daemon when GitHub says the request is approved.
///
/// Three in the thread altogether, the first being the notice the pull
/// request was opened at all, which is written as it is recorded and belongs
/// to no round.
#[tokio::test]
async fn a_published_round_pushes_the_replies_and_addresses_the_user_twice() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;
    h.goes_idle(&integrator.id).await;

    // A human asks for a change, and the engineer gets it.
    let mut commented = open_pull_request();
    commented["reviewDecision"] = "CHANGES_REQUESTED".into();
    commented["reviews"] = serde_json::json!([{
        "id": "R1", "author": {"login": "jon"}, "body": "split src/board.rs up",
        "state": "CHANGES_REQUESTED",
    }]);
    h.pull_request(commented);
    h.notify(&task.id);
    eventually("the engineer to be resumed with the comment", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();

    // It answers, and asks for review again: one reconciliation later the
    // task is approved and being integrated, with no reviewer in between.
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
        "echo fixed > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'fix: split it up'",
    );
    h.request_review(&task.id, &engineer.id, REPLIES).await;
    eventually("the integrator to be woken with the replies", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.resume_instruction(&integrator.id).contains(REPLIES)
    })
    .await;
    assert!(
        h.sessions(&task.id, Role::Reviewer).await.is_empty(),
        "a reviewer was started for a round GitHub's reviewers own"
    );
    let instruction = h.resume_instruction(&integrator.id);
    assert!(instruction.contains(PR_URL), "{instruction}");
    assert!(
        instruction.contains("`post_message` to \"user\""),
        "{instruction}"
    );
    assert!(
        instruction.ends_with(REPLIES),
        "the replies reached the integrator changed: {instruction:?}"
    );

    // The integrator does what it was told: pushes, and writes the one
    // message the user gets out of the round.
    h.post_to_user(
        &task.id,
        &integrator.id,
        &format!("The revision is on {PR_URL}. The engineer's replies:\n\n{REPLIES}"),
    )
    .await;
    h.goes_idle(&integrator.id).await;
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].body.contains(REPLIES), "{}", told[1].body);

    // GitHub says it is approved: the daemon tells the user once, and that is
    // the second and last thing it hears about this round.
    let mut approved = open_pull_request();
    approved["reviewDecision"] = "APPROVED".into();
    approved["reviews"] = serde_json::json!([{
        "id": "R2", "author": {"login": "jon"}, "body": "", "state": "APPROVED",
    }]);
    h.pull_request(approved);
    h.notify(&task.id);
    eventually("the user to be told it is theirs to merge", async || {
        h.user_messages(&task.id).await.len() == 3
    })
    .await;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 3, "the user was addressed again: {told:?}");
    assert!(told[2].body.contains("ready for you to merge"), "{told:?}");

    // And the merge finishes the task off the base branch, as it always did.
    let repo = h.repo_path();
    sh(
        &repo,
        &format!(
            "git merge --squash -q {branch} && \
             git -c user.email=t@t -c user.name=t commit -qm 'feat(board): render it'",
            branch = task.branch
        ),
    );
    let squash = out(&repo, "git rev-parse main");
    let mut merged = open_pull_request();
    merged["state"] = "MERGED".into();
    merged["mergedAt"] = "2026-08-24T10:00:00Z".into();
    merged["mergeCommit"] = serde_json::json!({"oid": squash});
    h.pull_request(merged);
    h.notify(&task.id);
    eventually(
        "the integrator to be woken to finish the task",
        async || h.resume_instruction(&integrator.id).contains("was merged"),
    )
    .await;
    let landed: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &integrator.id,
                serde_json::json!({"to": "merged", "merge_commit": squash}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(
        h.user_messages(&task.id).await.len(),
        3,
        "the round addressed the user more than twice"
    );
}

/// The same on a daemon that restarted: a published request with comments
/// waiting on it has no live integrator to poll around, and none is started
/// for it — the comments are the engineer's, and an agent stood up for a task
/// that is leaving `integrating` in the same breath is the hop this avoids.
#[tokio::test]
async fn comments_waiting_on_a_task_with_no_live_integrator_start_none() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;

    // The daemon went down and came back: the pane the integrator was in is
    // gone, and its session row with it.
    h.store
        .set_session_status(&integrator.id, SessionStatus::Exited)
        .await
        .unwrap();
    let launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;

    let mut commented = open_pull_request();
    commented["comments"] = serde_json::json!([{
        "id": "C1", "author": {"login": "maria"}, "body": "why a new module?",
    }]);
    h.pull_request(commented);
    h.review_comments(
        r#"[{"id":21,"user":{"login":"jon"},"body":"this allocates per row","path":"src/board.rs","line":42}]"#,
    );
    h.notify(&task.id);

    eventually("the engineer to be resumed with the comments", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    assert!(argv.contains("> why a new module?"), "{argv}");
    assert!(
        argv.contains("### jon commented on src/board.rs:42"),
        "{argv}"
    );

    // Nothing was started for the request and nothing was relaunched: the
    // session that died stays dead.
    assert_eq!(
        h.sessions(&task.id, Role::Integrator).await.len(),
        1,
        "an integrator was started for comments that were the engineer's"
    );
    let after = h.store.get_session(&integrator.id).await.unwrap();
    assert_eq!(after.status(), SessionStatus::Exited);
    assert_eq!(after.launched_at, launched_at);
}

/// A repository with no GitHub remote — or a `gh` that cannot answer for it —
/// is landed the local way: no pull request is ever recorded, so nothing is
/// polled and the ancestor check is what proves the merge.
#[tokio::test]
async fn without_a_pull_request_the_task_is_landed_locally() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;

    // The fallback the briefing names, run: rebase, squash, fast-forward.
    let worktree = PathBuf::from(integrator.worktree_path.clone().unwrap());
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
                &integrator.id,
                serde_json::json!({"to": "merged", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(landed.status, TaskStatus::Merged);
    assert_eq!(landed.pr_url, None);
    assert_eq!(h.gh_log(), "", "a locally landed task never asks GitHub");
}

/// Watching is the two forges Ariadne knows: a pull request published on a
/// third is recorded on the task and never polled — whatever a stub `gh` in
/// the same checkout would have said about it.
#[tokio::test]
async fn a_pull_request_on_another_forge_is_recorded_but_not_watched() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.goes_idle(&integrator.id).await;

    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": "https://codeberg.org/ariadne/ariadne/pulls/3"}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_number, Some(3));

    // Told about all the same, and told the truth: a forge with no watcher
    // is one nothing will say any more about, so the notice asks for the
    // merge to be reported here rather than promising a watch there is none
    // of.
    let notice = h.user_messages(&task.id).await;
    assert_eq!(notice.len(), 1);
    assert!(
        notice[0]
            .body
            .contains("https://codeberg.org/ariadne/ariadne/pulls/3"),
        "{}",
        notice[0].body
    );
    assert!(
        notice[0].body.contains("does not watch this forge"),
        "{}",
        notice[0].body
    );

    // An approval nobody is watching for is an approval nobody is told about.
    let mut approved = open_pull_request();
    approved["reviewDecision"] = "APPROVED".into();
    h.pull_request(approved);
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(h.gh_log(), "");
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

    // And a URL that names no pull request at all is refused outright.
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &integrator.id,
            serde_json::json!({"url": "https://github.com/ariadne/ariadne"}),
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("not a pull request URL"),
        "{}",
        String::from_utf8_lossy(&body)
    );
}

/// A pull request GitHub says is red is the engineer's, however approved it
/// is: the failing check goes back as a round of requested changes naming it,
/// the user is never told a request nobody can merge is theirs to merge, the
/// same failure is not sent back twice — and the failure on the revision that
/// was supposed to fix it is.
#[tokio::test]
async fn a_failing_check_goes_to_the_engineer_rather_than_to_the_user() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;
    h.goes_idle(&integrator.id).await;
    let integrator_launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;

    // Approved on GitHub, and failing its checks: the engineer is the one
    // woken for it, with the check named.
    h.pull_request(red_pull_request(HEAD));
    h.notify(&task.id);
    eventually(
        "the engineer to be resumed with the failing check",
        async || {
            h.status(&task.id).await == TaskStatus::InProgress
                && h.live_session(&task.id, Role::Engineer).await.is_some()
        },
    )
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        PR_URL,
        "1 failing check",
        "- test (FAILURE)",
        "https://github.com/ariadne/ariadne/actions/runs/17/job/test",
        "no rebase, no forced push",
        "request_review",
    ] {
        assert!(
            argv.contains(quoted),
            "the briefing has no {quoted}: {argv}"
        );
    }

    // Written by the daemon itself, with no agent woken to carry it across.
    let sent_back: Vec<_> = h
        .store
        .list_reviews(&task.id, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
        .collect();
    assert_eq!(sent_back.len(), 1);
    assert_eq!(sent_back[0].session_id, None);
    let sent_back_by_the_daemon = h
        .store
        .list_task_transitions(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.to_status == "changes_requested")
        .expect("the task went back to its engineer");
    assert_eq!(sent_back_by_the_daemon.actor, "daemon");
    assert_eq!(
        sent_back_by_the_daemon.reason.as_deref(),
        Some("Pull request #12's checks are failing")
    );
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .launched_at,
        integrator_launched_at,
        "the integrator was woken for a check the engineer fixes"
    );

    // And the user was told nothing: the only message it has is the one that
    // said the pull request was open.
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].body.contains("is open"), "{}", told[0].body);

    // Polled again on the same failure, nothing happens twice.
    let engineer_launched_at = h.store.get_session(&engineer.id).await.unwrap().launched_at;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.store
            .list_reviews(&task.id, None)
            .await
            .unwrap()
            .iter()
            .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
            .count(),
        1,
        "the same failing check was sent back a second time"
    );
    assert_eq!(
        h.store.get_session(&engineer.id).await.unwrap().launched_at,
        engineer_launched_at,
        "the engineer was resumed for a failure it had already been given"
    );
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

    // The engineer answers, the branch goes back to the integrator, and the
    // build fails again on the revision that was supposed to fix it: a new
    // commit is a new failure, and it is sent back like the first.
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
        "echo fixed > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'fix: make the test pass'",
    );
    h.request_review(&task.id, &engineer.id, REPLIES).await;
    eventually("the integrator to be handed the task again", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.resume_instruction(&integrator.id).contains(REPLIES)
    })
    .await;
    h.goes_idle(&integrator.id).await;

    h.pull_request(red_pull_request("def456"));
    h.notify(&task.id);
    eventually(
        "the engineer to be resumed with the new failure",
        async || {
            h.status(&task.id).await == TaskStatus::InProgress
                && h.store
                    .list_reviews(&task.id, None)
                    .await
                    .unwrap()
                    .iter()
                    .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
                    .count()
                    == 2
        },
    )
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    assert!(h.launched_argv(&engineer.id).contains("- test (FAILURE)"));
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

    // Fixed for good: the checks are green, GitHub still says approved, and
    // now — and only now — the user is told it is theirs to merge, once.
    sh(
        &worktree,
        "echo green > feature.txt && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm 'fix: really make it pass'",
    );
    h.request_review(&task.id, &engineer.id, REPLIES).await;
    eventually("the integrator to be woken with the fix", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.live_session(&task.id, Role::Integrator).await.is_some()
    })
    .await;
    h.goes_idle(&integrator.id).await;

    let mut green = open_pull_request();
    green["reviewDecision"] = "APPROVED".into();
    green["headRefOid"] = "789abc".into();
    green["statusCheckRollup"] = serde_json::json!([{
        "__typename": "CheckRun", "name": "test", "status": "COMPLETED",
        "conclusion": "SUCCESS",
        "detailsUrl": "https://github.com/ariadne/ariadne/actions/runs/19/job/test",
    }]);
    h.pull_request(green);
    h.notify(&task.id);
    eventually("the user to be told it is theirs to merge", async || {
        h.user_messages(&task.id).await.len() == 2
    })
    .await;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 2, "the user was addressed again: {told:?}");
    assert!(told[1].body.contains("ready for you to merge"), "{told:?}");
}

/// And a pull request that stopped merging into its base goes the same way,
/// with the one thing only the engineer can do about it: merge the base in on
/// top of the commits people are already reading.
#[tokio::test]
async fn a_conflicting_pull_request_goes_to_the_engineer_with_the_base_to_merge() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    let _: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": PR_URL}),
            ),
            StatusCode::OK,
        )
        .await;
    h.goes_idle(&integrator.id).await;

    // The base moved under the branch while the humans were reading it.
    let mut conflicting = open_pull_request();
    conflicting["reviewDecision"] = "APPROVED".into();
    conflicting["mergeable"] = "CONFLICTING".into();
    conflicting["mergeStateStatus"] = "DIRTY".into();
    h.pull_request(conflicting);
    h.notify(&task.id);

    eventually("the engineer to be resumed with the conflict", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        PR_URL,
        "no longer merges into main",
        "git merge --no-edit <remote>/main",
        "no rebase, no forced push",
        "request_review",
    ] {
        assert!(
            argv.contains(quoted),
            "the briefing has no {quoted}: {argv}"
        );
    }
    let sent_back_by_the_daemon = h
        .store
        .list_task_transitions(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.to_status == "changes_requested")
        .expect("the task went back to its engineer");
    assert_eq!(sent_back_by_the_daemon.actor, "daemon");
    assert_eq!(
        sent_back_by_the_daemon.reason.as_deref(),
        Some("Pull request #12 no longer merges into main")
    );

    // Once, however often it is polled, and never as news for the user.
    let engineer_launched_at = h.store.get_session(&engineer.id).await.unwrap().launched_at;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.store
            .list_reviews(&task.id, None)
            .await
            .unwrap()
            .iter()
            .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
            .count(),
        1,
        "the same conflict was sent back a second time"
    );
    assert_eq!(
        h.store.get_session(&engineer.id).await.unwrap().launched_at,
        engineer_launched_at
    );
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        !told[0].body.contains("ready for you to merge"),
        "{}",
        told[0].body
    );
}
