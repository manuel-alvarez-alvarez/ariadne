//! The GitLab integrator's lifecycle: publish, watch, send back, finish.
//!
//! `github_integrator`'s twin on the other forge, and deliberately the same
//! test: the task branch becomes a merge request, the daemon watches it while
//! humans review it, and what they do to it decides what happens next — an
//! approval announced to the user once, a discussion note written back to the
//! engineer once, and the merge finished off the base branch.
//!
//! No tmux, no agent CLI and no GitLab: `glab` is a stub script that prints
//! the merge request, the approvals and the discussions a test wants it to
//! see, and records what it was asked. `gh` is stubbed beside it and is
//! expected never to run — which forge a task is watched on is the recorded
//! URL's to say. The "integrator" doing the recording and the merging is the
//! test itself, calling the endpoints its briefing tells the agent to call.

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
    ReviewAuthor, SessionFilter, Store, Task,
};

/// How long a test waits for the scheduler to reach a state. Generous
/// because every test in here walks a real repository through several
/// reconciliations while the rest of the file runs beside it: what the number
/// has to outlast is a loaded machine, not a scheduler that is quick about it.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The merge request every test in here publishes: nested groups and all,
/// which is what a GitLab project path looks like and what the API paths have
/// to escape.
const MR_URL: &str = "https://gitlab.com/ariadne/tools/ariadne/-/merge_requests/3";

/// What the engineer answers the humans with, and what has to reach them
/// unchanged: the summary of the `request_review` that closes a published
/// round is a reply to every comment on the request.
const REPLIES: &str = "Reply to @jon on src/board.rs:42: it allocates once per lane now.\n\
                       Reply to @maria on src/lane.rs:7: renamed to `lane_index`.\n\
                       Reply to @maria: the module stays — it is what makes the lane testable.";

/// And what it writes next, once the review is requested: a message in the
/// same thread by the same author, which nothing downstream may hand on as
/// the summary of the round.
const AFTERWARDS: &str = "Thanks — I will watch for anything else on the request.";

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
    cfg.glab_bin = write_glab_stub(dir.path());
    cfg.gh_bin = write_gh_stub(dir.path());
    // Every reconciliation is a poll: what the interval is for is sparing
    // GitLab, and there is no GitLab here.
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

fn write_executable(path: &Path, script: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.display().to_string()
}

/// A `tmux` with no sessions that records every command it is given.
///
/// Its panes are gone as far as `has-session` is concerned, which is what
/// lets the liveness sweep retire the sessions a test has finished with. A
/// test about an agent that is *working* wants the opposite — a pane that
/// answers, so the session stays live for as long as the test needs it —
/// and writes a `tmux-alive` file to say so (see
/// [`Harness::tmux_keeps_sessions_alive`]).
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$1\" in\n\
         \x20 has-session) if [ -f '{alive}' ]; then exit 0; else exit 1; fi ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("tmux-commands.log").display(),
        alive = dir.join("tmux-alive").display(),
    );
    TmuxManager::new(write_executable(&dir.join("tmux-stub.sh"), &script))
}

/// A `glab` that answers `mr view`, `api …/approvals` and `api …/discussions`
/// with whatever JSON the test last wrote, and writes down everything it was
/// asked. No merge request file means no merge request, which is what `glab`
/// itself does about one: a failure on stderr. No approvals file is nobody
/// having approved it, and no discussions file is nothing written on it.
///
/// The three reads fail apart from each other, because that is how they fail
/// in life: a token that lost a scope, a rate limit or approval rules nobody
/// may read leave part of a poll answering and part of it failing. A
/// `glab-fails` file is the whole CLI down — not installed, not signed in —
/// and an `approvals-fails` file only the read of the approvals.
fn write_glab_stub(dir: &Path) -> String {
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         if [ -f '{down}' ]; then cat '{down}' >&2; exit 1; fi\n\
         case \"$*\" in\n\
         \x20 *'mr view'*)\n\
         \x20   if [ -f '{mr}' ]; then cat '{mr}'; else echo 'merge request not found' >&2; exit 1; fi ;;\n\
         \x20 *approvals*)\n\
         \x20   if [ -f '{approvals_down}' ]; then cat '{approvals_down}' >&2; exit 1; fi\n\
         \x20   if [ -f '{approvals}' ]; then cat '{approvals}'; else echo '{{\"approved\":false,\"approved_by\":[]}}'; fi ;;\n\
         \x20 *discussions*)\n\
         \x20   if [ -f '{discussions}' ]; then cat '{discussions}'; else echo '[]'; fi ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("glab-commands.log").display(),
        down = dir.join("glab-fails").display(),
        mr = dir.join("mr.json").display(),
        approvals_down = dir.join("approvals-fails").display(),
        approvals = dir.join("approvals.json").display(),
        discussions = dir.join("discussions.json").display(),
    );
    write_executable(&dir.join("glab-stub.sh"), &script)
}

