//! Resuming an agent keeps its session row.
//!
//! A task bounced back by its reviewers is the same engineer, in the same
//! conversation, in the same worktree — so it stays one session however many
//! rounds it takes, rather than growing a sibling row per round. The same
//! holds for each reviewer: one session for the whole review.
//!
//! No tmux and no agent CLI needed: `tmux` is a stub script that records the
//! commands the launcher issues, which is also how the console-log wiring is
//! checked without a pane to pipe. What the agent itself was launched with is
//! read from the session's spawn plan, since that is where it travels — tmux
//! is handed `ariadne _spawn <plan>`. `git` is real — a reviewer's worktree
//! has to actually move to the branch tip between rounds.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::Receiver;

use ariadne_api::stream::DomainEvent;
use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{Actor, AgentKind, GoalStatus, PromptKind, Role, SessionStatus, TaskStatus};
use ariadne_daemon::agents::prompts;
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, NewGoal, NewProfile, NewRepository, NewSession, NewTask, ProfileUpdate,
    SessionFilter, Store, Task,
};

/// How long a test waits for an event before giving up.
const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    store: Store,
    bus: EventBus,
    launcher: Arc<Launcher>,
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
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    Harness {
        store,
        bus,
        launcher,
        dir,
    }
}

/// Run a shell command in `dir` (repo setup), failing the test if it does not.
fn sh(dir: &Path, cmd: &str) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "command failed: {cmd}");
}

/// A `tmux` that has no sessions and records every command it is given, so a
/// test can read back what the launcher asked for.
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

