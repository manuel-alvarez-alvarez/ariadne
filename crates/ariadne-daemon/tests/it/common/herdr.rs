//! A herdr server stand-in that speaks the socket API's wire format.
//!
//! It answers on its own threads, because the daemon pings it from a
//! blocking call and a server on the test's runtime could never reply. Its
//! panes are real processes started from `layout.apply`, so a relay in one
//! outlives the daemon exactly as it does in herdr.
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

#[derive(Default)]
struct State {
    next: u32,
    panes: BTreeMap<String, (String, Child)>,
    requests: Vec<Value>,
}

pub(crate) struct FakeHerdr {
    dir: tempfile::TempDir,
    state: Arc<Mutex<State>>,
}

impl FakeHerdr {
    /// A server that answers every request in the API subset the daemon uses.
    pub(crate) fn start() -> Self {
        Self::start_with(true)
    }

    /// A server that accepts connections and never answers one.
    pub(crate) fn silent() -> Self {
        Self::start_with(false)
    }

    fn start_with(answers: bool) -> Self {
        // A short path: a socket path has a small length limit.
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let listener = UnixListener::bind(dir.path().join("herdr.sock")).unwrap();
        let state = Arc::new(Mutex::new(State::default()));
        let served = state.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let served = served.clone();
                std::thread::spawn(move || {
                    if answers {
                        serve(stream, &served);
                    } else {
                        // Hold the connection open without a word.
                        std::thread::sleep(std::time::Duration::from_secs(60));
                    }
                });
            }
        });
        let binary = dir.path().join("bin");
        std::fs::create_dir(&binary).unwrap();
        let herdr = binary.join("herdr");
        std::fs::write(&herdr, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, state }
    }

    pub(crate) fn socket(&self) -> PathBuf {
        self.dir.path().join("herdr.sock")
    }

    /// A search path holding a `herdr` executable.
    pub(crate) fn search_path(&self) -> std::ffi::OsString {
        let mut paths = vec![self.dir.path().join("bin")];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).unwrap()
    }

    /// The labels of the live panes, in creation order.
    pub(crate) fn labels(&self) -> Vec<String> {
        let mut state = self.state.lock().unwrap();
        reap(&mut state);
        state
            .panes
            .values()
            .map(|(label, _)| label.clone())
            .collect()
    }

    /// Each method received, in order.
    pub(crate) fn methods(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state
            .requests
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The params of every request of `method`.
    pub(crate) fn params_of(&self, method: &str) -> Vec<Value> {
        let state = self.state.lock().unwrap();
        state
            .requests
            .iter()
            .filter(|request| request["method"] == method)
            .map(|request| request["params"].clone())
            .collect()
    }

    /// Whether the process behind a pane is still running.
    pub(crate) fn pane_running(&self, pane_id: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        reap(&mut state);
        state.panes.contains_key(pane_id)
    }
}

impl Drop for FakeHerdr {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap();
        for (_, child) in state.panes.values_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A pane whose process has ended is gone, as in herdr.
fn reap(state: &mut State) {
    state
        .panes
        .retain(|_, (_, child)| matches!(child.try_wait(), Ok(None)));
}

fn serve(stream: UnixStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let request: Value = serde_json::from_str(&line).unwrap();
        line.clear();
        let reply = answer(&request, &mut state.lock().unwrap());
        let mut reply = match reply {
            Ok(result) => json!({"result": result}),
            Err((code, message)) => json!({"error": {"code": code, "message": message}}),
        };
        reply["id"] = request["id"].clone();
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
    }
}

fn answer(request: &Value, state: &mut State) -> Result<Value, (&'static str, String)> {
    let params = &request["params"];
    if request["method"] != "ping" {
        state.requests.push(request.clone());
    }
    reap(state);
    match request["method"].as_str().unwrap_or_default() {
        "ping" => Ok(json!({"type": "pong", "version": "0.9.5", "protocol": 22})),
        "workspace.create" => {
            state.next += 1;
            let n = state.next;
            Ok(json!({
                "type": "workspace_created",
                "workspace": {"workspace_id": format!("w{n}")},
                "tab": {"tab_id": format!("w{n}:t1")},
                "root_pane": {"pane_id": format!("w{n}:p1")},
            }))
        }
        "workspace.close" => Ok(json!({"type": "ok"})),
        "layout.apply" => {
            if params.get("workspace_id").is_some_and(|id| !id.is_null()) {
                return Err((
                    "invalid_target",
                    "use either tab_id or workspace_id, not both".into(),
                ));
            }
            let tab = params["tab_id"].as_str().unwrap_or_default();
            let workspace = tab.split(':').next().unwrap_or_default().to_owned();
            let node = &params["root"];
            let command: Vec<&str> = node["command"]
                .as_array()
                .ok_or(("invalid_params", "no command".to_owned()))?
                .iter()
                .map(|part| part.as_str().unwrap())
                .collect();
            let mut process = Command::new(command[0]);
            process
                .args(&command[1..])
                .env_clear()
                .current_dir(node["cwd"].as_str().unwrap())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            for (key, value) in node["env"].as_object().unwrap() {
                process.env(key, value.as_str().unwrap());
            }
            let child = process
                .spawn()
                .map_err(|error| ("spawn_failed", error.to_string()))?;
            let pane = format!("{workspace}:p1");
            state.panes.insert(
                pane.clone(),
                (node["label"].as_str().unwrap().to_owned(), child),
            );
            Ok(json!({"type": "layout_apply", "layout": {
                "workspace_id": workspace, "tab_id": tab, "zoomed": false,
                "focused_pane_id": pane,
                "root": {"type": "pane", "pane_id": pane, "label": node["label"]},
            }}))
        }
        "pane.list" => Ok(json!({"type": "pane_list", "panes": state.panes.iter().map(
            |(id, (label, _))| json!({"pane_id": id, "label": label, "terminal_id": "t",
                "workspace_id": "w", "tab_id": "t", "focused": false,
                "agent_status": "unknown", "revision": 1}),
        ).collect::<Vec<_>>()})),
        "pane.close" => {
            let id = params["pane_id"].as_str().unwrap_or_default();
            let (_, mut child) = state
                .panes
                .remove(id)
                .ok_or(("not_found", "pane not found".to_owned()))?;
            // A closed pane hangs up its terminal, as herdr's does.
            let _ = Command::new("kill")
                .args(["-HUP", &child.id().to_string()])
                .status();
            let _ = child.wait();
            Ok(json!({"type": "ok"}))
        }
        other => Err(("unknown_method", other.to_owned())),
    }
}
