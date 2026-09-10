//! The client side of the ACP adapter.
//!
//! An ACP agent is a JSON-RPC server over standard input and output. The
//! daemon writes its setup into `acp.json`; `_spawn` starts the configured
//! executable, drives the protocol, and translates updates into Ariadne's
//! existing agent-event vocabulary.

use std::io::{IsTerminal, Write as _};
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Command;

use ariadne_core::acp::{self, Hook, LaunchConfig};
use ariadne_core::spawn_plan::SpawnPlanFile;

/// Launch the plan's ACP server and drive its session until the pane closes.
pub async fn run(plan: &SpawnPlanFile, config_path: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading the ACP config {}", config_path.display()))?;
    let config: LaunchConfig = serde_json::from_str(&raw)
        .with_context(|| format!("reading the ACP config {}", config_path.display()))?;
    if config.version != acp::VERSION {
        bail!(
            "ACP config version {} is not supported; this client supports {}",
            config.version,
            acp::VERSION
        );
    }

    let (program, args) = plan
        .argv
        .split_first()
        .expect("spawn-plan validation rejects an empty argv");
    let mut child = Command::new(program)
        .args(args)
        .envs(plan.env.iter().cloned())
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting ACP agent {program} in {}", plan.cwd.display()))?;
    let stdin = child
        .stdin
        .take()
        .context("opening the ACP agent's stdin")?;
    let stdout = child
        .stdout
        .take()
        .context("opening the ACP agent's stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("opening the ACP agent's stderr")?;
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("{line}");
        }
    });

    let sink = EventSink::new(config.event_sink.clone(), plan.env.clone());
    let internal_id = config.resume_session_id.clone();
    let result = run_protocol(
        BufReader::new(stdout),
        stdin,
        &plan.cwd,
        config,
        sink.clone(),
        true,
    )
    .await;
    if let Err(error) = &result {
        sink.emit(
            "session.error",
            json!({"session_id": internal_id, "error": {"data": {"message": error.to_string()}}}),
        )
        .await;
        sink.emit("session_end", json!({"session_id": internal_id}))
            .await;
    }
    drop(child.start_kill());
    result
}

#[derive(Clone)]
struct EventSink {
    hook: Hook,
    env: Vec<(String, String)>,
}

impl EventSink {
    fn new(hook: Hook, env: Vec<(String, String)>) -> Self {
        Self { hook, env }
    }

    /// Event reporting is fail-safe, like every agent hook.
    async fn emit(&self, kind: &str, payload: Value) {
        let body = json!({"kind": kind, "payload": payload}).to_string();
        let mut command = Command::new(&self.hook.command);
        command
            .args(&self.hook.args)
            .arg("--json")
            .arg(body)
            .envs(self.env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = command.status().await;
    }
}

struct Rpc<R, W> {
    reader: tokio::io::Lines<R>,
    writer: BufWriter<W>,
    next_id: u64,
    sink: EventSink,
    assistant_text: String,
}

impl<R, W> Rpc<R, W>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    fn new(reader: R, writer: W, sink: EventSink) -> Self {
        Self {
            reader: reader.lines(),
            writer: BufWriter::new(writer),
            next_id: 1,
            sink,
            assistant_text: String::new(),
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;

        loop {
            let line = self
                .reader
                .next_line()
                .await
                .context("reading from the ACP agent")?
                .ok_or_else(|| anyhow!("ACP agent closed stdout during {method}"))?;
            let message: Value = serde_json::from_str(&line)
                .with_context(|| format!("reading ACP message `{line}`"))?;
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                if let Some(error) = message.get("error") {
                    bail!("ACP {method} failed: {error}");
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow!("ACP {method} response has no result"));
            }
            self.handle_incoming(message).await?;
        }
    }

    async fn write(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .await
            .context("writing to the ACP agent")?;
        self.writer.flush().await.context("flushing ACP request")
    }

