//! The prompts an agent is launched with come from its profile's rows.
//!
//! Two things have to hold for prompts to be a developer's to edit: what the
//! database says is what the session gets, and a template edited into nonsense
//! still starts a session — a broken prompt is a bad briefing, never a task
//! that cannot get an agent. Saving such a template is refused these days (see
//! `PromptKind::validate_template`), so the broken one below is put into the
//! database behind the store's back, the way one written before the check
//! existed sits there.
//!
//! No tmux and no agent CLI: `tmux` is a stub that records the commands the
//! launcher issues, and the rendered briefing is read back from the session's
//! spawn plan — tmux is handed `ariadne _spawn <plan>` and nothing of the
//! briefing itself. `git` is real — spawning an engineer creates its worktree.

use std::path::Path;
use std::sync::Arc;

use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{AgentKind, PromptKind, Role};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::tmux::TmuxManager;
use ariadne_store::{NewGoal, NewProfile, NewRepository, NewTask, Store, Task};

struct Harness {
    store: Store,
    launcher: Arc<Launcher>,
    dir: tempfile::TempDir,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    Harness {
        store,
        launcher,
        dir,
    }
}

fn sh(dir: &Path, cmd: &str) {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "command failed: {cmd}");
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

impl Harness {
    /// A task ready for its engineer to be spawned, in a real repo. Returns
    /// the task and its engineer profile id.
    async fn task(&self) -> (Task, String) {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let repo_path = self.dir.path().join("repo");
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
                title: "Render prompts from the database".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.clone(),
                reviewer_profile_ids: vec![reviewer],
                depends_on: vec![],
            })
            .await
            .unwrap();
        (task, engineer)
    }

    /// Store a template the store itself would refuse, straight into the row.
    ///
    /// Placeholders are validated when a prompt is saved, never when one is
    /// rendered, so a database can still hold a briefing naming a token
    /// nothing fills in: edited by hand, restored from a backup, or written
    /// before the check existed. Spawning has to survive it.
    async fn plant_template(&self, profile_id: &str, kind: PromptKind, content: &str) {
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self.dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        sqlx::query("UPDATE profile_prompts SET content = ? WHERE profile_id = ? AND kind = ?")
            .bind(content)
            .bind(profile_id)
            .bind(kind.as_str())
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    async fn profile(&self, name: &str, role: Role) -> String {
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

    /// Everything the launcher said to the stub `tmux`, as one string: the
    /// briefing is the last argument of the spawn command.
    fn tmux_log(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("tmux-commands.log")).unwrap_or_default()
    }

    /// The argv the agent of `session_id` was launched with, joined for
    /// reading. It comes from the session's spawn plan, which is where a
    /// briefing of any size now travels.
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
}

/// The briefing in the database is the briefing the agent is launched with,
/// placeholders and all.
#[tokio::test]
async fn a_spawned_engineer_is_briefed_from_its_profiles_prompt() {
    let h = harness().await;
    let (task, engineer) = h.task().await;
    h.store
        .update_profile_prompt(
            &engineer,
            PromptKind::EngineerBriefing,
            "Do {task_title} on {branch}, in {worktree_path}.",
        )
        .await
        .unwrap();

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    let worktree = session.worktree_path.clone().unwrap();
    let briefing = format!("Do {} on {}, in {worktree}.", task.title, task.branch);
    let argv = h.launched_argv(&session.id);
    assert!(
        argv.contains(&briefing),
        "the edited briefing, rendered: {argv}"
    );
    // And nowhere else: a briefing in the tmux command line is what the plan
    // file exists to prevent, whatever its size.
    let log = h.tmux_log();
    assert!(
        !log.contains(&briefing) && !log.contains("Do "),
        "the briefing reached the tmux command line: {log}"
    );

    // The system layer is the profile's prompt as it stands — no playbook
    // appended to it by the daemon any more.
    let system = std::fs::read_to_string(
        h.launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("system-prompt.md"),
    )
    .unwrap();
    assert_eq!(system, "You are engineer.");
}

/// A template that is nonsense by the time it is read is still a briefing: the
/// unknown token and the brace that never closes travel through verbatim and
/// the session starts.
#[tokio::test]
async fn a_broken_template_still_spawns_the_engineer() {
    let h = harness().await;
    let (task, engineer) = h.task().await;
    h.plant_template(
        &engineer,
        PromptKind::EngineerBriefing,
        "# {task_title} {who_even} {unclosed",
    )
    .await;

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(session.status(), ariadne_core::SessionStatus::Running);
    let argv = h.launched_argv(&session.id);
    assert!(
        argv.contains(&format!("# {} {{who_even}} {{unclosed", task.title)),
        "what could not be substituted stayed as it was: {argv}"
    );
}

/// An empty template is no reason to hold a session back either.
#[tokio::test]
async fn an_empty_template_still_spawns_the_engineer() {
    let h = harness().await;
    let (task, engineer) = h.task().await;
    h.store
        .update_profile_prompt(&engineer, PromptKind::EngineerBriefing, "")
        .await
        .unwrap();

    let session = h.launcher.spawn_engineer(&task.id).await.unwrap();
    assert_eq!(session.status(), ariadne_core::SessionStatus::Running);
}

/// A prompt that cannot be read at all — the profile is gone — falls back to
/// the built-in default rather than failing the caller.
#[tokio::test]
async fn a_missing_prompt_falls_back_to_the_default() {
    let h = harness().await;
    let template = ariadne_daemon::agents::prompts::template_for(
        &h.store,
        "01nosuchprofilexxxxxxxxxxx",
        PromptKind::EngineerBriefing,
    )
    .await;
    assert_eq!(
        template,
        ariadne_store::defaults::default_prompt(Role::Engineer, PromptKind::EngineerBriefing)
            .unwrap()
    );
}