/// A `gh` that records what it was asked and answers nothing: every line it
/// writes is a line this suite fails on.
fn write_gh_stub(dir: &Path) -> String {
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> '{log}'\nexit 1\n",
        log = dir.join("gh-commands.log").display(),
    );
    write_executable(&dir.join("gh-stub.sh"), &script)
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

    /// What the stub `glab mr view` will answer the next poll with.
    fn merge_request(&self, json: serde_json::Value) {
        std::fs::write(
            self.dir.path().join("mr.json"),
            serde_json::to_string(&json).unwrap(),
        )
        .unwrap();
    }

    /// And what `glab api …/approvals` will say about it.
    fn approvals(&self, who: &[&str]) {
        let by: Vec<serde_json::Value> = who
            .iter()
            .map(|u| serde_json::json!({"user": {"username": u}}))
            .collect();
        std::fs::write(
            self.dir.path().join("approvals.json"),
            serde_json::json!({"approved": !who.is_empty(), "approved_by": by}).to_string(),
        )
        .unwrap();
    }

    /// And `glab api --paginate …/discussions`, verbatim: the pages as
    /// `--paginate` writes them.
    fn discussions(&self, pages: &str) {
        std::fs::write(self.dir.path().join("discussions.json"), pages).unwrap();
    }

    /// Keep every session's pane answering `has-session`, so the liveness
    /// sweep leaves the sessions where they are: what a test about an agent
    /// mid-turn needs, since a session the sweep has retired is no longer one
    /// anything has to work around.
    fn tmux_keeps_sessions_alive(&self) {
        std::fs::write(self.dir.path().join("tmux-alive"), "").unwrap();
    }

    /// Everything `glab` has been asked, one invocation per line.
    fn glab_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("glab-commands.log")).unwrap_or_default()
    }

    /// What `glab` fails with from now on, whole (`glab-fails`) or only for
    /// the approvals (`approvals-fails`); `None` puts it back on its feet.
    fn glab_fails(&self, file: &str, error: Option<&str>) {
        let path = self.dir.path().join(file);
        match error {
            Some(error) => std::fs::write(&path, error).unwrap(),
            None => {
                std::fs::remove_file(&path).ok();
            }
        }
    }

    /// Poll the request once and wait for the poll to have happened, rather
    /// than for what it decided: what a failing poll decides is nothing, and
    /// there is no state to wait on.
    async fn poll(&self, task_id: &str) {
        let before = self.glab_log().matches("mr view").count();
        self.notify(task_id);
        eventually("the merge request to be polled", async || {
            self.glab_log().matches("mr view").count() > before
        })
        .await;
    }

    /// And everything `gh` has, which had better be nothing.
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

    /// The agent has finished its turn: what the daemon acts on a moved merge
    /// request against is an idle integrator, never one mid-turn.
    ///
    /// The launch it is in the middle of is waited out first. A resume writes
    /// `running` when its tmux side is done, which is after the file the
    /// briefing is read from — so an idleness set on seeing that file is
    /// written straight back over, and the poll leaves the review to an
    /// integrator that will never finish its turn, there being no agent in
    /// it.
    async fn goes_idle(&self, session_id: &str) {
        eventually("the launch to be finished", async || {
            self.store.get_session(session_id).await.unwrap().status() == SessionStatus::Running
        })
        .await;
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
    /// the summary goes into the thread for whoever is reading it, and onto
    /// the transition, which is the round's own record of it and where
    /// everything downstream reads it from.
    ///
    /// Then the engineer says something else, as one answering a question in
    /// its thread does — the message that must not be mistaken for the
    /// summary of the round.
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
        let _: MessageDto = self
            .json(
                as_session(
                    &format!("/v1/tasks/{task_id}/messages"),
                    engineer,
                    serde_json::json!({"body": AFTERWARDS}),
                ),
                StatusCode::CREATED,
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

    /// Walk a fresh task to its integrator working on it: the engineer
    /// commits, the reviewer approves.
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
                author: ReviewAuthor::Profile(reviewer.to_string()),
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

    /// Publish the task as the merge request every test here watches.
    async fn publish(&self, task: &Task, integrator: &str) -> TaskDto {
        self.json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                integrator,
                serde_json::json!({"url": MR_URL}),
            ),
            StatusCode::OK,
        )
        .await
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

/// An open merge request with nothing on it, in the shape `glab mr view -F
/// json` answers with.
fn open_merge_request() -> serde_json::Value {
    serde_json::json!({
        "iid": 3,
        "state": "opened",
        "merged_at": null,
        "merge_commit_sha": null,
        "squash_commit_sha": null,
        "sha": HEAD,
        "target_branch": "main",
        "has_conflicts": false,
        "detailed_merge_status": "mergeable",
        "head_pipeline": {
            "id": 4711,
            "sha": HEAD,
            "status": "success",
            "web_url": PIPELINE_URL,
        },
        "web_url": MR_URL,
    })
}

/// The commit the merge request is open at, until a test pushes a revision.
const HEAD: &str = "abc123";

/// Where the pipeline of that commit is read.
const PIPELINE_URL: &str = "https://gitlab.com/ariadne/tools/ariadne/-/pipelines/4711";

/// A merge request whose head pipeline failed at `head`.
fn red_merge_request(head: &str) -> serde_json::Value {
    let mut mr = open_merge_request();
    mr["sha"] = head.into();
    mr["head_pipeline"]["sha"] = head.into();
    mr["head_pipeline"]["status"] = "failed".into();
    mr
}

/// The whole GitLab path: the integrator is briefed to publish rather than to
/// land, records the merge request it opened, and from there the daemon
/// watches it — an approval told to the user once, and the merge finished off
/// the base branch with a `mark_merged` no ancestor check would have accepted.
#[tokio::test]
async fn a_merge_request_is_watched_from_publication_to_its_merge() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;

    // It was briefed with the merge-request half of the one integrator
    // playbook: which of the three ways it lands this task is the
    // repository's to answer, and the briefing carries all of them.
    let argv = h.launched_argv(&integrator.id);
    for expected in [
        "Publish it as a pull request (GitHub) or a merge request (GitLab)",
        "glab auth status",
        "glab mr create",
        ".gitlab/merge_request_templates/",
        "record_pull_request",
        "land the task locally instead",
    ] {
        assert!(argv.contains(expected), "the briefing has no {expected}");
    }

    // Nothing is asked of GitLab before there is a merge request to ask about.
    h.notify(&task.id);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        h.glab_log(),
        "",
        "glab was called for a task with no merge request"
    );

    // The integrator opens it and reports it. That is the end of its turn.
    let published = h.publish(&task, &integrator.id).await;
    assert_eq!(published.pr_number, Some(3));
    assert_eq!(published.pr_url.as_deref(), Some(MR_URL));

    // Recording it is what tells the user there is one, in the forge's own
    // vocabulary: nothing in Ariadne merges a merge request either.
    let opened = h.user_messages(&task.id).await;
    assert_eq!(opened.len(), 1);
    assert!(opened[0].body.contains(MR_URL), "{}", opened[0].body);
    assert!(
        opened[0].body.contains("Merge request !3 is open"),
        "{}",
        opened[0].body
    );
    // Said once, and by the daemon: the integrator's briefing does not ask it
    // to announce the URL as well.
    assert_eq!(
        h.thread_messages(&task.id)
            .await
            .into_iter()
            .filter(|m| m.author_session_id.is_some() && m.body.contains(MR_URL))
            .count(),
        0
    );
    eventually("the merge request to go up for the user", async || {
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason()
            == Some(AttentionReason::WaitingUser)
    })
    .await;
    h.goes_idle(&integrator.id).await;

    // An untouched merge request wakes nobody: the integrator is left idle
    // rather than nudged for not having landed anything, and the approval it
    // is still waiting on is not news to tell the user twice.
    h.merge_request(open_merge_request());
    h.notify(&task.id);
    // The discussions are the last of the three reads one poll takes, so a
    // log carrying them is a poll that finished.
    eventually("the merge request to be polled", async || {
        h.glab_log().contains("discussions")
    })
    .await;
    let log = h.glab_log();
    // By number and by the project its URL names, on the host it names.
    assert!(
        log.contains("mr view 3 -R gitlab.com/ariadne/tools/ariadne"),
        "{log}"
    );
    assert!(
        log.contains("api --hostname gitlab.com projects/ariadne%2Ftools%2Fariadne/merge_requests/3/approvals"),
        "{log}"
    );
    assert!(
        log.contains(
            "--paginate projects/ariadne%2Ftools%2Fariadne/merge_requests/3/discussions?per_page=100"
        ),
        "{log}"
    );
    assert_eq!(h.gh_log(), "", "a GitLab task never asks the GitHub CLI");
    assert_eq!(h.user_messages(&task.id).await.len(), 1);
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "an integrator waiting on humans is not a stalled task"
    );

    // Approved: the user is told once that merging it is theirs to do.
    h.approvals(&["maria"]);
    h.notify(&task.id);
    eventually(
        "the user to be told the merge request is ready",
        async || h.user_messages(&task.id).await.len() > 1,
    )
    .await;
    let notice = h.user_messages(&task.id).await;
    assert_eq!(notice.len(), 2);
    assert!(notice[1].body.contains(MR_URL), "{}", notice[1].body);
    assert!(
        notice[1].body.contains("The merge request for"),
        "{}",
        notice[1].body
    );
    assert!(
        notice[1].body.contains("ready for you to merge"),
        "{}",
        notice[1].body
    );
    eventually("the approval to go up for the user", async || {
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason()
            == Some(AttentionReason::WaitingUser)
    })
    .await;
    // Polled again and again, it is still those two — and the flag the user
    // takes down stays down.
    h.store
        .clear_session_attention(&integrator.id)
        .await
        .unwrap();
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(h.user_messages(&task.id).await.len(), 2);
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason(),
        None,
        "a quiet poll raised the approval the user had already dealt with"
    );

    // Merging is not the integrator's to claim while GitLab says otherwise.
    let branch_tip = out(&h.repo_path(), &format!("git rev-parse {}", task.branch));
    let (status, body) = h
        .send(as_session(
            &format!("/v1/tasks/{}/transitions", task.id),
            &integrator.id,
            serde_json::json!({"to": "merged", "merge_commit": branch_tip}),
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal = String::from_utf8_lossy(&body).to_string();
    assert!(refusal.contains("Merge request !3 is opened"), "{refusal}");
    assert!(refusal.contains("not merged"), "{refusal}");

    // Merged on GitLab as a squash: a commit the local base does not contain
    // yet, and no ancestor of the task branch anywhere. Reporting it before
    // the local base has caught up is refused too — a task is landed here as
    // well as there.
    let repo = h.repo_path();
    let mut merged_elsewhere = open_merge_request();
    merged_elsewhere["state"] = "merged".into();
    merged_elsewhere["merged_at"] = "2026-08-24T10:00:00Z".into();
    merged_elsewhere["squash_commit_sha"] = branch_tip.clone().into();
    h.merge_request(merged_elsewhere);
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
    let mut merged = open_merge_request();
    merged["state"] = "merged".into();
    merged["merged_at"] = "2026-08-24T10:00:00Z".into();
    merged["squash_commit_sha"] = squash.clone().into();
    h.merge_request(merged);
    h.notify(&task.id);

    // The wake instruction itself, not a phrase the briefing already carried:
    // what is being watched for here is the relaunch.
    eventually(
        "the integrator to be woken to finish the task",
        async || {
            h.launched_argv(&integrator.id)
                .contains("Merge request !3 was merged on GitLab")
        },
    )
    .await;
    let argv = h.launched_argv(&integrator.id);
    assert!(argv.contains("mark_merged"), "{argv}");
    assert!(argv.contains("fast-forward main"), "{argv}");

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

/// What humans write on the merge request reaches the engineer exactly once,
/// as a round of requested changes the daemon writes itself — no integrator
/// woken to copy it across — and the revision goes back to the same merge
/// request rather than to a second one.
#[tokio::test]
async fn discussion_notes_reach_the_engineer_once_each() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;
    // When it was last launched, and the round the merge request was
    // published from: the notes belong on that round, and they wake nobody
    // here.
    let integrator_launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;
    let round = h.store.get_task(&task.id).await.unwrap().review_round;

    h.merge_request(open_merge_request());
    // Two pages of discussions, which is the shape `glab api --paginate`
    // writes and the one a merge request people have really been through comes
    // back as: a conversation note, two threads on the diff, and one GitLab
    // wrote itself.
    h.discussions(
        r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"maria"},"body":"why a new module?",
             "system":false,"resolvable":false,"resolved":false}]},
           {"id":"d2","notes":[{"id":102,"author":{"username":"jon"},"body":"this allocates per row",
             "system":false,"resolvable":true,"resolved":false,
             "position":{"new_path":"src/board.rs","old_path":"src/board.rs","new_line":42}}]}]
        [{"id":"d3","notes":[{"id":103,"author":{"username":"maria"},"body":"and this name is wrong",
             "system":false,"resolvable":true,"resolved":false,
             "position":{"new_path":"src/lane.rs","old_path":"src/lane.rs","new_line":7}}]},
         {"id":"d4","notes":[{"id":104,"author":{"username":"maria"},"body":"approved this merge request",
             "system":true,"resolvable":false,"resolved":false}]}]"#,
    );
    h.notify(&task.id);

    // One poll is the whole relay: the engineer is resumed on it, and the
    // task passed through changes_requested to get there.
    eventually("the engineer to be resumed with the notes", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        MR_URL,
        "Merge request !3",
        "3 new comments",
        "### maria commented",
        "> why a new module?",
        // The notes on the diff, both pages of them, each naming the file and
        // line it hangs on.
        "### jon requested changes on src/board.rs:42",
        "> this allocates per row",
        "### maria requested changes on src/lane.rs:7",
        "> and this name is wrong",
        "request_review",
        // Under the name of what the humans wrote on, never the integrator's:
        // the round was relayed off GitLab, not written by an agent.
        "### From Merge request !3 on GitLab",
    ] {
        assert!(
            argv.contains(quoted),
            "the briefing has no {quoted}: {argv}"
        );
    }
    assert!(
        !argv.contains("approved this merge request"),
        "GitLab's own note is not a reviewer's: {argv}"
    );
    assert!(
        !argv.contains("From Integrator"),
        "the humans' notes wear the integrator's name: {argv}"
    );

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
        Some("Merge request !3 was commented on")
    );

    // Written as a round of requested changes on the round the merge request
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
    assert_eq!(
        sent_back[0].reviewer_profile_id, None,
        "the relay was recorded under a profile's name"
    );
    assert_eq!(sent_back[0].author_role.as_deref(), Some("forge"));
    let body = sent_back[0].body.clone().unwrap();
    for quoted in [
        MR_URL,
        "why a new module?",
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
        "the integrator was woken to relay the notes: {}",
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
        vec!["N101".to_string(), "N102".into(), "N103".into()],
        "every one of them is remembered as relayed, whichever page it came on"
    );

    // The engineer answers every one of them and asks for review again. The
    // merge request is published, so the people reading it are this round's
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
        "a reviewer was started for a round the humans on the merge request are reviewing"
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
        Some("Merge request !3 is published: its reviewers replace the internal review round")
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
        !h.thread_messages(&task.id).await.iter().any(|m| m
            .body
            .contains("is published, so the humans reviewing it are this round's reviewers")),
        "the reason the round was approved is written twice: once as the \
         transition's own and once into the thread"
    );

    // And what the integrator was woken with: push the revision to the same
    // merge request, the one way a published branch may be updated, and hand
    // the engineer's replies to the user.
    let instruction = h.resume_instruction(&integrator.id);
    for expected in [
        MR_URL,
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

    // Polled again, on the same notes, nothing is relayed a second time: the
    // ids are remembered, so the poll reads a quiet merge request.
    eventually("the resume launch to be finished", async || {
        h.store.get_session(&integrator.id).await.unwrap().status() == SessionStatus::Running
    })
    .await;
    h.goes_idle(&integrator.id).await;
    let engineer_launched_at = h.store.get_session(&engineer.id).await.unwrap().launched_at;
    let polls = h.glab_log().matches("mr view").count();
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        h.glab_log().matches("mr view").count() > polls,
        "the merge request was never polled again"
    );
    let round = h.store.get_task(&task.id).await.unwrap().review_round;
    assert!(
        h.store
            .list_reviews(&task.id, Some(round))
            .await
            .unwrap()
            .iter()
            .all(|r| r.verdict() == ReviewVerdict::Approve),
        "the notes were sent back a second time"
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Integrating);
    assert_eq!(
        h.store.get_session(&engineer.id).await.unwrap().launched_at,
        engineer_launched_at,
        "the engineer was resumed for notes it had already been given"
    );
}

