//! Portable daemon settings through the worker API.
use super::common::{harness, put_json};
use axum::http::StatusCode;
use serde_json::{Value, json};

fn settings() -> Value {
    json!({
        "delete_merged_branches": false, "delete_merged_worktrees": false,
        "auto_switch": false, "exhausted_patterns": ["credits spent"],
        "log_filter": "warn", "telemetry_samples_days": 3,
        "telemetry_logs_days": 4, "telemetry_rollups_days": 30,
        "acp_registry_url": "https://example.invalid/registry.json",
        "acp_agents": [], "agent_flags": {}
    })
}

#[tokio::test]
async fn portable_settings_apply_without_changing_worker_paths() {
    let h = harness().await;
    let socket = h.launcher.cfg.socket_path.clone();
    let db = h.launcher.cfg.db_path.clone();
    let result: Value = h
        .json(put_json("/v1/config/sync", settings()), StatusCode::OK)
        .await;
    assert_eq!(result["state"], "pending_restart");
    let result: Value = h.get("/v1/config/sync").await;
    assert_eq!(result["configuration"]["auto_switch"], false);
    assert_eq!(result["configuration"]["telemetry_logs_days"], 4);
    assert_eq!(result["configuration"]["log_filter"], "warn");
    assert_eq!(h.launcher.cfg.socket_path, socket);
    assert_eq!(h.launcher.cfg.db_path, db);
    let restored = ariadne_daemon::config::Config::load(Some(h.launcher.cfg.root.clone())).unwrap();
    assert!(!restored.auto_switch);
    assert_eq!(restored.telemetry_logs_days, 4);
    assert_eq!(restored.socket_path, socket);
}

