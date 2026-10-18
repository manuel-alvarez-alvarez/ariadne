//! The local `kev.serve` child and its restart loop.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use ariadne_api::permissions::{Device, Flavour};
use ariadne_store::AiPermissionSettingsUpdate;
use rustix::process::{Pid, Signal, kill_process_group};
use tokio::process::{Child, Command as ProcessCommand};
use tokio::sync::{mpsc, oneshot};

use super::AiPermissions;
use super::decide;
use super::install::run_of;

/// The shell `kev.serve` runs under. It holds the read end of a pipe whose
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

/// The entry point for `cpu` on macOS. PyTorch there picks MPS by itself
/// (`kev/device.py`), so this hides MPS and then runs `kev.serve` as
/// `__main__` with the arguments the daemon passed.
const CPU_ON_MACOS: &str = "import runpy, sys, torch
torch.backends.mps.is_available = lambda: False
sys.argv[0] = 'kev.serve'
runpy.run_module('kev.serve', run_name='__main__', alter_sys=True)
";

pub(crate) enum Command {
    Reconcile,
    Stop(oneshot::Sender<()>),
}

pub(crate) fn start(ai_permissions: AiPermissions, rx: mpsc::UnboundedReceiver<Command>) {
    tokio::spawn(run(ai_permissions, rx));
}

async fn run(ai_permissions: AiPermissions, mut rx: mpsc::UnboundedReceiver<Command>) {
    let mut child = None;
    // The `last_refresh_at` of the install the running child serves. A
    // refresh that ended since then is a new install to serve.
    let mut serving = None;
    let mut retry_at = Instant::now();
    let mut backoff = ai_permissions.timeouts.ai_permissions_serve_restart;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = tick.tick() => {},
            command = rx.recv() => match command {
                Some(Command::Reconcile) => {},
                Some(Command::Stop(done)) => {
                    if let Some(mut child) = child.take() { stop(&mut child).await; }
                    ai_permissions.set_endpoint(None);
                    ai_permissions.announce().await;
                    let _ = done.send(());
                    return;
                }
                None => return,
            }
        }

        let Ok(settings) = ai_permissions.store.ai_permission_settings().await else {
            continue;
        };
        let wanted = settings.enabled && settings.state == "ready";
        if !wanted {
            if let Some(mut running) = child.take() {
                stop(&mut running).await;
                ai_permissions.set_endpoint(None);
                ai_permissions.announce().await;
            }
            continue;
        }
        if let Some(running) = &mut child {
            if !matches!(running.try_wait(), Ok(None)) {
                tracing::warn!("AI permission model server exited; restarting it");
                child = None;
                ai_permissions.set_endpoint(None);
                ai_permissions.announce().await;
                retry_at = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            } else if settings.last_refresh_at != serving {
                // Read from the row, not queued as a command: a queued
                // restart could land after the new server started, and stop
                // it too.
                let mut running = child.take().expect("running child");
                stop(&mut running).await;
                ai_permissions.set_endpoint(None);
                ai_permissions.announce().await;
            }
            continue;
        }
        if Instant::now() < retry_at {
            continue;
        }
        ai_permissions.set_starting(true);
        serving = settings.last_refresh_at.clone();
        let (flavour, device) = ai_permissions.chosen().await;
        let os = ai_permissions.hardware().await.os;
        match launch(&ai_permissions, flavour, device, &os).await {
            Ok((running, endpoint)) => {
                child = Some(running);
                if health(
                    child.as_mut().expect("started child"),
                    &endpoint,
                    ai_permissions.timeouts.ai_permissions_serve_start,
                )
                .await
                {
                    if let Err(reason) = check_device(&endpoint, device).await {
                        tracing::warn!(error = %reason, "the AI permission model server runs on the wrong device");
                        let mut running = child.take().expect("started child");
                        stop(&mut running).await;
                        ai_permissions.set_endpoint(None);
                        ai_permissions
                            .write(AiPermissionSettingsUpdate {
                                state: Some("failed".into()),
                                state_while_enabled: true,
                                last_error: Some(Some(reason)),
                                ..Default::default()
                            })
                            .await;
                    } else {
                        ai_permissions.set_endpoint(Some(endpoint));
                        ai_permissions.announce().await;
                        backoff = ai_permissions.timeouts.ai_permissions_serve_restart;
                    }
                } else {
                    tracing::warn!(
                        "AI permission model server did not become healthy before its startup timeout"
                    );
                    let mut running = child.take().expect("started child");
                    stop(&mut running).await;
                    ai_permissions.set_endpoint(None);
                    ai_permissions.announce().await;
                    retry_at = Instant::now() + backoff;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "starting the AI permission model server failed");
                retry_at = Instant::now() + backoff;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
        ai_permissions.set_starting(false);
    }
}

async fn launch(
    ai_permissions: &AiPermissions,
    flavour: Flavour,
    device: Device,
    os: &str,
) -> anyhow::Result<(Child, String)> {
    spawn(
        &ai_permissions.home,
        ai_permissions.serve_command(),
        flavour,
        device,
        os,
    )
    .await
}