/// A merge request somebody closed without merging it — and the locked state
/// GitLab spells the same ending with: the end of the task, said once to the
/// user, with the branch left where a retry can pick it up.
///
/// Read as quiet — as it was — this was the shape of a task that hung for
/// ever: `integrating`, polled every few minutes, with no stall watch running
/// (the watch replaces it) and nothing said to anybody.
#[tokio::test]
async fn a_merge_request_closed_unmerged_fails_the_task_and_tells_the_user() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;
    let opened = h.user_messages(&task.id).await.len();

    // Closed on GitLab, unmerged, with a note on it nobody is waiting for an
    // answer to: one poll is all it takes.
    let mut closed = open_merge_request();
    closed["state"] = "closed".into();
    h.merge_request(closed);
    h.discussions(
        r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"maria"},"body":"not this way",
             "system":false,"resolvable":false,"resolved":false}]}]"#,
    );
    h.notify(&task.id);
    eventually("the task to be failed", async || {
        h.status(&task.id).await == TaskStatus::Failed
    })
    .await;

    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), opened + 1, "{told:?}");
    let notice = &told[opened].body;
    assert!(notice.contains(MR_URL), "{notice}");
    assert!(
        notice.contains("Merge request !3") && notice.contains("closed without being merged"),
        "{notice}"
    );
    assert!(
        notice.contains("Retry it") && notice.contains("cancel it"),
        "{notice}"
    );
    assert!(
        h.sessions(&task.id, Role::Engineer)
            .await
            .iter()
            .all(|s| s.status() != SessionStatus::Running),
        "the engineer was sent back to a request nobody will merge"
    );
    eventually("the integrator session to end", async || {
        !h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .status()
            .is_live()
    })
    .await;

    // Retried, it starts over on the same branch and with no memory of the
    // merge request that was closed.
    let retried: TaskDto = h
        .json(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/tasks/{}/retry", task.id))
                .body(Body::empty())
                .unwrap(),
            StatusCode::OK,
        )
        .await;
    assert_eq!(retried.status, TaskStatus::Ready);
    assert_eq!(retried.pr_url, None);
    eventually("a fresh engineer to be spawned", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let polls = h.glab_log().matches("mr view").count();
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.glab_log().matches("mr view").count(),
        polls,
        "a task that starts over is still being watched on GitLab"
    );
}

