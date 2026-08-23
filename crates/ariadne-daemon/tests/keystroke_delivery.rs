//! Delivering a message into an agent's TUI, and knowing that it arrived.
//!
//! A coding-agent TUI reads a burst of input as a paste, and an Enter that
//! lands inside that window becomes a newline in the composer instead of a
//! submission — the message is typed, never sent, and the agent sits there
//! having heard nothing. `tmux send-keys` reports success for both, so the
//! only way to tell them apart is to read the pane back.
//!
//! The first two tests drive a real tmux with a pane whose program swallows
//! Enter the way Codex does: once after a paste, or every time. The last one
//! drives the daemon instead — a stub `tmux` whose composer never lets go —
//! for what a resumed agent that never heard its instruction leaves behind.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ariadne_core::{AgentKind, AttentionReason, Role};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::tmux::{TmuxManager, TmuxSpawn};
use ariadne_store::{
    AgentSession, NewGoal, NewProfile, NewRepository, NewSession, NewTask, Store, Task,
};

/// Two lines, so the delivery has to be a paste: typed literally, the newline
/// between them would submit the first line on its own.
const MESSAGE: &str = "Finish reviewing this round.\nSubmit approve or request_changes.";

/// A TUI that is all composer: it draws what it has been given under the
/// transcript of what it has been sent, and its Enter can be swallowed.
///
/// `once` swallows the first Enter after a paste, which is the Codex
/// behaviour this whole path exists for; `always` never submits anything.
const SWALLOWING_TUI: &str = r#"import os, sys, tty

mode = sys.argv[1]
fd = sys.stdin.fileno()
tty.setraw(fd)

sent, composer, pending = [], "", b""
pasting, armed = False, False

def redraw():
    out = "\x1b[2J\x1b[H"
    for message in sent:
        out += "sent: " + message.replace("\n", " ") + "\r\n"
    out += "\r\n\r\n> " + composer.replace("\n", "\r\n")
    sys.stdout.write(out)
    sys.stdout.flush()

redraw()
while True:
    chunk = os.read(fd, 4096)
    if not chunk:
        break
    pending += chunk
    while pending:
        if pending.startswith(b"\x1b[200~"):
            pending, pasting = pending[6:], True
            continue
        if pending.startswith(b"\x1b[201~"):
            pending, pasting, armed = pending[6:], False, True
            continue
        if b"\x1b[200~".startswith(pending[:6]) or b"\x1b[201~".startswith(pending[:6]):
            break  # a paste marker still arriving
        char, pending = pending[:1], pending[1:]
        if char == b"\x03":
            sys.exit(0)
        elif char in (b"\r", b"\n"):
            if pasting:
                composer += "\n"  # a newline inside a paste is text, not a key
            elif mode == "always" or armed:
                armed, composer = False, composer + "\n"
            else:
                sent.append(composer.strip())
                composer = ""
        else:
            composer += char.decode("utf-8", "replace")
    redraw()
"#;