/// Start `kev.serve` under [`GUARD`], on an ephemeral loopback port, exactly
/// as the daemon starts it: the run of `flavour`, the backend of `device`,
/// the model's Hugging Face cache under `home`, offline. `interpreter` is
/// the Python it runs under; this adds the entry point.
pub(crate) async fn spawn(
    home: &Path,
    interpreter: Vec<String>,
    flavour: Flavour,
    device: Device,
    os: &str,
) -> anyhow::Result<(Child, String)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut command = interpreter;
    command.extend(entry_point(device, os));
    command.extend([
        "--run".to_string(),
        run_of(flavour),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        port.to_string(),
    ]);
    let (program, args) = command.split_first().ok_or_else(|| {
        anyhow::anyhow!("the configured AI permission model server is an empty command")
    })?;
    let mut child = guarded(program, args);
    child
        .env("HF_HOME", home.join("hf"))
        .env("HF_HUB_OFFLINE", "1")
        .env("KEV_TEMPERATURE", format!("{:.1}", decide::TEMPERATURE))
        .env_remove("KEV_API_KEY");
    for (key, value) in device_env(device, os) {
        child.env(key, value);
    }
    Ok((child.spawn()?, format!("http://127.0.0.1:{port}")))
}

/// How the interpreter runs `kev.serve`: as a module, or through
/// [`CPU_ON_MACOS`] for `cpu` on macOS.
fn entry_point(device: Device, os: &str) -> [String; 2] {
    match (device, os) {
        (Device::Cpu, "macos") => ["-c".to_string(), CPU_ON_MACOS.to_string()],
        _ => ["-m".to_string(), "kev.serve".to_string()],
    }
}

/// The environment that puts Kev on `device` (`kev/checkpoint.py`,
/// `kev/serve.py`). `cpu` also hides every GPU on Linux, where PyTorch would
/// otherwise take CUDA first.
fn device_env(device: Device, os: &str) -> Vec<(&'static str, &'static str)> {
    match device {
        Device::Mlx => vec![("KEV_BACKEND", "mlx")],
        Device::Cuda => vec![("KEV_BACKEND", "torch")],
        Device::Cpu => {
            let mut env = vec![("KEV_BACKEND", "torch"), ("KEV_DTYPE", "fp32")];
            if os == "linux" {
                env.push(("CUDA_VISIBLE_DEVICES", ""));
            }
            env
        }
    }
}

/// What `/v1/models` must report for `device`: the torch device and the backend.
fn expected_report(device: Device) -> (&'static str, &'static str) {
    match device {
        Device::Mlx => ("mps", "mlx"),
        Device::Cuda => ("cuda", "torch"),
        Device::Cpu => ("cpu", "torch"),
    }
}

/// Check that the server answering at `endpoint` runs where `device` says.
/// The error names what it reported and what was chosen.
async fn check_device(endpoint: &str, device: Device) -> Result<(), String> {
    let card = match reqwest::get(format!("{endpoint}/v1/models")).await {
        Ok(response) => response
            .bytes()
            .await
            .ok()
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok()),
        Err(_) => None,
    };
    let field = |name: &str| {
        card.as_ref()
            .and_then(|card| card["models"][0][name].as_str())
            .unwrap_or("unknown")
            .to_string()
    };
    let (reported_device, reported_backend) = (field("device"), field("backend"));
    let (want_device, want_backend) = expected_report(device);
    if reported_device == want_device && reported_backend == want_backend {
        return Ok(());
    }
    Err(format!(
        "the AI permission model server runs on {reported_device} via {reported_backend}, \
         but the chosen device {} needs {want_device} via {want_backend}",
        device.as_str()
    ))
}

/// `program` run under [`GUARD`], in a process group of its own. The child
/// keeps the daemon's end of the guard's pipe as its stdin for as long as it
/// is held. [`Child::wait`] closes that stdin first, which kills the server,
/// so a live server is only ever polled with [`Child::try_wait`].
fn guarded(program: &str, args: &[String]) -> ProcessCommand {
    let mut child = ProcessCommand::new("/bin/sh");
    child
        .args(["-c", GUARD, "ai-permissions-serve-guard", program])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    child
}

pub(crate) async fn health(child: &mut Child, endpoint: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let client = reqwest::Client::new();
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        if client
            .get(format!("{endpoint}/v1/models"))
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

pub(crate) async fn stop(child: &mut Child) {
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

    /// The macOS `cpu` launcher hides MPS and then runs `kev.serve` as
    /// `__main__` with the daemon's arguments. A stand-in `torch` whose MPS is
    /// there and a stand-in `kev.serve` that prints what it sees prove it.
    #[test]
    fn the_macos_cpu_launcher_hides_mps_and_runs_kev_serve_as_main() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("torch")).unwrap();
        std::fs::create_dir_all(dir.path().join("kev")).unwrap();
        std::fs::write(
            dir.path().join("torch/__init__.py"),
            "class _Mps:\n    @staticmethod\n    def is_available():\n        return True\nclass backends:\n    mps = _Mps\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("kev/__init__.py"), "").unwrap();
        std::fs::write(
            dir.path().join("kev/serve.py"),
            "import json, sys, torch\nif __name__ == '__main__':\n    print(json.dumps({'argv': sys.argv[1:], 'mps': torch.backends.mps.is_available()}))\n",
        )
        .unwrap();

        let [flag, script] = entry_point(Device::Cpu, "macos");
        let output = std::process::Command::new("python3")
            .arg(flag)
            .arg(script)
            .args(["--run", "jaredpalmer/kev-0.8b@9a45d25e", "--port", "8008"])
            .env("PYTHONPATH", dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let saw: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            saw,
            serde_json::json!({
                "argv": ["--run", "jaredpalmer/kev-0.8b@9a45d25e", "--port", "8008"],
                "mps": false,
            })
        );
    }

    #[tokio::test]
    async fn the_guard_exits_with_the_server_status() {
        let mut child = guarded("/bin/sh", &["-c".into(), "exit 7".into()])
            .spawn()
            .unwrap();
        assert_eq!(exit_of(&mut child).await.code(), Some(7));
    }
}