/// The other ending GitLab spells that is not a merge: a locked merge request
/// is nobody's to merge either, and it fails the task the same way.
#[tokio::test]
async fn a_locked_merge_request_ends_the_task_like_a_closed_one() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;

    let mut locked = open_merge_request();
    locked["state"] = "locked".into();
    h.merge_request(locked);
    h.notify(&task.id);
    eventually("the task to be failed", async || {
        h.status(&task.id).await == TaskStatus::Failed
    })
    .await;
    let told = h.user_messages(&task.id).await;
    assert!(
        told.last().unwrap().body.contains(MR_URL),
        "{:?}",
        told.last()
    );
}

/// The same, with the integrator still mid-turn.
///
/// Every other thing a poll says is left for a working agent to finish, on
/// the grounds that what it is doing right now is more current than the poll
/// — and a closed request is the one answer that cannot be: there is nothing
/// left for the turn to push to. A task that waited for its integrator to
/// come back before it could be failed is a task nobody hears has ended.
#[tokio::test]
async fn a_closed_merge_request_ends_the_task_even_mid_turn() {
    let h = harness().await;
    h.tmux_keeps_sessions_alive();
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    // No `goes_idle`: the integrator is working, exactly as it is for the
    // whole turn in which it opens the request and reports it — and its pane
    // answers, so it stays that way rather than being retired out from under
    // the poll.
    assert_eq!(
        h.store.get_session(&integrator.id).await.unwrap().status(),
        SessionStatus::Running
    );
    let opened = h.user_messages(&task.id).await.len();

    let mut closed = open_merge_request();
    closed["state"] = "closed".into();
    h.merge_request(closed);
    h.notify(&task.id);
    eventually("the task to be failed", async || {
        h.status(&task.id).await == TaskStatus::Failed
    })
    .await;
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), opened + 1, "{told:?}");
    assert!(told[opened].body.contains(MR_URL), "{told:?}");
    eventually("the working integrator to be stood down", async || {
        !h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .status()
            .is_live()
    })
    .await;
}

