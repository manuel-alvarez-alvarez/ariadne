//! The local model server.

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::permissions::{AiPermissionsState, AiPermissionsStatusDto};
use ariadne_daemon::ai_permissions::hardware::HardwareOverride;
use ariadne_daemon::timeouts::Timeouts;

use crate::common::{Harness, TIMEOUT, eventually, harness, post, put_json, shared_script};

/// A `kev.serve` stand-in. `$1` is where it records what it was started
/// with, as one JSON object: its pid, the arguments after its own two, and
/// the environment the daemon sets. `$2` is the device and backend its
/// `/v1/models` reports, `device/backend`, or `-` to report the ones its
/// environment chose, as Kev would.
const SERVER: &str = r#"#!/usr/bin/env python3
import http.server, json, os, socketserver, sys
record, report = sys.argv[1], sys.argv[2]
args = sys.argv[3:]
host = args[args.index('--host') + 1]
port = int(args[args.index('--port') + 1])
names = ['HF_HOME', 'HF_HUB_OFFLINE', 'KEV_TEMPERATURE', 'KEV_BACKEND', 'KEV_DTYPE', 'CUDA_VISIBLE_DEVICES']
env = {name: os.environ.get(name) for name in names}
backend = os.environ['KEV_BACKEND']
device = 'mps' if backend == 'mlx' else 'cpu' if os.environ.get('KEV_DTYPE') == 'fp32' else 'cuda'
if report != '-':
    device, backend = report.split('/')
card = json.dumps({"models": [{"name": "kev-latest", "device": device, "backend": backend}]}).encode()
with open(record + '.part', 'w') as f:
    json.dump({"pid": os.getpid(), "args": args, "env": env}, f)
os.replace(record + '.part', record)
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200 if self.path == '/v1/models' else 404)
        self.end_headers()
        if self.path == '/v1/models':
            self.wfile.write(card)
    def log_message(self, *args): pass
class Server(socketserver.TCPServer):
    # Not `http.server.HTTPServer`: it looks up the name of its host as it
    # binds, and that lookup takes 35 s on a GitHub macOS runner.
    allow_reuse_address = True
Server((host, port), Handler).serve_forever()
"#;

/// The run of 4b, what a 64 GB Mac settles on.
const RUN: &str = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101";
const RUN_08B: &str = "jaredpalmer/kev-0.8b@9a45d25eb2ab761841196625383fa1dff0e56c1e";

/// What the server stand-in recorded, once it has started.
#[derive(serde::Deserialize, Clone, Debug)]
struct Started {
    pid: u32,
    args: Vec<String>,
    env: std::collections::HashMap<String, Option<String>>,
}

impl Started {
    fn arg_after(&self, flag: &str) -> &str {
        let at = self.args.iter().position(|arg| arg == flag).unwrap();
        &self.args[at + 1]
    }

    fn env(&self, name: &str) -> Option<&str> {
        self.env.get(name).and_then(|value| value.as_deref())
    }
}

fn started(record: &std::path::Path) -> Option<Started> {
    serde_json::from_str(&std::fs::read_to_string(record).ok()?).ok()
}

/// Wait for a server other than the one with pid `not`, and read what it
/// was started with.
async fn next_server(record: &std::path::Path, not: Option<u32>) -> Started {
    eventually(TIMEOUT, "a new server", || async {
        started(record).is_some_and(|s| Some(s.pid) != not)
    })
    .await;
    started(record).unwrap()
}

fn mac() -> HardwareOverride {
    HardwareOverride {
        os: "macos".into(),
        arch: "aarch64".into(),
        memory_gb: 64,
        gpu_name: None,
        vram_gb: None,
    }
}

fn linux_with_gpu() -> HardwareOverride {
    HardwareOverride {
        os: "linux".into(),
        arch: "x86_64".into(),
        memory_gb: 64,
        gpu_name: Some("test-gpu".into()),
        vram_gb: Some(24),
    }
}

/// A daemon on `hardware` whose server is [`SERVER`], reporting `report`.
async fn serving(
    hardware: HardwareOverride,
    installer: Vec<String>,
    record: &std::path::Path,
    report: &str,
) -> Harness {
    harness()
        .python_bin(python())
        .ai_permissions_hardware(hardware)
        .ai_permissions_installer(installer)
        .ai_permissions_serve_command(vec![
            shared_script(SERVER).display().to_string(),
            record.display().to_string(),
            report.to_string(),
        ])
        .await
}

fn python() -> String {
    shared_script("#!/bin/sh\necho 'Python 3.12.1'\n")
        .display()
        .to_string()
}