    async fn handle_incoming(&mut self, message: Value) -> Result<()> {
        match message.get("method").and_then(Value::as_str) {
            Some("session/update") => self.handle_update(&message["params"]).await,
            Some("session/request_permission") if message.get("id").is_some() => {
                self.handle_permission(&message).await
            }
            Some(_) if message.get("id").is_some() => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": message["id"],
                    "error": {"code": -32601, "message": "method not supported"},
                });
                self.write(&response).await
            }
            _ => Ok(()),
        }
    }

    async fn handle_update(&mut self, params: &Value) -> Result<()> {
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let update = params.get("update").cloned().unwrap_or_default();
        let update_kind = update.get("sessionUpdate").and_then(Value::as_str);
        match update_kind {
            Some("agent_message_chunk") => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    print!("{text}");
                    std::io::stdout().flush().context("printing ACP output")?;
                    self.assistant_text.push_str(text);
                }
            }
            Some("tool_call") => {
                self.sink
                    .emit("pre_tool_use", tool_payload(session_id, &update))
                    .await;
            }
            Some("tool_call_update") if terminal_tool_status(&update) => {
                self.sink
                    .emit("post_tool_use", tool_payload(session_id, &update))
                    .await;
            }
            Some("compaction_update")
                if update.get("status").and_then(Value::as_str) == Some("completed") =>
            {
                let mut payload = update;
                payload["session_id"] = session_id;
                self.sink.emit("compaction_update", payload).await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_permission(&mut self, message: &Value) -> Result<()> {
        let params = &message["params"];
        let session_id = params.get("sessionId").cloned().unwrap_or(Value::Null);
        let mut payload = tool_payload(session_id.clone(), &params["toolCall"]);
        payload["options"] = params.get("options").cloned().unwrap_or_default();
        self.sink.emit("permission_request", payload).await;

        let selected = choose_permission(params).await?;
        let outcome = selected.as_ref().map_or_else(
            || json!({"outcome": "cancelled"}),
            |option_id| json!({"outcome": "selected", "optionId": option_id}),
        );
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": {"outcome": outcome},
        }))
        .await?;
        self.sink
            .emit(
                "permission.replied",
                json!({"session_id": session_id, "option_id": selected}),
            )
            .await;
        Ok(())
    }
}

async fn run_protocol<R, W>(
    reader: R,
    writer: W,
    cwd: &Path,
    config: LaunchConfig,
    sink: EventSink,
    interactive: bool,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut rpc = Rpc::new(reader, writer, sink.clone());
    let initialized = rpc
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                    "session": {"configOptions": {}, "compaction": {}},
                    "auth": {},
                },
                "clientInfo": {"name": "ariadne", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await?;
    if initialized.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
        bail!("ACP agent did not negotiate protocol version 1");
    }

    let setup = session_setup(&mut rpc, cwd, &config, &initialized).await?;
    let session_id = setup
        .get("sessionId")
        .and_then(Value::as_str)
        .or(config.resume_session_id.as_deref())
        .ok_or_else(|| anyhow!("ACP session setup returned no session id"))?
        .to_string();
    let options = setup
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let options = set_pinned_option(
        &mut rpc,
        &session_id,
        options,
        &["model"],
        &["model"],
        "model",
        &config.model,
    )
    .await?;
    if let Some(effort) = &config.effort {
        set_pinned_option(
            &mut rpc,
            &session_id,
            options,
            &["thought_level"],
            &["effort", "reasoning", "thought_level"],
            "effort",
            effort,
        )
        .await?;
    }

    sink.emit("session_start", json!({"session_id": session_id}))
        .await;
    if let Some(prompt) = config.initial_prompt.as_deref() {
        prompt_once(&mut rpc, &session_id, &config.system_prompt, prompt).await?;
    }
    if interactive && std::io::stdin().is_terminal() {
        while let Some(prompt) = read_prompt().await? {
            if !prompt.trim().is_empty() {
                prompt_once(&mut rpc, &session_id, &config.system_prompt, &prompt).await?;
            }
        }
    }
    sink.emit("session_end", json!({"session_id": session_id}))
        .await;
    Ok(())
}