/// A `glab` that cannot read the merge request at all: the failure nobody
/// could see, because a watch that reads nothing looks exactly like a watch on
/// a request nobody is touching.
#[tokio::test]
async fn a_glab_that_cannot_read_the_request_is_reported_once_and_recovers() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;
    h.store
        .clear_session_attention(&integrator.id)
        .await
        .unwrap();
    let opened = h.user_messages(&task.id).await.len();

    // Signed out, and every poll fails the same way. The first of them says
    // nothing: a forge has bad minutes, and one failed poll is not a broken
    // watch.
    h.glab_fails(
        "glab-fails",
        Some("error: you must be authenticated: run `glab auth login`"),
    );
    let before = h.glab_log().matches("mr view").count();
    h.poll(&task.id).await;
    assert_eq!(
        h.user_messages(&task.id).await.len(),
        opened,
        "one bad minute is not news"
    );

    // A run of them is: the user is told, once, with the CLI and what it
    // said, and the task goes up on the strip they read such things from.
    while h.user_messages(&task.id).await.len() == opened {
        h.poll(&task.id).await;
    }
    assert!(
        h.glab_log().matches("mr view").count() - before >= 5,
        "the user was told about the first poll that failed"
    );
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), opened + 1, "{told:?}");
    let notice = &told[opened].body;
    assert!(notice.contains("`glab`"), "{notice}");
    assert!(
        notice.contains("glab auth login"),
        "the error is quoted: {notice}"
    );
    assert!(notice.contains(MR_URL), "{notice}");
    eventually("the integrator session to carry the failure", async || {
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason()
            == Some(AttentionReason::AgentError)
    })
    .await;
    for _ in 0..3 {
        h.poll(&task.id).await;
    }
    assert_eq!(h.user_messages(&task.id).await.len(), opened + 1);
    assert_eq!(h.status(&task.id).await, TaskStatus::Integrating);

    // Signed in again: the next poll that reads the request says so, once,
    // and takes the flag back down.
    h.merge_request(open_merge_request());
    h.glab_fails("glab-fails", None);
    h.poll(&task.id).await;
    eventually("the user to be told it works again", async || {
        h.user_messages(&task.id).await.len() > opened + 1
    })
    .await;
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), opened + 2, "{told:?}");
    assert!(told[opened + 1].body.contains("again"), "{told:?}");
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .attention_reason(),
        None
    );
    for _ in 0..3 {
        h.poll(&task.id).await;
    }
    assert_eq!(h.user_messages(&task.id).await.len(), opened + 2);
}