fn installer() -> Vec<String> {
    vec!["/usr/bin/true".into()]
}

async fn ready(h: &Harness) -> AiPermissionsStatusDto {
    eventually(TIMEOUT, "the model to be ready", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .state
            == AiPermissionsState::Ready
    })
    .await;
    h.get("/v1/permissions/ai").await
}

fn enable() -> axum::http::Request<Body> {
    put_json("/v1/permissions/ai", json!({"enabled":true}))
}

#[tokio::test]
async fn a_ready_model_starts_the_server_with_its_built_in_weights() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let h = serving(mac(), installer(), record.path(), "-").await;
    let _: AiPermissionsStatusDto = h.json(enable(), StatusCode::OK).await;
    ready(&h).await;
    eventually(TIMEOUT, "the model endpoint", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .endpoint
            .is_some()
    })
    .await;
    let saw = next_server(record.path(), None).await;
    assert_eq!(saw.arg_after("--run"), RUN);
    assert!(saw.env("HF_HOME").unwrap().ends_with("/ai-permissions/hf"));
    assert_eq!(saw.env("HF_HUB_OFFLINE"), Some("1"));
    assert_eq!(saw.env("KEV_TEMPERATURE"), Some("0.6"));
    let _: AiPermissionsStatusDto = h
        .json(
            put_json("/v1/permissions/ai", json!({"enabled":false})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the server endpoint to clear", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .endpoint
            .is_none()
    })
    .await;
    h.state.ai_permissions.shutdown().await;
}

#[tokio::test]
async fn a_refresh_and_an_unexpected_exit_restart_the_server() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let h = serving(mac(), installer(), record.path(), "-").await;
    let _: AiPermissionsStatusDto = h.json(enable(), StatusCode::OK).await;
    ready(&h).await;
    let first = next_server(record.path(), None).await.pid;
    let _: AiPermissionsStatusDto = h
        .json(post("/v1/permissions/ai/refresh"), StatusCode::ACCEPTED)
        .await;
    let second = next_server(record.path(), Some(first)).await;
    // Kill it once it is served: before its device check, the daemon reads
    // no card from it and fails the model rather than restarting it.
    let endpoint = format!("http://127.0.0.1:{}", second.arg_after("--port"));
    eventually(TIMEOUT, "the new server to be served", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .endpoint
            .as_deref()
            == Some(endpoint.as_str())
    })
    .await;
    let second = second.pid;
    let status = std::process::Command::new("/bin/kill")
        .args(["-9", &second.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    next_server(record.path(), Some(second)).await;
    h.state.ai_permissions.shutdown().await;
}

#[tokio::test]
async fn a_server_that_never_answers_health_is_not_live() {
    let h = harness()
        .python_bin(python())
        .ai_permissions_installer(installer())
        .ai_permissions_serve_command(vec!["/bin/sleep".into(), "10".into()])
        .timeouts(Timeouts {
            ai_permissions_serve_start: std::time::Duration::from_millis(100),
            ai_permissions_serve_restart: std::time::Duration::from_secs(10),
            ..Timeouts::default()
        })
        .await;
    let _: AiPermissionsStatusDto = h.json(enable(), StatusCode::OK).await;
    ready(&h).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let status: AiPermissionsStatusDto = h.get("/v1/permissions/ai").await;
    assert_eq!(status.endpoint, None);
    h.state.ai_permissions.shutdown().await;
}

#[tokio::test]
async fn a_ready_enabled_model_starts_after_a_daemon_restart() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let first = serving(mac(), installer(), record.path(), "-").await;
    let _: AiPermissionsStatusDto = first.json(enable(), StatusCode::OK).await;
    ready(&first).await;
    next_server(record.path(), None).await;
    first.state.ai_permissions.shutdown().await;
    std::fs::write(record.path(), "").unwrap();
    let second = ariadne_daemon::ai_permissions::AiPermissions::new(
        first.store.clone(),
        first.bus.clone(),
        &first.launcher.cfg,
        first.timeouts,
    );
    next_server(record.path(), None).await;
    assert_eq!(second.status().await.state, AiPermissionsState::Ready);
    second.shutdown().await;
}

