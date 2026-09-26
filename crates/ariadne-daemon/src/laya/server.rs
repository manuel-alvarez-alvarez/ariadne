//! The local `laya-serve` child and its restart loop.

use std::process::Stdio;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::{mpsc, oneshot};

use super::Laya;

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
    let mut child = ProcessCommand::new(program);
    child
        .args(args)
        .env("LAYA_HOST", "127.0.0.1")
        .env("LAYA_PORT", port.to_string())
        .env("LAYA_PRELOAD", "1")
        .env("LAYA_MODELS", models)
        .env("HF_HOME", laya.home.join("hf"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    Ok((child.spawn()?, format!("http://127.0.0.1:{port}")))
}

async fn health(child: &mut Child, endpoint: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::new();
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        if client
            .post(format!("{endpoint}/v1/systemone"))
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