/// Half a poll is not a quiet merge request: `glab mr view` answers, the read
/// of the approvals does not, and what comes back must not be acted on as
/// though nobody had approved anything.
#[tokio::test]
async fn a_poll_that_could_only_be_half_read_decides_nothing() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;
    let opened = h.user_messages(&task.id).await.len();
    // Recording the request told the user it was theirs, which is the flag a
    // poll that reads nobody having approved it takes back down.
    assert!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .pr_approved_notified()
    );

    h.merge_request(open_merge_request());
    h.glab_fails(
        "approvals-fails",
        Some("error: GET .../approvals: 403 Forbidden"),
    );
    h.poll(&task.id).await;
    assert!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .pr_approved_notified(),
        "an approval nobody could read was taken for one withdrawn"
    );
    assert_eq!(h.user_messages(&task.id).await.len(), opened);

    // It counts as a failed poll like any other, so a run of them is
    // reported — naming the CLI, not the merge request's silence.
    while h.user_messages(&task.id).await.len() == opened {
        h.poll(&task.id).await;
        assert!(
            h.store
                .get_task(&task.id)
                .await
                .unwrap()
                .pr_approved_notified()
        );
    }
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), opened + 1, "{told:?}");
    assert!(told[opened].body.contains("`glab`"), "{told:?}");

    // Reading the approvals again: the poll decides, and a merge request
    // nobody has approved takes the flag back down.
    h.glab_fails("approvals-fails", None);
    h.approvals(&[]);
    h.poll(&task.id).await;
    eventually(
        "the approval flag to be cleared by a whole answer",
        async || {
            !h.store
                .get_task(&task.id)
                .await
                .unwrap()
                .pr_approved_notified()
        },
    )
    .await;
}