async fn session_setup<R, W>(
    rpc: &mut Rpc<R, W>,
    cwd: &Path,
    config: &LaunchConfig,
    initialized: &Value,
) -> Result<Value>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let cwd = cwd.display().to_string();
    let mcp_servers = serde_json::to_value(&config.mcp_servers)?;
    let Some(session_id) = &config.resume_session_id else {
        return rpc
            .request(
                "session/new",
                json!({"cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await;
    };
    let capabilities = &initialized["agentCapabilities"];
    let resume = &capabilities["sessionCapabilities"]["resume"];
    if resume.is_object() || resume.as_bool() == Some(true) {
        let mut result = rpc
            .request(
                "session/resume",
                json!({"sessionId": session_id, "cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await?;
        result["sessionId"] = Value::String(session_id.clone());
        return Ok(result);
    }
    if capabilities.get("loadSession").and_then(Value::as_bool) == Some(true) {
        let mut result = rpc
            .request(
                "session/load",
                json!({"sessionId": session_id, "cwd": cwd, "mcpServers": mcp_servers}),
            )
            .await?;
        result["sessionId"] = Value::String(session_id.clone());
        return Ok(result);
    }
    bail!("ACP agent supports neither session/resume nor session/load")
}

async fn set_pinned_option<R, W>(
    rpc: &mut Rpc<R, W>,
    session_id: &str,
    options: Vec<Value>,
    categories: &[&str],
    names: &[&str],
    label: &str,
    value: &str,
) -> Result<Vec<Value>>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let option = options
        .iter()
        .find(|option| {
            option
                .get("category")
                .and_then(Value::as_str)
                .is_some_and(|category| categories.contains(&category))
        })
        .or_else(|| {
            options.iter().find(|option| {
                [option.get("id"), option.get("name")]
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|candidate| {
                        names
                            .iter()
                            .any(|name| candidate.eq_ignore_ascii_case(name))
                    })
            })
        })
        .ok_or_else(|| anyhow!("ACP agent did not offer a {label} configuration option"))?;
    let config_id = option
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("ACP {label} configuration option has no id"))?;
    let response = rpc
        .request(
            "session/set_config_option",
            json!({"sessionId": session_id, "configId": config_id, "value": value}),
        )
        .await
        .with_context(|| format!("setting ACP {label} to `{value}`"))?;
    Ok(response
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or(options))
}

async fn prompt_once<R, W>(
    rpc: &mut Rpc<R, W>,
    session_id: &str,
    system_prompt: &str,
    prompt: &str,
) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let prompt = format!("{system_prompt}\n\n{prompt}");
    rpc.assistant_text.clear();
    rpc.sink
        .emit(
            "user_prompt_submit",
            json!({"session_id": session_id, "prompt": prompt}),
        )
        .await;
    let response = rpc
        .request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": prompt}],
            }),
        )
        .await?;
    if !rpc.assistant_text.ends_with('\n') {
        println!();
    }
    rpc.sink
        .emit(
            "stop",
            json!({
                "session_id": session_id,
                "stop_reason": response.get("stopReason"),
                "last_assistant_message": rpc.assistant_text.clone(),
            }),
        )
        .await;
    Ok(())
}

fn tool_payload(session_id: Value, update: &Value) -> Value {
    let name = update
        .get("title")
        .or_else(|| update.get("toolCallId"))
        .cloned()
        .unwrap_or_else(|| Value::String("ACP tool".into()));
    json!({
        "session_id": session_id,
        "tool_name": name,
        "tool_input": update.get("rawInput").cloned().unwrap_or_default(),
        "acp": update,
    })
}

fn terminal_tool_status(update: &Value) -> bool {
    matches!(
        update.get("status").and_then(Value::as_str),
        Some("completed" | "failed")
    )
}