/// On a Mac, `mlx` runs `kev.serve` as a module with the MLX backend, and
/// `cpu` runs it through the launcher that hides MPS, with the torch backend
/// in float32. No GPU is hidden: a Mac has no CUDA to hide.
#[tokio::test]
async fn on_a_mac_mlx_takes_the_mlx_backend_and_cpu_the_launcher() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let h = serving(mac(), installer(), record.path(), "-").await;
    let _: AiPermissionsStatusDto = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "flavour": "0.8b", "device": "mlx"}),
            ),
            StatusCode::OK,
        )
        .await;
    let mlx = next_server(record.path(), None).await;
    assert_eq!(mlx.args[..2], ["-m", "kev.serve"]);
    assert_eq!(mlx.arg_after("--run"), RUN_08B);
    assert_eq!(mlx.env("KEV_BACKEND"), Some("mlx"));
    assert_eq!(mlx.env("KEV_DTYPE"), None);

    let _: AiPermissionsStatusDto = h
        .json(
            put_json("/v1/permissions/ai", json!({"device": "cpu"})),
            StatusCode::OK,
        )
        .await;
    let cpu = next_server(record.path(), Some(mlx.pid)).await;
    assert_eq!(cpu.args[0], "-c");
    assert!(
        cpu.args[1].contains("torch.backends.mps.is_available = lambda: False"),
        "{}",
        cpu.args[1]
    );
    assert!(
        cpu.args[1].contains("runpy.run_module('kev.serve', run_name='__main__'"),
        "{}",
        cpu.args[1]
    );
    assert_eq!(cpu.arg_after("--run"), RUN_08B);
    assert_eq!(cpu.env("KEV_BACKEND"), Some("torch"));
    assert_eq!(cpu.env("KEV_DTYPE"), Some("fp32"));
    assert_eq!(cpu.env("CUDA_VISIBLE_DEVICES"), None);
    h.state.ai_permissions.shutdown().await;
}

/// On Linux, `cuda` takes the torch backend at its default precision, and
/// `cpu` takes torch in float32 with every GPU hidden.
#[tokio::test]
async fn on_linux_cuda_takes_torch_and_cpu_hides_every_gpu() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let h = serving(linux_with_gpu(), installer(), record.path(), "-").await;
    let _: AiPermissionsStatusDto = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "flavour": "4b", "device": "cuda"}),
            ),
            StatusCode::OK,
        )
        .await;
    let cuda = next_server(record.path(), None).await;
    assert_eq!(cuda.args[..2], ["-m", "kev.serve"]);
    assert_eq!(cuda.env("KEV_BACKEND"), Some("torch"));
    assert_eq!(cuda.env("KEV_DTYPE"), None);
    assert_eq!(cuda.env("CUDA_VISIBLE_DEVICES"), None);

    let _: AiPermissionsStatusDto = h
        .json(
            put_json("/v1/permissions/ai", json!({"device": "cpu"})),
            StatusCode::OK,
        )
        .await;
    let cpu = next_server(record.path(), Some(cuda.pid)).await;
    assert_eq!(cpu.args[..2], ["-m", "kev.serve"]);
    assert_eq!(cpu.env("KEV_BACKEND"), Some("torch"));
    assert_eq!(cpu.env("KEV_DTYPE"), Some("fp32"));
    assert_eq!(cpu.env("CUDA_VISIBLE_DEVICES"), Some(""));
    h.state.ai_permissions.shutdown().await;
}

/// A switch while the model is on reinstalls at once, and the server comes
/// back on the new flavour once that install is ready.
#[tokio::test]
async fn a_switch_while_on_reinstalls_and_restarts_the_server_on_the_new_flavour() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let installs = tempfile::NamedTempFile::new().unwrap();
    let installer = vec![
        "/bin/sh".into(),
        "-c".into(),
        "echo \"$AI_PERMISSIONS_FLAVOUR $AI_PERMISSIONS_DEVICE $AI_PERMISSIONS_RUN\" >> \"$1\""
            .into(),
        "installer".into(),
        installs.path().display().to_string(),
    ];
    let h = serving(mac(), installer, record.path(), "-").await;
    let _: AiPermissionsStatusDto = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "flavour": "0.8b"}),
            ),
            StatusCode::OK,
        )
        .await;
    ready(&h).await;
    let first = next_server(record.path(), None).await;
    assert_eq!(first.arg_after("--run"), RUN_08B);

    let switched: AiPermissionsStatusDto = h
        .json(
            put_json("/v1/permissions/ai", json!({"flavour": "4b"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(switched.state, AiPermissionsState::Installing);
    let second = next_server(record.path(), Some(first.pid)).await;
    assert_eq!(second.arg_after("--run"), RUN);
    assert_eq!(
        std::fs::read_to_string(installs.path()).unwrap(),
        format!("0.8b mlx {RUN_08B}\n4b mlx {RUN}\n")
    );
    let status = ready(&h).await;
    assert_eq!(
        status.installed_release.as_deref(),
        Some("kev@f1535963 jaredpalmer/kev-4b@139fdd94 on mlx")
    );
    h.state.ai_permissions.shutdown().await;
}

/// A server whose `/v1/models` names another device than the chosen one is
/// stopped, and the model fails with both named.
#[tokio::test]
async fn a_server_that_reports_the_wrong_device_fails_the_model() {
    let record = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(record.path(), "").unwrap();
    let h = serving(mac(), installer(), record.path(), "cpu/torch").await;
    let _: AiPermissionsStatusDto = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "device": "mlx"}),
            ),
            StatusCode::OK,
        )
        .await;
    let server = next_server(record.path(), None).await;
    eventually(TIMEOUT, "the model to fail", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .state
            == AiPermissionsState::Failed
    })
    .await;
    let status: AiPermissionsStatusDto = h.get("/v1/permissions/ai").await;
    assert_eq!(status.endpoint, None);
    assert_eq!(
        status.last_error.as_deref(),
        Some(
            "the AI permission model server runs on cpu via torch, \
             but the chosen device mlx needs mps via mlx"
        )
    );
    let pid = rustix::process::Pid::from_raw(server.pid.cast_signed()).unwrap();
    eventually(TIMEOUT, "the server to stop", || async {
        rustix::process::test_kill_process(pid).is_err()
    })
    .await;
    h.state.ai_permissions.shutdown().await;
}

