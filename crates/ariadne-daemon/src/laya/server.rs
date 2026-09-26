//! The local `laya-serve` child and its restart loop.

use std::process::Stdio;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::{mpsc, oneshot};

use super::Laya;

/// The shell `laya-serve` runs under. It holds the read end of a pipe whose
/// write end only the daemon has. The kernel closes that end however the
/// daemon ends, `kill -9` included, and the shell then kills the server's
/// whole process group, so no server outlives the daemon that started it.
/// When the server exits on its own, the shell exits with its status.
const GUARD: &str = r#"exec 3<&0
"$@" </dev/null 3<&- &
server=$!
(read -r _ <&3; kill -s KILL 0) &
watcher=$!
wait "$server"
status=$?
kill "$watcher" 2>/dev/null
exit "$status"
"#;

pub(crate) enum Command {
    Reconcile,
    Restart,
    Stop(oneshot::Sender<()>),
}

pub(crate) fn start(laya: Laya, rx: mpsc::UnboundedReceiver<Command>) {
    tokio::spawn(run(laya, rx));
}

async fn run(laya: Laya, mut rx: mpsc::UnboundedReceiver<Command>) {
    let mut child = None;
    let mut retry_at = Instant::now();
    let mut backoff = laya.timeouts.laya_serve_restart;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = tick.tick() => {},
            command = rx.recv() => match command {
                Some(Command::Reconcile) => {},
                Some(Command::Restart) => {
                    if let Some(mut child) = child.take() {
                        stop(&mut child).await;
                        laya.set_endpoint(None);
                        laya.announce().await;
                    }
                }
                Some(Command::Stop(done)) => {
                    if let Some(mut child) = child.take() { stop(&mut child).await; }
                    laya.set_endpoint(None);
                    laya.announce().await;
                    let _ = done.send(());
                    return;
                }
                None => return,
            }
        }

        let Ok(settings) = laya.store.laya_settings().await else {
            continue;
        };
        let wanted = settings.enabled && settings.state == "ready";
        if !wanted {
            if let Some(mut running) = child.take() {
                stop(&mut running).await;
                laya.set_endpoint(None);
                laya.announce().await;
            }
            continue;
        }
        if let Some(running) = &mut child {
            if !matches!(running.try_wait(), Ok(None)) {
                tracing::warn!("Laya server exited; restarting it");
                child = None;
                laya.set_endpoint(None);
                laya.announce().await;
                retry_at = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
            continue;
        }
        if Instant::now() < retry_at {
            continue;
        }
        laya.set_starting(true);
        match launch(&laya, &settings.checkpoints).await {
            Ok((running, endpoint)) => {
                child = Some(running);
                if health(
                    child.as_mut().expect("started child"),
                    &endpoint,
                    laya.timeouts.laya_serve_start,
                )
                .await
                {
                    laya.set_endpoint(Some(endpoint));
                    laya.announce().await;
                    backoff = laya.timeouts.laya_serve_restart;
                } else {
                    tracing::warn!("Laya server did not become healthy before its startup timeout");
                    let mut running = child.take().expect("started child");
                    stop(&mut running).await;
                    laya.set_endpoint(None);
                    laya.announce().await;
                    retry_at = Instant::now() + backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "starting Laya server failed");
                retry_at = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
        laya.set_starting(false);
    }
}

async fn launch(laya: &Laya, checkpoints: &str) -> anyhow::Result<(Child, String)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let command = laya.serve_command();
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("the configured Laya server is an empty command"))?;
    let models = match checkpoints {
        "all" => "english,multilingual,typed-decisions",
        _ => "english",
    };
    let mut child = guarded(program, args);
    child
        .env("LAYA_HOST", "127.0.0.1")
        .env("LAYA_PORT", port.to_string())
        .env("LAYA_PRELOAD", "1")
        .env("LAYA_MODELS", models)
        .env("HF_HOME", laya.home.join("hf"));
    Ok((child.spawn()?, format!("http://127.0.0.1:{port}")))
}

/// `program` run under [`GUARD`], in a process group of its own. The child
/// keeps the daemon's end of the guard's pipe as its stdin for as long as it
/// is held. [`Child::wait`] closes that stdin first, which kills the server,
/// so a live server is only ever polled with [`Child::try_wait`].
fn guarded(program: &str, args: &[String]) -> ProcessCommand {
    let mut child = ProcessCommand::new("/bin/sh");
    child
        .args(["-c", GUARD, "laya-serve-guard", program])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    child
}

async fn health(child: &mut Child, endpoint: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::new();
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        if client
            .get(format!("{endpoint}/health"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn stop(child: &mut Child) {
    if let Some(group) = child.id().and_then(|pid| Pid::from_raw(pid.cast_signed())) {
        let _ = kill_process_group(group, Signal::KILL);
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(pid: i32) -> bool {
        Pid::from_raw(pid).is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok())
    }

    /// The guard's exit, polled: [`Child::wait`] would close its stdin first.
    async fn exit_of(child: &mut Child) -> std::process::ExitStatus {
        let mut status = None;
        until("the guard to exit", || {
            status = child.try_wait().unwrap();
            status.is_some()
        })
        .await;
        status.unwrap()
    }

    async fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn the_server_dies_when_the_daemon_end_of_its_pipe_closes() {
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("pid");
        let script = format!("echo $$ > '{}'; exec sleep 600", record.display());
        let mut child = guarded("/bin/sh", &["-c".into(), script]).spawn().unwrap();
        until("the server to start", || {
            std::fs::read_to_string(&record).is_ok_and(|pid| pid.ends_with('\n'))
        })
        .await;
        let server: i32 = std::fs::read_to_string(&record)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(alive(server));

        drop(child.stdin.take());

        until("the server to die", || !alive(server)).await;
        assert!(!exit_of(&mut child).await.success());
    }

    #[tokio::test]
    async fn the_guard_exits_with_the_server_status() {
        let mut child = guarded("/bin/sh", &["-c".into(), "exit 7".into()])
            .spawn()
            .unwrap();
        assert_eq!(exit_of(&mut child).await.code(), Some(7));
    }
}