async fn choose_permission(params: &Value) -> Result<Option<String>> {
    let options = params
        .get("options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let title = params
        .pointer("/toolCall/title")
        .and_then(Value::as_str)
        .unwrap_or("ACP tool");
    eprintln!("Permission requested for {title}:");
    for (index, option) in options.iter().enumerate() {
        let name = option
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("option");
        eprintln!("  {}. {name}", index + 1);
    }
    eprint!("> ");
    std::io::stderr().flush()?;
    let answer = tokio::task::spawn_blocking(|| {
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).map(|_| answer)
    })
    .await
    .context("joining the permission prompt")??;
    let selected = answer
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| options.get(index.saturating_sub(1)))
        .and_then(|option| option.get("optionId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(selected)
}

async fn read_prompt() -> Result<Option<String>> {
    eprint!("> ");
    std::io::stderr().flush()?;
    tokio::task::spawn_blocking(|| {
        let mut prompt = String::new();
        let count = std::io::stdin().read_line(&mut prompt)?;
        Ok((count != 0).then_some(prompt))
    })
    .await
    .context("joining the ACP prompt reader")?
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;

    use tokio::io::{BufReader, DuplexStream, duplex};

    fn option(id: &str, category: &str, current: &str) -> Value {
        json!({
            "id": id,
            "name": id,
            "category": category,
            "type": "select",
            "currentValue": current,
            "options": [],
        })
    }

    async fn stub_agent(stream: DuplexStream, calls: std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let request: Value = serde_json::from_str(&line).unwrap();
            calls.lock().unwrap().push(request.clone());
            let id = request["id"].clone();
            let result = match request["method"].as_str().unwrap() {
                "initialize" => json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {
                        "loadSession": true,
                        "sessionCapabilities": {"resume": {}},
                    },
                }),
                "session/new" => json!({
                    "sessionId": "stub-session",
                    "configOptions": [
                        option("model-id", "model", "old-model"),
                        option("effort-id", "thought_level", "low"),
                    ],
                }),
                "session/resume" => json!({
                    "configOptions": [
                        option("model-id", "model", "old-model"),
                        option("effort-id", "thought_level", "low"),
                    ],
                }),
                "session/set_config_option" => json!({
                    "configOptions": [
                        option("model-id", "model", "test-model"),
                        option("effort-id", "thought_level", "high"),
                    ],
                }),
                "session/prompt" => {
                    for update in [
                        json!({"sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "Read", "rawInput": {"path": "README.md"}}),
                        json!({"sessionUpdate": "tool_call_update", "toolCallId": "call-1", "title": "Read", "status": "completed"}),
                        json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}}),
                        json!({"sessionUpdate": "compaction_update", "status": "completed"}),
                    ] {
                        let notification = json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {"sessionId": "stub-session", "update": update},
                        });
                        writer
                            .write_all(notification.to_string().as_bytes())
                            .await
                            .unwrap();
                        writer.write_all(b"\n").await.unwrap();
                    }
                    json!({"stopReason": "end_turn"})
                }
                method => panic!("unexpected ACP method {method}"),
            };
            let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
            writer
                .write_all(response.to_string().as_bytes())
                .await
                .unwrap();
            writer.write_all(b"\n").await.unwrap();
        }
    }

    fn event_sink(dir: &Path) -> (EventSink, std::path::PathBuf) {
        let events = dir.join("events.jsonl");
        let script = dir.join("event-sink");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$5\" >> \"$EVENT_LOG\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (
            EventSink::new(
                Hook {
                    command: script.display().to_string(),
                    args: vec!["agent-event".into(), "--kind".into(), "acp".into()],
                },
                vec![("EVENT_LOG".into(), events.display().to_string())],
            ),
            events,
        )
    }

    fn config(sink: Hook, resume_session_id: Option<&str>) -> LaunchConfig {
        LaunchConfig {
            version: acp::VERSION,
            system_prompt: "system rules".into(),
            initial_prompt: Some("do the task".into()),
            model: "test-model".into(),
            effort: Some("high".into()),
            resume_session_id: resume_session_id.map(str::to_string),
            mcp_servers: vec![ariadne_core::acp::McpServer {
                name: "ariadne".into(),
                command: "ariadne".into(),
                args: vec!["mcp".into(), "serve".into()],
                env: vec![],
            }],
            event_sink: sink,
        }
    }

    #[tokio::test]
    async fn the_acp_contract_runs_against_a_stub() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, events_path) = event_sink(dir.path());
        let hook = sink.hook.clone();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (client, agent) = duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let stub = tokio::spawn(stub_agent(agent, calls.clone()));

        run_protocol(
            BufReader::new(client_read),
            client_write,
            dir.path(),
            config(hook, None),
            sink,
            false,
        )
        .await
        .unwrap();
        stub.abort();

        let calls = calls.lock().unwrap();
        let methods: Vec<_> = calls
            .iter()
            .map(|call| call["method"].as_str().unwrap())
            .collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "session/new",
                "session/set_config_option",
                "session/set_config_option",
                "session/prompt",
            ]
        );
        assert_eq!(calls[1]["params"]["mcpServers"][0]["command"], "ariadne");
        assert_eq!(calls[2]["params"]["value"], "test-model");
        assert_eq!(calls[3]["params"]["value"], "high");
        assert_eq!(
            calls[4]["params"]["prompt"][0]["text"],
            "system rules\n\ndo the task"
        );

        let events = std::fs::read_to_string(events_path).unwrap();
        let event_kinds: Vec<_> = events
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap()["kind"].clone())
            .collect();
        for kind in [
            "session_start",
            "user_prompt_submit",
            "pre_tool_use",
            "post_tool_use",
            "compaction_update",
            "stop",
            "session_end",
        ] {
            assert!(
                event_kinds.contains(&Value::String(kind.into())),
                "{events}"
            );
        }
    }

    #[tokio::test]
    async fn resume_uses_the_saved_acp_session() {
        let dir = tempfile::tempdir().unwrap();
        let (sink, _) = event_sink(dir.path());
        let hook = sink.hook.clone();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let (client, agent) = duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let stub = tokio::spawn(stub_agent(agent, calls.clone()));

        run_protocol(
            BufReader::new(client_read),
            client_write,
            dir.path(),
            config(hook, Some("saved-session")),
            sink,
            false,
        )
        .await
        .unwrap();
        stub.abort();

        let calls = calls.lock().unwrap();
        assert_eq!(calls[1]["method"], "session/resume");
        assert_eq!(calls[1]["params"]["sessionId"], "saved-session");
    }
}