impl Harness {
    /// A task with an engineer session that has already run once: a worktree
    /// on disk and a tmux that is no longer alive.
    async fn task_with_engineer_session(&self) -> (Task, AgentSession) {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        // Not a git repo: a fresh spawn cannot get off the ground here, which
        // is what the fallback test leans on.
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner,
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
                repo_id: repo.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.clone(),
                reviewer_profile_ids: vec![reviewer],
                depends_on: vec![],
            })
            .await
            .unwrap();

        let worktree = self.dir.path().join("wt-eng");
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
            .await
            .unwrap();
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: engineer,
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await
            .unwrap();
        (self.store.get_task(&task.id).await.unwrap(), session)
    }

    /// The same, with the agent id a first run would have reported: the
    /// conversation there is to resume.
    async fn task_with_resumable_engineer(&self) -> (Task, AgentSession) {
        let (task, session) = self.task_with_engineer_session().await;
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
        self.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        (task, self.store.get_session(&session.id).await.unwrap())
    }

    /// A task under review for real: a repo on disk with a commit on the task
    /// branch, and one reviewer assigned to it (whose id is returned).
    async fn task_under_review(&self) -> (Task, String) {
        self.task_under_review_on(None).await
    }

    /// The same, with the engineer and reviewer profiles carrying `model` at
    /// the moment the task is created — so that is what the task and the
    /// reviewer slot are pinned to.
    async fn task_under_review_on(&self, model: Option<&str>) -> (Task, String) {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self
            .profile_with(
                "engineer",
                Role::Engineer,
                Some(AgentKind::ClaudeCode),
                model,
            )
            .await;
        let reviewer = self
            .profile_with(
                "reviewer",
                Role::Reviewer,
                Some(AgentKind::ClaudeCode),
                model,
            )
            .await;
        let repo_path = self.dir.path().join("repo-git");
        std::fs::create_dir_all(&repo_path).unwrap();
        sh(
            &repo_path,
            "git init -q -b main && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm init",
        );
        let repo = self
            .store
            .create_repository(NewRepository {
                path: repo_path.display().to_string(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner,
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
                repo_id: repo.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer,
                reviewer_profile_ids: vec![reviewer.clone()],
                depends_on: vec![],
            })
            .await
            .unwrap();
        sh(&repo_path, &format!("git branch {}", task.branch));
        for (to, actor) in [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Engineer),
        ] {
            self.store
                .transition_task(&task.id, to, actor, None, None)
                .await
                .unwrap();
        }
        (self.store.get_task(&task.id).await.unwrap(), reviewer)
    }

    /// The reviewer bounces the task back and the engineer pushes another
    /// commit: the task returns to review one round on, one commit ahead.
    async fn next_round(&self, task: &Task) -> Task {
        let repo_path =
            PathBuf::from(&self.store.get_repository(&task.repo_id).await.unwrap().path);
        sh(
            &repo_path,
            &format!(
                "git checkout -q {branch} && echo v2 > file.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm revision && \
                 git checkout -q main",
                branch = task.branch
            ),
        );
        for (to, actor) in [
            (TaskStatus::ChangesRequested, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Engineer),
        ] {
            self.store
                .transition_task(&task.id, to, actor, None, None)
                .await
                .unwrap();
        }
        self.store.get_task(&task.id).await.unwrap()
    }

    async fn profile(&self, name: &str, role: Role) -> String {
        self.profile_with(name, role, Some(AgentKind::ClaudeCode), None)
            .await
    }

    async fn profile_with(
        &self,
        name: &str,
        role: Role,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
    ) -> String {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind,
                model: model.map(str::to_string),
                system_prompt: Some(format!("You are {name}.")),
            })
            .await
            .unwrap()
            .id
    }

    /// A goal on a repo of its own, whose planner profile was pinned at
    /// `model` when the goal was created. Returns the goal and the planner's
    /// profile id.
    async fn goal_with_planner(&self, model: Option<&str>) -> (String, String) {
        let planner = self
            .profile_with("planner", Role::Planner, Some(AgentKind::ClaudeCode), model)
            .await;
        let repo_path = self.dir.path().join("repo-planner");
        std::fs::create_dir_all(&repo_path).unwrap();
        let repo = self
            .store
            .create_repository(NewRepository {
                path: repo_path.display().to_string(),
                base_branch: "main".into(),
                description: None,
                merge_strategy: Default::default(),
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Plan the work".into(),
                description: "desc".into(),
                planner_profile_id: planner.clone(),
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id],
            })
            .await
            .unwrap();
        (goal.id, planner)
    }

    /// Point a profile at another model, the way an edit in the UI would.
    async fn set_model(&self, profile_id: &str, model: Option<&str>) {
        self.store
            .update_profile(
                profile_id,
                ProfileUpdate {
                    model: Some(model.map(str::to_string)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    /// Move a profile onto another agent CLI *and* another model, which is
    /// what a `PUT /v1/profiles/{id}` from the UI amounts to.
    async fn set_agent_and_model(
        &self,
        profile_id: &str,
        agent_kind: Option<AgentKind>,
        model: Option<&str>,
    ) {
        self.store
            .update_profile(
                profile_id,
                ProfileUpdate {
                    agent_kind: Some(agent_kind),
                    model: Some(model.map(str::to_string)),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    async fn sessions_of(&self, task: &Task) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    /// Every command the launcher gave the stub `tmux`, one per line.
    fn tmux_commands(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("tmux-commands.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The spawn plan the last launch of `session_id` left in its run dir:
    /// the argv and env that no longer ride in the tmux command line.
    fn spawn_plan(&self, session_id: &str) -> SpawnPlanFile {
        let path = self.plan_file(session_id);
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        SpawnPlanFile::from_json(&raw).unwrap()
    }

    fn plan_file(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json")
    }

    /// The last `new-session` the launcher issued, as the stub recorded it.
    fn last_new_session(&self) -> String {
        self.tmux_commands()
            .into_iter()
            .rfind(|c| c.starts_with("new-session"))
            .expect("the launcher started a tmux session")
    }

    fn console_log(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("console.log")
    }
}

/// Wait for the first event matching `pred`, skipping unrelated ones.
async fn next_event(rx: &mut Receiver<BusEvent>, pred: impl Fn(&BusEvent) -> bool) -> BusEvent {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if pred(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for a matching domain event")
}

/// Which agent and model a session runs on comes off the pin its role
/// carries — the reviewer slot here — and a profile edited afterwards does not
/// reach it, on any launch path: not the resume that carries a reviewer into
/// round two, and not the fresh session a round with nothing to resume gets.
#[tokio::test]
async fn a_reviewers_pin_outlives_a_profile_edit() {
    let h = harness().await;
    let (task, reviewer) = h.task_under_review_on(Some("opus")).await;

    // Nothing to resume yet, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.model.as_deref(), Some("opus"));
    assert!(
        h.spawn_plan(&first.id)
            .argv
            .join(" ")
            .contains("--model opus"),
        "the launch asked for the pinned model"
    );

    // The profile moves to another agent and another model while the session
    // is alive. The row is not rewritten behind it.
    h.set_agent_and_model(&reviewer, Some(AgentKind::Codex), Some("sonnet"))
        .await;
    assert_eq!(
        h.store
            .get_session(&first.id)
            .await
            .unwrap()
            .model
            .as_deref(),
        Some("opus"),
        "a profile edit rewrote a running session's model"
    );

    // Round two relaunches the same session, on the same agent and model it
    // was pinned to — the profile now says codex/sonnet.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = h.next_round(&task).await;
    let second = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "Round 2: have another look.")
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "round 2 reused the session");
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model.as_deref(), Some("opus"));
    let argv = h.spawn_plan(&second.id).argv.join(" ");
    assert!(
        argv.contains("--model opus"),
        "and that is what the agent was launched with: {argv}"
    );

    // A round that finds nothing to resume spawns afresh, and lands on the
    // pin just the same.
    h.launcher.kill_session(&second.id).await.unwrap();
    let third = h
        .launcher
        .spawn_reviewer(&task.id, &reviewer)
        .await
        .unwrap();
    assert_ne!(third.id, second.id, "a fresh session, not the old one");
    assert_eq!(third.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(third.model.as_deref(), Some("opus"));
    assert!(
        h.spawn_plan(&third.id)
            .argv
            .join(" ")
            .contains("--model opus"),
        "a fresh session read the profile instead of the pin"
    );
}

/// The same for the engineer, whose pin is the task's: the spawn that starts
/// the work and every resume that carries it through review run on the model
/// the task was created with.
#[tokio::test]
async fn an_engineers_pin_outlives_a_profile_edit() {
    let h = harness().await;
    let (task, _reviewer) = h.task_under_review_on(Some("opus")).await;
    h.set_agent_and_model(
        &task.engineer_profile_id,
        Some(AgentKind::Codex),
        Some("sonnet"),
    )
    .await;

    let first = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(first.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(first.model.as_deref(), Some("opus"));

    h.launcher.kill_session(&first.id).await.unwrap();
    let resumed = h
        .launcher
        .resume_engineer(&task.id, "Round 1: please fix things.")
        .await
        .unwrap();
    assert_eq!(resumed.id, first.id, "the resume reused the session");
    assert_eq!(resumed.model.as_deref(), Some("opus"));
    let argv = h.spawn_plan(&resumed.id).argv.join(" ");
    assert!(
        argv.contains("--model opus"),
        "the resume re-read the profile: {argv}"
    );
}

/// And for the planner, whose pin is the goal's: a respawn after the profile
/// moved still plans on the agent and model the goal was created with.
#[tokio::test]
async fn a_planner_respawn_stays_on_the_goals_pin() {
    let h = harness().await;
    let (goal, planner) = h.goal_with_planner(Some("opus")).await;

    let first = h.launcher.spawn_planner(&goal).await.unwrap();
    assert_eq!(first.model.as_deref(), Some("opus"));

    h.set_agent_and_model(&planner, Some(AgentKind::Codex), Some("sonnet"))
        .await;
    h.launcher.kill_session(&first.id).await.unwrap();

    let second = h.launcher.spawn_planner(&goal).await.unwrap();
    assert_ne!(second.id, first.id, "a planner respawn is a fresh session");
    assert_eq!(second.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(second.model.as_deref(), Some("opus"));
    assert!(
        h.spawn_plan(&second.id)
            .argv
            .join(" ")
            .contains("--model opus"),
        "the respawn read the profile instead of the goal's pin"
    );
}

/// A pin of "no model" is a pin too: the work runs on the agent CLI's own
/// default however the profile is edited afterwards.
#[tokio::test]
async fn a_pin_of_no_model_stays_the_agents_own_default() {
    let h = harness().await;
    let (task, reviewer) = h.task_under_review_on(None).await;
    h.set_model(&reviewer, Some("sonnet")).await;

    let session = h
        .launcher
        .spawn_reviewer(&task.id, &reviewer)
        .await
        .unwrap();
    assert_eq!(session.model, None);
    assert!(
        !h.spawn_plan(&session.id).argv.join(" ").contains("--model"),
        "no model was asked for"
    );
}

/// The changes-requested bounce, twice over: the task panel's Sessions tab
/// must still list one engineer, live again, on the same conversation.
#[tokio::test]
async fn resuming_the_engineer_reuses_its_session_across_review_rounds() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    for round in 1..=2 {
        let resumed = h
            .launcher
            .resume_engineer(&task.id, &format!("Round {round}: please fix things."))
            .await
            .unwrap();
        assert_eq!(resumed.id, first.id, "round {round} reused the session");
        assert_eq!(resumed.status(), SessionStatus::Running);
        assert_eq!(resumed.ended_at, None, "the session is live again");
        assert_eq!(
            resumed.tmux_session, first.tmux_session,
            "and keeps its tmux name"
        );
        assert_eq!(
            resumed.internal_session_id.as_deref(),
            Some("uuid-1234"),
            "on the same agent conversation"
        );
        assert!(resumed.last_activity_at.is_some(), "and is stamped live");
        let sessions = h.sessions_of(&task).await;
        assert_eq!(
            sessions.len(),
            1,
            "round {round} left more than one engineer session: {sessions:?}"
        );
        // Each relaunch resumed the stored conversation rather than starting
        // one. The plan is where that is written now, one per launch.
        let argv = h.spawn_plan(&resumed.id).argv.join(" ");
        assert!(argv.contains("--resume uuid-1234"), "round {round}: {argv}");
        assert!(
            argv.contains(&format!("Round {round}: please fix things.")),
            "round {round} carried its instruction: {argv}"
        );
    }
}

/// The point of the spawn plan: what an agent is told has no bearing on the
/// size of the command tmux is given.
///
/// A briefing of a hundred kilobytes used to be unlaunchable — tmux hands its
/// server one message, capped near 16KB, so `new-session` answered "command
/// too long" until the spawn ran out of attempts and the task was failed for
/// it. Now tmux gets three words and a path, and the launch itself is in the
/// plan file: argv, environment, working directory, and permissions that keep
/// it to the daemon.
#[tokio::test]
async fn a_launch_hands_tmux_nothing_that_can_outgrow_it() {
    use std::os::unix::fs::PermissionsExt;

    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;
    let briefing = "B".repeat(100_000);

    let session = h
        .launcher
        .resume_engineer(&task.id, &briefing)
        .await
        .unwrap();
    let worktree = session.worktree_path.clone().unwrap();

    // What tmux was asked to run, in full: the plan file and nothing else.
    let plan_file = h.plan_file(&first.id);
    assert_eq!(
        h.last_new_session(),
        format!(
            "new-session -d -s {} -c {worktree} -- {} _spawn {}",
            session.tmux_session,
            h.launcher.cfg.cli_bin,
            plan_file.display()
        )
    );

    // And the plan is the launch, verbatim: the briefing the adapter built,
    // the environment that used to arrive as `-e` pairs, the working dir.
    let plan = h.spawn_plan(&first.id);
    assert_eq!(plan.argv[0], "claude");
    assert!(
        plan.argv.iter().any(|arg| arg.ends_with(&briefing)),
        "the briefing rode in the plan: {:?}",
        plan.argv.iter().map(String::len).collect::<Vec<_>>()
    );
    assert!(
        plan.env
            .contains(&("ARIADNE_SESSION_ID".to_string(), first.id.clone())),
        "the session env rode in the plan: {:?}",
        plan.env
    );
    assert_eq!(plan.cwd, PathBuf::from(&worktree));

    // The plan stays behind as the record of how the session was started, and
    // it holds the agent's whole environment: nobody else's to read.
    let mode = std::fs::metadata(&plan_file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "plan mode: {mode:o}");
}

/// A reviewer that sees a task through two rounds is one reviewer with one
/// memory of it: round two wakes the session it already has — same row, same
/// tmux name, same conversation — in a worktree moved to the new tip, and is
/// told which round it is now judging.
#[tokio::test]
async fn a_reviewer_reuses_its_session_across_review_rounds() {
    let h = harness().await;
    let (task, reviewer) = h.task_under_review().await;

    // Round one: nothing to resume, so this is the reviewer's first spawn.
    let first = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: no session yet)")
        .await
        .unwrap();
    assert_eq!(first.role(), Role::Reviewer);
    assert_eq!(first.review_round, Some(1));
    assert!(
        !first.tmux_session.ends_with("-r1"),
        "the round is no part of the session's name: {}",
        first.tmux_session
    );
    let internal = first
        .internal_session_id
        .clone()
        .expect("claude picks its session uuid at spawn");

    // The task leaves review, so the daemon tears the reviewer's tmux down;
    // then the engineer revises and it comes back for round two.
    h.launcher.kill_session(&first.id).await.unwrap();
    let task = h.next_round(&task).await;
    assert_eq!(task.review_round, 2);

    // The briefing is the reviewer profile's own template, rendered — the same
    // path the scheduler takes.
    let template = prompts::template_for(&h.store, &reviewer, PromptKind::ReviewerResume).await;
    let second = h
        .launcher
        .resume_reviewer(
            &task.id,
            &reviewer,
            &prompts::reviewer_resume_briefing(&template, &task, Some("I rewrote the thing.")),
        )
        .await
        .unwrap();
    assert_eq!(second.id, first.id, "round 2 reused the session");
    assert_eq!(
        second.tmux_session, first.tmux_session,
        "and keeps its tmux name"
    );
    assert_eq!(
        second.internal_session_id.as_deref(),
        Some(internal.as_str()),
        "on the same agent conversation"
    );
    assert_eq!(second.status(), SessionStatus::Running);
    assert_eq!(second.ended_at, None, "the session is live again");
    assert_eq!(
        second.review_round,
        Some(2),
        "and its row says which round it is on"
    );
    let sessions: Vec<AgentSession> = h
        .sessions_of(&task)
        .await
        .into_iter()
        .filter(|s| s.role() == Role::Reviewer)
        .collect();
    assert_eq!(
        sessions.len(),
        1,
        "two rounds left more than one reviewer session: {sessions:?}"
    );

    // The worktree it wakes up in is the branch as it stands now.
    let worktree = PathBuf::from(second.worktree_path.as_deref().unwrap());
    assert_eq!(
        std::fs::read_to_string(worktree.join("file.txt")).unwrap(),
        "v2\n",
        "the reviewer woke up in the tree it already reviewed"
    );

    let argv = h.spawn_plan(&second.id).argv.join(" ");
    assert!(
        argv.contains(&format!("--resume {internal}")),
        "round 2 resumed the stored conversation: {argv}"
    );
    assert!(
        argv.contains("Round 2 of"),
        "and was told which round it is reviewing: {argv}"
    );
    // One console log, appended to across both rounds.
    let commands = h.tmux_commands();
    let expected = format!("cat >> '{}'", h.console_log(&first.id).display());
    let pipes: Vec<&String> = commands
        .iter()
        .filter(|c| c.starts_with("pipe-pane"))
        .collect();
    assert_eq!(pipes.len(), 2, "one pipe-pane per launch: {pipes:?}");
    for pipe in pipes {
        assert!(
            pipe.contains(&expected),
            "both rounds pipe into the one console log: {pipe}"
        );
    }
}

/// A reviewer session that never reported an agent id is no conversation to
/// go back to — codex and opencode only report theirs from a hook — so the
/// next round spawns a fresh one rather than failing.
#[tokio::test]
async fn a_reviewer_without_an_agent_id_is_spawned_afresh() {
    let h = harness().await;
    let (task, reviewer) = h.task_under_review().await;
    let stillborn = h
        .store
        .create_session(NewSession {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Reviewer,
            profile_id: reviewer.clone(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            tmux_session: session_name(&task.goal_id, Some(&task.id), "reviewer", Some("rev")),
            worktree_path: None,
            review_round: Some(1),
        })
        .await
        .unwrap();
    h.store
        .set_session_status(&stillborn.id, SessionStatus::Exited)
        .await
        .unwrap();

    let spawned = h
        .launcher
        .resume_reviewer(&task.id, &reviewer, "(unused: nothing to resume)")
        .await
        .unwrap();
    assert_ne!(spawned.id, stillborn.id, "a fresh session, not that one");
    assert_eq!(spawned.status(), SessionStatus::Running);
    assert!(spawned.internal_session_id.is_some());
    assert_eq!(
        h.store.get_session(&stillborn.id).await.unwrap().status(),
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
}

/// The UI's caches are driven by domain events, and a reused row only ever
/// gets updates — so the relaunch has to announce itself as one.
#[tokio::test]
async fn a_relaunch_announces_the_session_as_updated() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;
    let mut rx = h.bus.subscribe();

    h.launcher
        .resume_engineer(&task.id, "fix things")
        .await
        .unwrap();

    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::SessionUpdated(s) if s.status.is_live()),
    )
    .await;
    let DomainEvent::SessionUpdated(session) = event.event else {
        unreachable!("filtered above")
    };
    assert_eq!(session.id, first.id);
    assert!(
        !rx.try_recv()
            .is_ok_and(|e| matches!(e.event, DomainEvent::SessionCreated(_))),
        "a relaunch creates nothing"
    );
}

/// Console-log continuity: with the id reused, both runs pipe into the one
/// file, and deliberately append to it — the log stays the whole transcript of
/// the one session, in the order the terminal produced it.
#[tokio::test]
async fn relaunches_append_to_the_same_console_log() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    h.launcher.resume_engineer(&task.id, "again").await.unwrap();
    h.launcher
        .resume_engineer(&task.id, "and again")
        .await
        .unwrap();

    let expected = format!("cat >> '{}'", h.console_log(&first.id).display());
    let commands = h.tmux_commands();
    let pipes: Vec<&String> = commands
        .iter()
        .filter(|c| c.starts_with("pipe-pane"))
        .collect();
    assert_eq!(pipes.len(), 2, "one pipe-pane per launch: {commands:?}");
    for pipe in pipes {
        assert!(
            pipe.contains(&expected),
            "a relaunch must append to the session's own console log: {pipe}"
        );
    }
}

/// Manual resume (the UI's button, `ariadne attach`): the caller gets the very
/// session it named back, live again, not a sibling to go and find.
#[tokio::test]
async fn reviving_a_session_revives_it_in_place() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    let revived = h.launcher.revive_session(&first.id, None).await.unwrap();
    assert_eq!(revived.id, first.id);
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, first.worktree_path);
    assert_eq!(h.sessions_of(&task).await.len(), 1);
}

/// In place down to the agent and the model: a revive puts the session back on
/// its feet exactly as it was launched, so a profile edited in the meantime
/// does not get to move the conversation somewhere else either.
#[tokio::test]
async fn a_revive_keeps_the_agent_and_model_the_session_was_launched_with() {
    let h = harness().await;
    let (task, _reviewer) = h.task_under_review_on(Some("opus")).await;
    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    h.launcher.kill_session(&session.id).await.unwrap();

    h.set_agent_and_model(
        &task.engineer_profile_id,
        Some(AgentKind::Codex),
        Some("sonnet"),
    )
    .await;

    let revived = h.launcher.revive_session(&session.id, None).await.unwrap();
    assert_eq!(revived.id, session.id, "the same session, revived");
    assert_eq!(revived.agent_kind(), AgentKind::ClaudeCode);
    assert_eq!(revived.model.as_deref(), Some("opus"));
    let argv = h.spawn_plan(&revived.id).argv.join(" ");
    assert!(
        argv.contains("--model opus"),
        "the revive re-read the profile: {argv}"
    );
}

/// Nothing to resume from: an engineer session that never reported an agent id
/// is not a conversation, so it is left alone and a fresh spawn is what runs
/// (which fails here for want of a git repo — the point is the path taken).
#[tokio::test]
async fn a_session_without_an_agent_id_is_not_revived() {
    let h = harness().await;
    let (task, first) = h.task_with_engineer_session().await;
    h.store
        .set_session_status(&first.id, SessionStatus::Exited)
        .await
        .unwrap();

    assert!(
        h.launcher
            .resume_engineer(&task.id, "carry on")
            .await
            .is_err(),
        "there is no repo to spawn a fresh engineer in"
    );
    let after = h.store.get_session(&first.id).await.unwrap();
    assert_eq!(
        after.status(),
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
    assert_eq!(h.sessions_of(&task).await.len(), 1);
}

/// A finished goal has nothing left for an agent to come back to, and the
/// scheduler kills what is live under one — so a revive here would put a
/// session up for the next tick to take straight down. Refused instead, and
/// the session stays as it ended.
#[tokio::test]
async fn a_session_of_a_finished_goal_is_not_revived() {
    for finished in [GoalStatus::Completed, GoalStatus::Cancelled] {
        let h = harness().await;
        let (_task, session) = h.task_with_resumable_engineer().await;
        h.store
            .set_goal_status(&session.goal_id, finished)
            .await
            .unwrap();

        let error = h
            .launcher
            .revive_session(&session.id, None)
            .await
            .expect_err("a finished goal revives nothing")
            .to_string();
        assert!(
            error.contains(finished.as_str()),
            "the refusal says what the goal is: {error}"
        );
        let after = h.store.get_session(&session.id).await.unwrap();
        assert_eq!(after.status(), SessionStatus::Exited);
    }
}