/// The whole of one round on a published merge request: a note, the
/// engineer's answers, the push that carries them and the two — exactly two —
/// messages the user gets out of it, one from the integrator with the replies
/// in it and one from the daemon when GitLab says the request is approved.
///
/// Four in the thread altogether: the notice the merge request was opened at
/// all, which is written as it is recorded and belongs to no round, and the
/// one the merge ends the task with.
#[tokio::test]
async fn a_published_round_pushes_the_replies_and_addresses_the_user_twice() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;

    // A human asks for a change, and the engineer gets it.
    h.merge_request(open_merge_request());
    h.discussions(
        r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"jon"},"body":"split src/board.rs up",
             "system":false,"resolvable":true,"resolved":false,
             "position":{"new_path":"src/board.rs","old_path":"src/board.rs","new_line":42}}]}]"#,
    );
    h.notify(&task.id);
    eventually("the engineer to be resumed with the note", async || {
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
        "a reviewer was started for a round GitLab's reviewers own"
    );
    let instruction = h.resume_instruction(&integrator.id);
    assert!(instruction.contains(MR_URL), "{instruction}");
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
        &format!("The revision is on {MR_URL}. The engineer's replies:\n\n{REPLIES}"),
    )
    .await;
    eventually("the resume launch to be finished", async || {
        h.store.get_session(&integrator.id).await.unwrap().status() == SessionStatus::Running
    })
    .await;
    h.goes_idle(&integrator.id).await;
    // Two so far: the notice the daemon wrote when the request was opened,
    // before this round began, and the replies the integrator just handed on.
    let told = h.user_messages(&task.id).await;
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].body.contains(REPLIES), "{}", told[1].body);

    // GitLab says it is approved: the daemon tells the user once, and that is
    // the second and last thing it hears about this round.
    h.approvals(&["maria"]);
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
    let mut merged = open_merge_request();
    merged["state"] = "merged".into();
    merged["merged_at"] = "2026-08-24T10:00:00Z".into();
    merged["squash_commit_sha"] = squash.clone().into();
    h.merge_request(merged);
    h.notify(&task.id);
    eventually(
        "the integrator to be woken to finish the task",
        async || {
            h.resume_instruction(&integrator.id)
                .contains("Merge request !3 was merged on GitLab")
        },
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
    // The round itself addressed the user exactly twice — the replies and the
    // approval — on top of the notice the publication wrote. The merge adds
    // the last one: a task that ended says so, naming what it landed as.
    eventually("the user to be told the task is merged", async || {
        h.user_messages(&task.id).await.len() == 4
    })
    .await;
    let told = h.user_messages(&task.id).await;
    assert!(told[3].body.contains(&squash), "{}", told[3].body);
}

/// The same on a daemon that restarted: a published request with notes
/// waiting on it has no live integrator to poll around, and none is started
/// for it — the notes are the engineer's, and an agent stood up for a task
/// that is leaving `integrating` in the same breath is the hop this avoids.
#[tokio::test]
async fn notes_waiting_on_a_task_with_no_live_integrator_start_none() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;

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

    h.merge_request(open_merge_request());
    h.discussions(
        r#"[{"id":"d1","notes":[{"id":101,"author":{"username":"maria"},"body":"why a new module?",
             "system":false,"resolvable":false,"resolved":false}]},
           {"id":"d2","notes":[{"id":102,"author":{"username":"jon"},"body":"this allocates per row",
             "system":false,"resolvable":true,"resolved":false,
             "position":{"new_path":"src/board.rs","old_path":"src/board.rs","new_line":42}}]}]"#,
    );
    h.notify(&task.id);

    eventually("the engineer to be resumed with the notes", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    assert!(argv.contains("> why a new module?"), "{argv}");
    assert!(
        argv.contains("### jon requested changes on src/board.rs:42"),
        "{argv}"
    );

    // Nothing was started for the request and nothing was relaunched: the
    // session that died stays dead.
    assert_eq!(
        h.sessions(&task.id, Role::Integrator).await.len(),
        1,
        "an integrator was started for notes that were the engineer's"
    );
    let after = h.store.get_session(&integrator.id).await.unwrap();
    assert_eq!(after.status(), SessionStatus::Exited);
    assert_eq!(after.launched_at, launched_at);
}

/// A repository with no GitLab remote — or a `glab` that cannot answer for it
/// — is landed the local way: no merge request is ever recorded, so nothing is
/// polled and the ancestor check is what proves the merge.
#[tokio::test]
async fn without_a_merge_request_the_task_is_landed_locally() {
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
    assert_eq!(h.glab_log(), "", "a locally landed task never asks GitLab");
}