/// Start the swallowing TUI in a real tmux session, and give it a moment to
/// draw its first frame.
async fn start_tui(dir: &Path, session: &str, mode: &str) -> TmuxManager {
    let script = dir.join("swallowing-tui.py");
    std::fs::write(&script, SWALLOWING_TUI).unwrap();
    let tmux = TmuxManager::default();
    tmux.new_session(&TmuxSpawn {
        session: session.into(),
        cwd: dir.to_path_buf(),
        env: vec![],
        argv: vec!["python3".into(), script.display().to_string(), mode.into()],
        log_file: None,
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    tmux
}

/// The pane swallows the Enter that should have submitted, exactly as Codex
/// does with one that arrives inside its paste window. The message is not
/// lost: it is still in the composer, and the next Enter sends it.
#[tokio::test]
#[ignore = "requires tmux and python3"]
async fn a_swallowed_enter_is_pressed_again_until_the_message_goes() {
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-swallow-once-{}", std::process::id());
    let tmux = start_tui(dir.path(), &name, "once").await;

    let delivered = tmux.send_submitted(&name, MESSAGE).await.unwrap();

    assert!(delivered, "the delivery is confirmed, not assumed");
    let pane = tmux.capture_pane(&name, 100).await.unwrap();
    assert!(
        pane.contains("sent: Finish reviewing this round. Submit approve or request_changes."),
        "the whole message was submitted, once: {pane}"
    );
    let _ = tmux.kill_session(&name).await;
}

/// A pane that never submits is never reported as if it had: the caller is
/// told the message is still sitting there, which is what raises the session
/// for the user.
#[tokio::test]
#[ignore = "requires tmux and python3"]
async fn a_message_that_never_submits_is_never_called_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let name = format!("ariadne-test-swallow-always-{}", std::process::id());
    let tmux = start_tui(dir.path(), &name, "always").await;

    let delivered = tmux.send_submitted(&name, MESSAGE).await.unwrap();

    assert!(!delivered, "an unconfirmed delivery is not a delivery");
    let pane = tmux.capture_pane(&name, 100).await.unwrap();
    assert!(
        !pane.contains("sent:"),
        "nothing was submitted, and the pane says so: {pane}"
    );
    assert!(
        pane.contains("Submit approve or request_changes."),
        "the message is where it was left, in the composer: {pane}"
    );
    let _ = tmux.kill_session(&name).await;
}

/// The other half of the same story, one level up: the resume instruction the
/// launcher types into a freshly relaunched OpenCode TUI is delivered by a
/// task of its own, with no caller to hand a failure back to. A composer that
/// never lets go has to reach the user some other way — the session's
/// attention flag — rather than being logged as typed and forgotten.
#[tokio::test]
async fn a_resume_instruction_that_stays_in_the_composer_raises_the_session() {
    let h = harness().await;
    let (task, session) = h.opencode_engineer().await;
    // Whatever is pressed, the pane keeps showing the instruction where it was
    // pasted.
    h.composer_keeps(INSTRUCTION);

    h.launcher
        .resume_engineer(&task.id, INSTRUCTION)
        .await
        .unwrap();

    eventually("the session to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        h.enters() > 1,
        "the Enter was pressed again before giving up"
    );
}

const INSTRUCTION: &str = "The reviewers asked for changes: apply them on the same branch.";

/// How long a test waits for a spawned delivery to run out of attempts.
const TIMEOUT: Duration = Duration::from_secs(20);

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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct Harness {
    store: Store,
    launcher: Arc<Launcher>,
    dir: tempfile::TempDir,
    _bus: ariadne_daemon::bus::EventBus,
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
        launcher,
        dir,
        _bus: bus,
    }
}

/// A `tmux` whose pane is always there, draws whatever the test put in
/// `composer`, and counts the Enters it is sent.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
        \x20 capture-pane) cat '{composer}' 2>/dev/null ;;\n\
        \x20 display-message) echo '80x24 0,0' ;;\n\
        \x20 send-keys) [ \"$#\" = 4 ] && [ \"$4\" = Enter ] && echo enter >> '{enters}' ;;\n\
         esac\n\
         exit 0\n",
        composer = dir.join("composer").display(),
        enters = dir.join("enters").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    /// An engineer session on an OpenCode agent, already run once: it has the
    /// internal session id a resume goes back to, and a worktree to be
    /// resumed in. OpenCode is the kind whose resume instruction cannot ride
    /// the argv, so it is the one typed into the pane.
    async fn opencode_engineer(&self) -> (Task, AgentSession) {
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let planner = self.profile("planner", Role::Planner).await;
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
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
                title: "task".into(),
                description: "desc".into(),
                engineer_profile_id: engineer.clone(),
                integrator_profile_id: None,
                reviewer_profile_ids: vec![reviewer],
                depends_on: vec![],
            })
            .await
            .unwrap();
        let worktree = self.dir.path().join("worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id,
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: engineer,
                agent_kind: AgentKind::Opencode,
                model: None,
                tmux_session: "ariadne-test-stuck-eng".into(),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await
            .unwrap();
        self.store
            .set_session_internal_id(&session.id, "ses_previous")
            .await
            .unwrap();
        (self.store.get_task(&task.id).await.unwrap(), session)
    }

    async fn profile(&self, name: &str, role: Role) -> String {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::Opencode),
                model: None,
                system_prompt: "You work.".into(),
                prompts: vec![],
            })
            .await
            .unwrap()
            .id
    }

    /// What the pane draws: a composer holding `text`, for good.
    fn composer_keeps(&self, text: &str) {
        std::fs::write(self.dir.path().join("composer"), format!("> {text}\n")).unwrap();
    }

    /// How many Enters the pane was sent.
    fn enters(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("enters"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    async fn attention(&self, session: &AgentSession) -> Option<AttentionReason> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    }
}