#[tokio::test]
async fn an_unmapped_source_path_fails_without_applying_settings() {
    let h = harness().await;
    for path in [
        "/source/bin/agent",
        "~/bin/agent",
        "./bin/agent",
        "../bin/agent",
        "C:/source/bin/agent",
        r"C:\source\bin\agent",
    ] {
        let mut request = settings();
        request["acp_agents"] = json!([{"id":"source-only", "command":[path]}]);
        let result: Value = h
            .json(
                put_json("/v1/config/sync", request),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(
            result.to_string().contains("source-only"),
            "{path}: {result}"
        );
        assert!(result.to_string().contains("mapping"), "{path}: {result}");
        assert!(
            !h.launcher.cfg.root.join("portable-config.json").exists(),
            "{path}"
        );
    }
}

#[tokio::test]
async fn invalid_retention_fails_before_any_setting_changes() {
    let h = harness().await;
    let mut request = settings();
    request["telemetry_logs_days"] = json!(-1);
    h.error(
        put_json("/v1/config/sync", request),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(!h.launcher.cfg.root.join("portable-config.json").exists());
}

struct Daemon(std::process::Child);
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct SshChildren(std::path::PathBuf);
impl Drop for SshChildren {
    fn drop(&mut self) {
        if let Ok(pids) = std::fs::read_to_string(&self.0) {
            for pid in pids
                .lines()
                .filter_map(|line| line.parse().ok())
                .filter_map(rustix::process::Pid::from_raw)
            {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
        }
    }
}

#[tokio::test]
async fn an_idle_worker_restarts_with_an_open_event_stream() {
    use super::common::{TIMEOUT, eventually, store_copy};
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    use std::os::unix::fs::PermissionsExt;
    let ssh = dir.path().join("fake-ssh");
    let children = SshChildren(dir.path().join("ssh-pids"));
    std::fs::write(&ssh, format!("#!/usr/bin/env python3\nimport os, signal\nwith open({:?}, 'a') as output: output.write(str(os.getpid()) + '\\n')\nsignal.pause()\n", children.0)).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "tcp_listen = \"127.0.0.1:{port}\"\nprevent_sleep = false\nssh_bin = {:?}\n",
            ssh
        ),
    )
    .unwrap();
    let daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_ariadned"))
            .arg("--home")
            .arg(&home)
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", dir.path())
            .env_remove("RUST_LOG")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = ariadne_client::Client::for_home(Some(home.clone()));
    eventually(TIMEOUT, "worker health", || async {
        client.health().await.is_ok()
    })
    .await;
    let _: Value = client
        .post_json(
            "/v1/hosts",
            &json!({"name": "unanswered", "endpoint": "ssh://unanswered"}),
        )
        .await
        .unwrap();
    eventually(TIMEOUT, "first SSH child", || async {
        std::fs::read_to_string(&children.0).is_ok_and(|text| !text.is_empty())
    })
    .await;
    let old_pid = std::fs::read_to_string(&children.0)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let old_pid = rustix::process::Pid::from_raw(old_pid).unwrap();
    let stream = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/v1/events/stream"))
        .send()
        .await
        .unwrap();
    assert!(stream.status().is_success());
    let response: Value = client
        .put_json("/v1/config/sync", &settings())
        .await
        .unwrap();
    assert_eq!(response["state"], "pending_restart");
    eventually(
        TIMEOUT,
        "configuration applied after replacement",
        || async {
            client
                .get_json::<Value>("/v1/config/sync")
                .await
                .is_ok_and(|s| s["state"] == "in_sync")
        },
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(home.join("ariadned.pid")).unwrap(),
        daemon.0.id().to_string()
    );
    let response: Value = client.get_json("/v1/config/sync").await.unwrap();
    assert_eq!(
        response["configuration"]["acp_registry_url"],
        "https://example.invalid/registry.json"
    );
    eventually(
        TIMEOUT,
        "old SSH forward to stop after replacement",
        || async { rustix::process::test_kill_process(old_pid).is_err() },
    )
    .await;
    drop(stream);
}

#[tokio::test]
async fn worker_commands_and_worker_only_agents_survive_source_replacement() {
    let h = harness().second_agent().await;
    let original = h.launcher.cfg.acp_agents.clone();
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    request["acp_agents"] = json!([{"id":"stub", "command":["/source/bin/stub", "--source"]}]);
    request["agent_flags"] = json!({"stub":["--shared"]});
    h.store
        .update_agent_config("stub2", vec!["--worker-only".into()])
        .await
        .unwrap();
    let result: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    assert_eq!(result["state"], "in_sync");
    assert_eq!(
        result["configuration"]["acp_agents"],
        json!(
            original
                .iter()
                .map(|a| json!({"id":a.id,"command":a.command}))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(h.store.agent_flags("stub").await.unwrap(), ["--shared"]);
    assert_eq!(
        h.store.agent_flags("stub2").await.unwrap(),
        ["--worker-only"]
    );
    assert!(!h.launcher.cfg.live_settings().auto_switch);
    assert_eq!(
        h.launcher.cfg.live_settings().exhausted_patterns,
        ["credits spent"]
    );
}

#[tokio::test]
async fn maintenance_keeps_admitted_tasks_running_and_holds_new_tasks() {
    use super::common::{TIMEOUT, eventually, test_pin};
    use ariadne_core::TaskStatus;
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.activate(&cast.goal).await;
    eventually(TIMEOUT, "first task admission", || async {
        h.status(&cast.task.id).await == TaskStatus::InProgress
    })
    .await;
    let _: Value = h
        .json(put_json("/v1/config/sync", settings()), StatusCode::OK)
        .await;
    let queued = h
        .task_on(
            &cast.goal,
            &cast.repo,
            "queued during maintenance",
            test_pin(),
        )
        .await;
    h.notify(&queued.id);
    h.flush_scheduler().await;
    assert_ne!(h.status(&queued.id).await, TaskStatus::InProgress);
    assert!(h.sessions_of(&queued.id).await.is_empty());
    assert!(
        !h.launcher
            .cfg
            .portable
            .restart_ready(&h.store)
            .await
            .unwrap()
    );
    let steps = h.store.goal_steps(&cast.goal.id).await.unwrap();
    h.advance_to(&cast.task, &steps[1].id).await;
    h.notify(&cast.task.id);
    h.flush_scheduler().await;
    assert!(
        h.sessions_of(&cast.task.id)
            .await
            .iter()
            .any(|s| s.task_agent_id.as_deref() == Some(&cast.agents[1].id)),
        "task: {:?}; sessions: {:?}; agents: {:?}; steps: {:?}",
        h.store.get_task(&cast.task.id).await.unwrap(),
        h.sessions_of(&cast.task.id).await,
        cast.agents,
        steps
    );
    assert!(
        !h.launcher
            .cfg
            .portable
            .restart_ready(&h.store)
            .await
            .unwrap()
    );
    h.launcher
        .cfg
        .portable
        .restart_failed("replacement executable is unavailable".into());
    h.notify(&queued.id);
    h.flush_scheduler().await;
    assert_eq!(h.status(&queued.id).await, TaskStatus::InProgress);
    let result: Value = h.get("/v1/config/sync").await;
    assert_eq!(result["state"], "failed");
    assert_eq!(result["error"], "replacement executable is unavailable");
}

#[tokio::test]
async fn a_failed_process_replacement_keeps_the_worker_available_and_can_retry() {
    use super::common::{TIMEOUT, eventually, store_copy};
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    std::fs::write(home.join("config.toml"), "prevent_sleep = false\n").unwrap();
    let binary = dir.path().join("ariadned");
    std::fs::copy(env!("CARGO_BIN_EXE_ariadned"), &binary).unwrap();
    let _daemon = Daemon(
        std::process::Command::new(&binary)
            .arg("--home")
            .arg(&home)
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = ariadne_client::Client::for_home(Some(home));
    eventually(TIMEOUT, "worker health", || async {
        client.health().await.is_ok()
    })
    .await;
    let original: Value = client.get_json("/v1/config/sync").await.unwrap();
    std::fs::remove_file(binary).unwrap();
    let _: Value = client
        .put_json("/v1/config/sync", &settings())
        .await
        .unwrap();
    eventually(TIMEOUT, "replacement failure", || async {
        client
            .get_json::<Value>("/v1/config/sync")
            .await
            .is_ok_and(|s| {
                s["state"] == "failed"
                    && s["error"]
                        .as_str()
                        .is_some_and(|error| error.contains("executable"))
            })
    })
    .await;
    assert!(client.health().await.is_ok());
    let restored: Value = client
        .put_json("/v1/config/sync", &original["configuration"])
        .await
        .unwrap();
    assert_eq!(restored["state"], "in_sync");
    assert!(restored["error"].is_null());
}

#[tokio::test]
async fn a_running_retention_job_uses_the_new_cutoff() {
    use super::common::{RUNS_OUT, TIMEOUT, eventually};
    use ariadne_store::telemetry::NewLogRecord;
    let h = harness().await;
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    request["telemetry_logs_days"] = json!(365_000);
    let _: Value = h
        .json(put_json("/v1/config/sync", request.clone()), StatusCode::OK)
        .await;
    ariadne_daemon::telemetry::retention::start_live(
        h.telemetry_store.clone(),
        RUNS_OUT / 10,
        h.launcher.cfg.clone(),
    );
    let old = (chrono::Utc::now() - chrono::Duration::days(3)).to_rfc3339();
    h.telemetry_store
        .write_logs(&[NewLogRecord {
            ts: old.clone(),
            observed_ts: old,
            severity_number: Some(9),
            severity_text: Some("INFO".into()),
            body: Some("retire after the policy change".into()),
            service: Some("test".into()),
            scope: None,
            target: None,
            attributes: json!({}),
            trace_id: None,
            span_id: None,
        }])
        .await
        .unwrap();
    assert_eq!(
        h.telemetry_store
            .read_logs(Default::default())
            .await
            .unwrap()
            .len(),
        1
    );
    request["telemetry_logs_days"] = json!(1);
    let _: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    eventually(TIMEOUT, "retention to use the live cutoff", || async {
        h.telemetry_store
            .read_logs(Default::default())
            .await
            .unwrap()
            .is_empty()
    })
    .await;
}

#[tokio::test]
async fn a_live_cleanup_policy_keeps_the_finished_worktree() {
    use super::common::{TIMEOUT, eventually};
    use ariadne_core::TaskStatus;
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.activate(&cast.goal).await;
    h.flush_scheduler().await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let path = std::path::PathBuf::from(task.worktree_path.unwrap());
    assert!(path.is_dir());
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    let _: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    h.store
        .end_step(
            &cast.task.id,
            "develop",
            TaskStatus::Finished,
            "finished for cleanup",
            None,
        )
        .await
        .unwrap();
    h.notify(&cast.task.id);
    h.flush_scheduler().await;
    eventually(TIMEOUT, "finished task runtimes to stop", || async {
        h.sessions_of(&cast.task.id)
            .await
            .iter()
            .all(|session| !h.launcher.acp.is_running(&session.id))
    })
    .await;
    h.flush_scheduler().await;
    assert!(path.is_dir());
}

#[derive(Clone)]
struct LogCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_logging_filter_changes_without_a_restart() {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::layer::SubscriberExt;
    let h = harness().await;
    let bytes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = bytes.clone();
    let (filter, reload) = tracing_subscriber::reload::Layer::new(EnvFilter::new("error"));
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(move || LogCapture(captured.clone())),
    );
    let dispatch = tracing::Dispatch::new(subscriber);
    h.launcher.cfg.portable.set_log_reload(move |value| {
        reload.reload(EnvFilter::try_new(value)?)?;
        Ok(())
    });
    tracing::dispatcher::with_default(&dispatch, || tracing::debug!("before the live update"));
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    request["log_filter"] = json!("debug");
    let result: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    assert_eq!(result["state"], "in_sync");
    tracing::dispatcher::with_default(&dispatch, || tracing::debug!("after the live update"));
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    assert!(!output.contains("before the live update"));
    assert!(output.contains("after the live update"));
}

#[tokio::test]
async fn an_existing_runtime_classifies_errors_with_the_new_patterns() {
    use super::common::{TIMEOUT, acp, eventually};
    use ariadne_core::AttentionReason;
    let h = harness().await;
    h.git_repo("repo");
    let cast = h.active_cast().await;
    let mut script = acp::script();
    script["prompts"] = json!([{"error":{"code":-32000,"message":"credits spent"}}]);
    h.agent.reprogram(script);
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    let _: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    let session = h.start_agent(&cast.task, "develop").await;
    eventually(TIMEOUT, "the updated exhaustion pattern", || async {
        h.attention(&session).await == Some(AttentionReason::Exhausted)
    })
    .await;
}

#[tokio::test]
async fn provider_model_flags_remain_portable_with_live_policy_updates() {
    let h = harness().await;
    let mut request = settings();
    request["acp_registry_url"] = json!(h.launcher.cfg.acp_registry_url);
    request["agent_flags"] = json!({"stub":["--model", "anthropic/claude-sonnet"]});
    let result: Value = h
        .json(put_json("/v1/config/sync", request), StatusCode::OK)
        .await;
    assert_eq!(result["state"], "in_sync");
    assert_eq!(result["configuration"]["log_filter"], "warn");
    assert_eq!(result["configuration"]["delete_merged_worktrees"], false);
    assert_eq!(result["configuration"]["telemetry_logs_days"], 4);
    assert_eq!(
        h.store.agent_flags("stub").await.unwrap(),
        ["--model", "anthropic/claude-sonnet"]
    );
}

#[tokio::test]
async fn scoped_npx_package_arguments_remain_portable() {
    use super::common::{TIMEOUT, acp, eventually, store_copy};
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    store_copy(&home.join("ariadne.db")).await;
    std::fs::write(home.join("config.toml"), "prevent_sleep = false\n").unwrap();
    let stub_dir = dir.path().join("stub");
    std::fs::create_dir_all(&stub_dir).unwrap();
    let stub = acp::stub_acp_agent(&stub_dir, acp::script());
    let paths = std::env::join_paths([
        std::path::PathBuf::from(stub.path_with(&["npx"])),
        "/usr/bin".into(),
        "/bin".into(),
    ])
    .unwrap();
    let _daemon = Daemon(
        std::process::Command::new(env!("CARGO_BIN_EXE_ariadned"))
            .arg("--home")
            .arg(&home)
            .env("PATH", paths)
            .env("HOME", dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = ariadne_client::Client::for_home(Some(home));
    eventually(TIMEOUT, "worker health", || async {
        client.health().await.is_ok()
    })
    .await;
    let mut request = settings();
    request["acp_agents"] =
        json!([{"id":"portable-package", "command":["npx", "-y", "@scope/pkg"]}]);
    let result: Value = client.put_json("/v1/config/sync", &request).await.unwrap();
    assert_eq!(result["state"], "pending_restart");
    assert_eq!(
        result["configuration"]["acp_agents"],
        json!([{"id":"portable-package", "command":["npx", "-y", "@scope/pkg"]}])
    );
    assert_eq!(result["configuration"]["telemetry_logs_days"], 4);
}

#[tokio::test]
async fn explicit_maintenance_survives_restart_and_restores_admission() {
    use super::common::post_json;
    let h = harness().await;
    let result: Value = h
        .json(
            post_json("/v1/maintenance", json!({"active":true})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(result["ready"], true);
    let restored = ariadne_daemon::config::Config::load(Some(h.launcher.cfg.root.clone())).unwrap();
    assert!(restored.root.join("worker-maintenance.json").exists());
    assert!(restored.portable.restart_ready(&h.store).await.unwrap());
    let _: Value = h
        .json(
            post_json("/v1/maintenance", json!({"active":false})),
            StatusCode::OK,
        )
        .await;
    assert!(!h.launcher.cfg.root.join("worker-maintenance.json").exists());
}

#[tokio::test]
async fn explicit_updates_hold_new_tasks_until_maintenance_ends() {
    use super::common::{TIMEOUT, eventually, test_pin};
    use ariadne_core::TaskStatus;
    let h = harness().scheduler().await;
    h.git_repo("repo");
    let cast = h.cast().await;
    h.activate(&cast.goal).await;
    eventually(TIMEOUT, "first task admission", || async {
        h.status(&cast.task.id).await == TaskStatus::InProgress
    })
    .await;
    let _: Value = h
        .json(
            super::common::post_json("/v1/maintenance", json!({"active":true})),
            StatusCode::OK,
        )
        .await;
    let queued = h
        .task_on(
            &cast.goal,
            &cast.repo,
            "queued during maintenance",
            test_pin(),
        )
        .await;
    h.notify(&queued.id);
    h.flush_scheduler().await;
    assert_ne!(h.status(&queued.id).await, TaskStatus::InProgress);
    assert!(h.sessions_of(&queued.id).await.is_empty());
    assert!(
        !h.launcher
            .cfg
            .portable
            .restart_ready(&h.store)
            .await
            .unwrap()
    );
    let steps = h.store.goal_steps(&cast.goal.id).await.unwrap();
    h.advance_to(&cast.task, &steps[1].id).await;
    h.notify(&cast.task.id);
    h.flush_scheduler().await;
    assert!(
        h.sessions_of(&cast.task.id)
            .await
            .iter()
            .any(|s| s.task_agent_id.as_deref() == Some(&cast.agents[1].id)),
        "task: {:?}; sessions: {:?}; agents: {:?}; steps: {:?}",
        h.store.get_task(&cast.task.id).await.unwrap(),
        h.sessions_of(&cast.task.id).await,
        cast.agents,
        steps
    );
    assert!(
        !h.launcher
            .cfg
            .portable
            .restart_ready(&h.store)
            .await
            .unwrap()
    );
    let _: Value = h
        .json(
            super::common::post_json("/v1/maintenance", json!({"active":false})),
            StatusCode::OK,
        )
        .await;
    h.notify(&queued.id);
    h.flush_scheduler().await;
    assert_eq!(h.status(&queued.id).await, TaskStatus::InProgress);
}