/// A self-hosted GitLab is watched like the hosted one: what says so is the
/// URL's own shape, and everything `glab` is asked is addressed at that host.
#[tokio::test]
async fn a_self_hosted_merge_request_is_watched_on_its_own_host() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.goes_idle(&integrator.id).await;

    let url = "https://git.example.com/platform/ariadne/-/merge_requests/7";
    let published: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &integrator.id,
                serde_json::json!({"url": url}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(published.pr_number, Some(7));

    let mut mr = open_merge_request();
    mr["iid"] = 7.into();
    h.merge_request(mr);
    h.approvals(&["maria"]);
    h.notify(&task.id);
    // The discussions are the last of the three reads one poll takes, so a
    // log carrying them is a poll that finished.
    eventually("the merge request to be polled", async || {
        h.glab_log().contains("discussions")
    })
    .await;

    let log = h.glab_log();
    assert!(
        log.contains("mr view 7 -R git.example.com/platform/ariadne"),
        "{log}"
    );
    assert!(log.contains("--hostname git.example.com"), "{log}");
    // Told once, as it was recorded: a project whose approvals are already in
    // has nothing left to announce, and the notice already points at it.
    let notice = h.user_messages(&task.id).await;
    assert_eq!(notice.len(), 1);
    assert!(
        notice[0].body.contains(url),
        "the URL the user is pointed at is the one that was recorded"
    );
    assert_eq!(h.gh_log(), "");
}

/// A merge request whose pipeline GitLab says failed is the engineer's,
/// however approved it is: the failure goes back as a round of requested
/// changes naming it, the user is never told a request nobody can merge is
/// theirs to merge, the same failure is not sent back twice — and the failure
/// on the revision that was supposed to fix it is.
#[tokio::test]
async fn a_failing_pipeline_goes_to_the_engineer_rather_than_to_the_user() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;
    let integrator_launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;

    // Approved on GitLab, and its pipeline red: the engineer is the one woken
    // for it, with the pipeline named.
    h.approvals(&["maria"]);
    h.merge_request(red_merge_request(HEAD));
    h.notify(&task.id);
    eventually(
        "the engineer to be resumed with the failed pipeline",
        async || {
            h.status(&task.id).await == TaskStatus::InProgress
                && h.live_session(&task.id, Role::Engineer).await.is_some()
        },
    )
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        MR_URL,
        "1 failing check",
        "- pipeline (failed)",
        PIPELINE_URL,
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
        Some("Merge request !3's checks are failing")
    );
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .launched_at,
        integrator_launched_at,
        "the integrator was woken for a pipeline the engineer fixes"
    );

    // And the user was told nothing: the only message it has is the one that
    // said the merge request was open.
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
        "the same failed pipeline was sent back a second time"
    );
    assert_eq!(
        h.store.get_session(&engineer.id).await.unwrap().launched_at,
        engineer_launched_at,
        "the engineer was resumed for a failure it had already been given"
    );
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

    // The engineer answers, the branch goes back to the integrator, and the
    // pipeline fails again on the revision that was supposed to fix it: a new
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
         git -c user.email=t@t -c user.name=t commit -qm 'fix: make the pipeline pass'",
    );
    h.request_review(&task.id, &engineer.id, REPLIES).await;
    eventually("the integrator to be handed the task again", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.resume_instruction(&integrator.id).contains(REPLIES)
    })
    .await;
    h.goes_idle(&integrator.id).await;

    h.merge_request(red_merge_request("def456"));
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
    assert!(
        h.launched_argv(&engineer.id)
            .contains("- pipeline (failed)")
    );
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

    // Fixed for good: the pipeline is green, GitLab still says approved, and
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

    // The revision is pushed and GitLab starts the pipeline over. While it
    // runs the merge request is neither the engineer's — there is nothing to
    // fix yet — nor the user's, and nothing at all is said about the wait.
    let mut running = open_merge_request();
    running["sha"] = "789abc".into();
    running["head_pipeline"]["sha"] = "789abc".into();
    running["head_pipeline"]["status"] = "running".into();
    h.merge_request(running);
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.user_messages(&task.id).await.len(),
        1,
        "the user was told a merge request whose pipeline was still running was theirs to merge"
    );
    assert_eq!(
        h.store
            .list_reviews(&task.id, None)
            .await
            .unwrap()
            .iter()
            .filter(|r| r.verdict() == ReviewVerdict::RequestChanges)
            .count(),
        2,
        "a pipeline that had not finished was sent back to the engineer"
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Integrating);

    let mut green = open_merge_request();
    green["sha"] = "789abc".into();
    green["head_pipeline"]["sha"] = "789abc".into();
    h.merge_request(green);
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

/// And a merge request that stopped merging into its target goes the same
/// way, with the one thing only the engineer can do about it: merge the base
/// in on top of the commits people are already reading.
#[tokio::test]
async fn a_conflicting_merge_request_goes_to_the_engineer_with_the_base_to_merge() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;

    // The base moved under the branch while the humans were reading it.
    h.approvals(&["maria"]);
    let mut conflicting = open_merge_request();
    conflicting["has_conflicts"] = true.into();
    conflicting["detailed_merge_status"] = "conflict".into();
    h.merge_request(conflicting);
    h.notify(&task.id);

    eventually("the engineer to be resumed with the conflict", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    for quoted in [
        MR_URL,
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
        Some("Merge request !3 no longer merges into main")
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
