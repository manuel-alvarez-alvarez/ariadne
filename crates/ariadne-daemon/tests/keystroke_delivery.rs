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

mod common;

use std::path::Path;
use std::time::Duration;

use ariadne_core::{AgentKind, AttentionReason, Role};
use ariadne_daemon::tmux::{TmuxManager, TmuxSpawn};
use ariadne_store::{AgentSession, Task};

use common::{Harness, eventually, harness};

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

const INSTRUCTION: &str = "The reviewers asked for changes: apply them on the same branch.";

/// How long a test waits for a spawned delivery to run out of attempts.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A daemon that gives up looking for a TUI after two seconds rather than two
/// minutes: the test that types into one finds it on the first look, and the
/// one about the window running out is only about it running out.
async fn stub_harness() -> Harness {
    harness().typed_input_window(Duration::from_secs(2)).await
}

/// An engineer session on an OpenCode agent, already run once: it has the
/// internal session id a resume goes back to, and a worktree to be resumed in.
/// OpenCode is the kind whose resume instruction cannot ride the argv, so it
/// is the one typed into the pane.
async fn opencode_engineer(h: &Harness) -> (Task, AgentSession) {
    let cast = h.cast_on(AgentKind::Opencode).await;
    let session = h
        .session_on(
            &cast.goal,
            Some(&cast.task),
            Role::Engineer,
            &cast.engineer.id,
            AgentKind::Opencode,
        )
        .await;
    h.make_resumable(&cast.task, &session).await;
    h.every_pane_exists();
    (h.store.get_task(&cast.task.id).await.unwrap(), session)
}

async fn raised(h: &Harness, session: &AgentSession) {
    eventually(TIMEOUT, "the session to be raised", async || {
        h.attention(session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// The other half of the same story, one level up: the resume instruction the
/// launcher types into a freshly relaunched OpenCode TUI is delivered by a
/// task of its own, with no caller to hand a failure back to. A composer that
/// never lets go has to reach the user some other way — the session's
/// attention flag — rather than being logged as typed and forgotten.
#[tokio::test]
async fn a_resume_instruction_that_stays_in_the_composer_raises_the_session() {
    let h = stub_harness().await;
    let (task, session) = opencode_engineer(&h).await;
    // Whatever is pressed, the pane keeps showing the instruction where it was
    // pasted.
    h.composer_keeps(INSTRUCTION);

    h.launcher
        .resume_engineer(&task.id, INSTRUCTION)
        .await
        .unwrap();

    raised(&h, &session).await;
    assert!(
        h.enters(&session) > 1,
        "the Enter was pressed again before giving up"
    );
}

/// The same story again with nothing to type into. A pane that never draws a
/// TUI is watched for the whole window and then given up on — and giving up
/// is a delivery that did not happen, so it ends where every other way of not
/// delivering this one does: on the session, for the user.
///
/// The window is the harness's short one; what it is worth in production is
/// `Config::typed_input_window`, and nothing here depends on which.
#[tokio::test]
async fn a_pane_that_never_draws_raises_the_session_when_the_window_runs_out() {
    let h = stub_harness().await;
    let (task, session) = opencode_engineer(&h).await;
    // No composer written at all: every look at the pane comes back empty,
    // which is what a TUI that never started looks like.

    h.launcher
        .resume_engineer(&task.id, INSTRUCTION)
        .await
        .unwrap();

    raised(&h, &session).await;
    assert_eq!(
        h.enters(&session),
        0,
        "and nothing was typed at a pane that was never ready for it"
    );
}
