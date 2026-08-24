//! The GitLab integrator's lifecycle: publish, watch, relay, finish.
//!
//! `github_integrator`'s twin on the other forge, and deliberately the same
//! test: the task branch becomes a merge request, the daemon watches it while
//! humans review it, and what they do to it decides what happens next — an
//! approval announced to the user once, a discussion note relayed to the
//! engineer once, and the merge finished off the base branch.
//!
//! No tmux, no agent CLI and no GitLab: `glab` is a stub script that prints
//! the merge request, the approvals and the discussions a test wants it to
//! see, and records what it was asked. `gh` is stubbed beside it and is
//! expected never to run — which forge a task is watched on is the recorded
//! URL's to say. The "integrator" doing the recording, the sending back and
//! the merging is the test itself, calling the endpoints its briefing tells
//! the agent to call.

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

/// The merge request every test in here publishes: nested groups and all,
/// which is what a GitLab project path looks like and what the API paths have
/// to escape.
const MR_URL: &str = "https://gitlab.com/ariadne/tools/ariadne/-/merge_requests/3";

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
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$1\" in\n\
         \x20 has-session) exit 1 ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("tmux-commands.log").display(),
    );
    TmuxManager::new(write_executable(&dir.join("tmux-stub.sh"), &script))
}

/// A `glab` that answers `mr view`, `api …/approvals` and `api …/discussions`
/// with whatever JSON the test last wrote, and writes down everything it was
/// asked. No merge request file means no merge request, which is what `glab`
/// itself does about one: a failure on stderr. No approvals file is nobody
/// having approved it, and no discussions file is nothing written on it.
fn write_glab_stub(dir: &Path) -> String {
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$*\" in\n\
         \x20 *'mr view'*)\n\
         \x20   if [ -f '{mr}' ]; then cat '{mr}'; else echo 'merge request not found' >&2; exit 1; fi ;;\n\
         \x20 *approvals*)\n\
         \x20   if [ -f '{approvals}' ]; then cat '{approvals}'; else echo '{{\"approved\":false,\"approved_by\":[]}}'; fi ;;\n\
         \x20 *discussions*)\n\
         \x20   if [ -f '{discussions}' ]; then cat '{discussions}'; else echo '[]'; fi ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("glab-commands.log").display(),
        mr = dir.join("mr.json").display(),
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

    /// Everything `glab` has been asked, one invocation per line.
    fn glab_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("glab-commands.log")).unwrap_or_default()
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
                reviewer_profile_id: reviewer.to_string(),
                session_id: None,
                verdict: ReviewVerdict::Approve,
                body: Some("looks right".into()),
            })
            .await
            .unwrap();
        self.notify(&task.id);
    }

    /// The messages of the task thread that are addressed to the user.
    async fn user_messages(&self, task_id: &str) -> Vec<MessageDto> {
        let messages: Vec<MessageDto> = self
            .json(
                Request::get(format!("/v1/tasks/{task_id}/messages?limit=100"))
                    .body(Body::empty())
                    .unwrap(),
                StatusCode::OK,
            )
            .await;
        messages
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
        "web_url": MR_URL,
    })
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
    h.goes_idle(&integrator.id).await;

    // An untouched merge request wakes nobody: the integrator is left idle
    // rather than nudged for not having landed anything.
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
    assert!(h.user_messages(&task.id).await.is_empty());
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "an integrator waiting on humans is not a stalled task"
    );

    // Approved: the user is told once that merging it is theirs to do.
    h.approvals(&["maria"]);
    h.notify(&task.id);
    eventually(
        "the user to be told the merge request is ready",
        async || !h.user_messages(&task.id).await.is_empty(),
    )
    .await;
    let notice = h.user_messages(&task.id).await;
    assert_eq!(notice.len(), 1);
    assert!(notice[0].body.contains(MR_URL), "{}", notice[0].body);
    assert!(
        notice[0].body.contains("The merge request for"),
        "{}",
        notice[0].body
    );
    assert!(
        notice[0].body.contains("ready for you to merge"),
        "{}",
        notice[0].body
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
    // Polled again and again, it is still one message.
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(h.user_messages(&task.id).await.len(), 1);

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
/// as a round of requested changes — and the revision goes back to the same
/// merge request rather than to a second one.
#[tokio::test]
async fn discussion_notes_reach_the_engineer_once_each() {
    let h = harness().await;
    let (task, reviewer) = h.task().await;
    let integrator = h.hand_to_the_integrator(&task, &reviewer).await;
    h.publish(&task, &integrator.id).await;
    h.goes_idle(&integrator.id).await;

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
             "position":{"new_path":"src/board.rs","old_path":"src/board.rs"}}]}]
        [{"id":"d3","notes":[{"id":103,"author":{"username":"maria"},"body":"and this name is wrong",
             "system":false,"resolvable":true,"resolved":false,
             "position":{"new_path":"src/lane.rs","old_path":"src/lane.rs"}}]},
         {"id":"d4","notes":[{"id":104,"author":{"username":"maria"},"body":"approved this merge request",
             "system":true,"resolvable":false,"resolved":false}]}]"#,
    );
    h.notify(&task.id);

    eventually("the integrator to be woken with the notes", async || {
        h.launched_argv(&integrator.id)
            .contains("why a new module?")
    })
    .await;
    let argv = h.launched_argv(&integrator.id);
    assert!(
        argv.contains("Merge request !3 has 3 new comments"),
        "{argv}"
    );
    assert!(argv.contains("maria commented"), "{argv}");
    assert!(argv.contains("jon requested changes"), "{argv}");
    // The notes on the diff, both pages of them, each carrying the file it
    // hangs on.
    assert!(
        argv.contains("src/board.rs: this allocates per row"),
        "{argv}"
    );
    assert!(
        argv.contains("src/lane.rs: and this name is wrong"),
        "{argv}"
    );
    assert!(
        !argv.contains("approved this merge request"),
        "GitLab's own note is not a reviewer's: {argv}"
    );
    assert!(argv.contains("return_to_engineer"), "{argv}");
    assert!(argv.contains("glab mr view"), "{argv}");
    assert_eq!(
        h.store
            .get_task(&task.id)
            .await
            .unwrap()
            .pr_relayed_comments(),
        vec!["N101".to_string(), "N102".into(), "N103".into()],
        "every one of them is remembered as relayed, whichever page it came on"
    );

    // Polled again, the same notes wake nobody a second time. The launch is
    // dated after the spawn plan is written, so the relaunch this watches for
    // is only settled once the session says it is running again.
    eventually("the relay launch to be finished", async || {
        h.store.get_session(&integrator.id).await.unwrap().status() == SessionStatus::Running
    })
    .await;
    let launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;
    h.goes_idle(&integrator.id).await;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .launched_at,
        launched_at,
        "the integrator was relaunched for notes it had already relayed: {}",
        h.launched_argv(&integrator.id)
    );

    // The integrator relays them, as its briefing says: the engineer reads a
    // round of requested changes and the task is its own again.
    let sent_back: TaskDto = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/return-to-engineer", task.id),
                &integrator.id,
                serde_json::json!({
                    "summary": "Merge request !3 was commented on.",
                    "changes": [
                        "maria: why a new module?",
                        "jon (src/board.rs): this allocates per row",
                    ],
                }),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(sent_back.status, TaskStatus::ChangesRequested);
    assert_eq!(
        sent_back.pr_url.as_deref(),
        Some(MR_URL),
        "the merge request survives the round trip: the revision goes back to it"
    );

    eventually("the engineer to be resumed with the notes", async || {
        h.status(&task.id).await == TaskStatus::InProgress
            && h.live_session(&task.id, Role::Engineer).await.is_some()
    })
    .await;
    let engineer = h.live_session(&task.id, Role::Engineer).await.unwrap();
    let argv = h.launched_argv(&engineer.id);
    assert!(argv.contains("why a new module?"), "{argv}");

    // Revised and approved again, the integrator gets the task back with the
    // resume briefing that tells it to merge the base into the branch and
    // push it to the merge request it already opened, never rewriting what the
    // humans reading it have already seen.
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
    h.approve(&h.store.get_task(&task.id).await.unwrap(), &reviewer)
        .await;
    eventually("the integrator to be handed the task again", async || {
        h.status(&task.id).await == TaskStatus::Integrating
            && h.launched_argv(&integrator.id)
                .contains("Pick the integration")
    })
    .await;
    let argv = h.launched_argv(&integrator.id);
    assert!(argv.contains("glab mr list --source-branch"), "{argv}");
    assert!(argv.contains("git merge --no-edit <remote>/main"), "{argv}");
    assert!(argv.contains("never open a second"), "{argv}");
    assert!(!argv.contains("--force"), "{argv}");
    assert!(
        argv.contains("ready to look at again"),
        "and the user is told once the push has happened: {argv}"
    );
    assert_eq!(
        h.sessions(&task.id, Role::Integrator).await.len(),
        1,
        "the same integrator session throughout"
    );

    // The notes already relayed stay relayed across the round trip: the
    // engineer is not sent back a second time for the same three. As above,
    // the relaunch is only settled once the session says it is running again.
    eventually("the resume launch to be finished", async || {
        h.store.get_session(&integrator.id).await.unwrap().status() == SessionStatus::Running
    })
    .await;
    h.goes_idle(&integrator.id).await;
    let launched_at = h
        .store
        .get_session(&integrator.id)
        .await
        .unwrap()
        .launched_at;
    for _ in 0..3 {
        h.notify(&task.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        h.store
            .get_session(&integrator.id)
            .await
            .unwrap()
            .launched_at,
        launched_at,
        "the same notes came round again after the revision: {}",
        h.launched_argv(&integrator.id)
    );
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
    eventually(
        "the user to be told the merge request is ready",
        async || !h.user_messages(&task.id).await.is_empty(),
    )
    .await;

    let log = h.glab_log();
    assert!(
        log.contains("mr view 7 -R git.example.com/platform/ariadne"),
        "{log}"
    );
    assert!(log.contains("--hostname git.example.com"), "{log}");
    assert!(
        h.user_messages(&task.id).await[0].body.contains(url),
        "the URL the user is pointed at is the one that was recorded"
    );
    assert_eq!(h.gh_log(), "");
}