/// A switch that lands after the install ended and before it let go of the
/// install is still installed and served. The daemon's Python stand-in holds
/// the status that the install's last write publishes, so the switch lands in
/// exactly that interval. The switch is written straight to the store: a PUT
/// there finds the install busy and only rejoins it, and its own status read
/// would wait on the same stand-in.
#[tokio::test]
async fn a_switch_as_the_install_ends_is_still_installed_and_served() {
    let dir = tempfile::tempdir().unwrap();
    let (gate, waiting, release) = (
        dir.path().join("gate"),
        dir.path().join("waiting"),
        dir.path().join("release"),
    );
    let python = shared_script(&format!(
        "#!/bin/sh\n\
         if [ -f '{gate}' ]; then\n\
           touch '{waiting}'\n\
           while [ ! -f '{release}' ]; do sleep 0.05; done\n\
         fi\n\
         echo 'Python 3.12.1'\n",
        gate = gate.display(),
        waiting = waiting.display(),
        release = release.display(),
    ));
    let hold = dir.path().join("let-go");
    let installs = dir.path().join("installs");
    let record = dir.path().join("server");
    let h = harness()
        .python_bin(python.display().to_string())
        .ai_permissions_hardware(mac())
        .ai_permissions_installer(vec![
            "/bin/sh".into(),
            "-c".into(),
            "echo \"$AI_PERMISSIONS_FLAVOUR\" >> \"$1\"; \
             while [ ! -f \"$2\" ]; do sleep 0.05; done"
                .into(),
            "installer".into(),
            installs.display().to_string(),
            hold.display().to_string(),
        ])
        .ai_permissions_serve_command(vec![
            shared_script(SERVER).display().to_string(),
            record.display().to_string(),
            "-".to_string(),
        ])
        .await;
    let _: AiPermissionsStatusDto = h
        .json(
            put_json(
                "/v1/permissions/ai",
                json!({"enabled": true, "flavour": "0.8b"}),
            ),
            StatusCode::OK,
        )
        .await;

    // The install of 0.8b ends and stops in its last write.
    std::fs::write(&gate, "").unwrap();
    std::fs::write(&hold, "").unwrap();
    eventually(TIMEOUT, "the install's last write to wait", || async {
        waiting.exists()
    })
    .await;
    h.store
        .update_ai_permission_settings(ariadne_store::AiPermissionSettingsUpdate {
            flavour: Some("4b".into()),
            device: Some("mlx".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    std::fs::write(&release, "").unwrap();

    eventually(TIMEOUT, "the switched pair to install", || async {
        h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
            .await
            .installed_release
            .as_deref()
            == Some("kev@f1535963 jaredpalmer/kev-4b@139fdd94 on mlx")
    })
    .await;
    assert_eq!(std::fs::read_to_string(&installs).unwrap(), "0.8b\n4b\n");
    let status = ready(&h).await;
    assert_eq!(status.flavour.as_str(), "4b");
    eventually(TIMEOUT, "the server on the switched pair", || async {
        started(&record).is_some_and(|s| s.arg_after("--run") == RUN)
            && h.get::<AiPermissionsStatusDto>("/v1/permissions/ai")
                .await
                .endpoint
                .is_some()
    })
    .await;
    h.state.ai_permissions.shutdown().await;
}
