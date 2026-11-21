//! Session locations and multiplexer discovery.
use anyhow::{Context, Result, bail, ensure};
use ariadne_core::probe;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

/// A resolved session location.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Place {
    None,
    Tmux(PathBuf),
    /// A herdr server, reached through its socket.
    Herdr(PathBuf),
}
impl Place {
    /// Stable session storage name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tmux(_) => "tmux",
            Self::Herdr(_) => "herdr",
        }
    }

    /// Herdr is found when its executable is on the search path and its
    /// server answers a ping on the socket. A server that stays silent is
    /// the same as one that is not there.
    fn find_herdr(path: Option<&OsStr>, socket: Option<&Path>) -> Option<Self> {
        probe::which(path?, "herdr")?;
        let socket = socket.map(Path::to_path_buf).or_else(herdr_socket)?;
        herdr_answers(&socket).then_some(Self::Herdr(socket))
    }

    /// Select a location on the daemon's search path.
    pub fn find(forced: Option<&str>, path: Option<&OsStr>) -> Result<Self> {
        Self::find_with(forced, path, None)
    }

    /// Select a location; `herdr_socket` replaces the socket herdr's own
    /// rules would resolve from the environment.
    pub fn find_with(
        forced: Option<&str>,
        path: Option<&OsStr>,
        herdr_socket: Option<&Path>,
    ) -> Result<Self> {
        let tmux = || {
            path.and_then(|path| probe::which(path, "tmux"))
                .map(Self::Tmux)
        };
        match forced {
            Some("none") => Ok(Self::None),
            Some("herdr") => Self::find_herdr(path, herdr_socket)
                .ok_or_else(|| anyhow::anyhow!("forced session place herdr was not found")),
            Some("tmux") => {
                tmux().ok_or_else(|| anyhow::anyhow!("forced session place tmux was not found"))
            }
            None => Ok(Self::find_herdr(path, herdr_socket)
                .or_else(tmux)
                .unwrap_or(Self::None)),
            Some(value) => bail!("invalid session place {value:?}; expected none, tmux, or herdr"),
        }
    }
}

/// Report discovery, including unavailable forced locations.
pub fn report(
    forced: Option<&str>,
    path: Option<&OsStr>,
    herdr_socket: Option<&Path>,
) -> ariadne_api::doctor::PlaceDto {
    use ariadne_api::doctor::PlaceDto;
    match Place::find_with(forced, path, herdr_socket) {
        Ok(place) => PlaceDto {
            name: place.name().into(),
            reason: if forced.is_some() {
                "forced by session_place".into()
            } else {
                match place {
                    Place::Herdr(_) => "herdr is installed and its server answers".into(),
                    Place::Tmux(_) => "herdr was not found; tmux is available".into(),
                    Place::None => "herdr and tmux were not found; use a direct child".into(),
                }
            },
            available: true,
        },
        Err(error) => PlaceDto {
            name: forced.unwrap_or("none").into(),
            reason: error.to_string(),
            available: false,
        },
    }
}

/// One relay command, passed as arguments without shell interpretation.
pub struct PaneLaunch<'a> {
    pub home: &'a std::path::Path,
    pub cwd: &'a std::path::Path,
    pub session_id: &'a str,
    pub launch_id: &'a str,
    pub agent_id: &'a str,
    pub task_id: &'a str,
    pub daemon: &'a std::path::Path,
    pub program: &'a str,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
}

/// A live pane in a location's private server.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pane {
    pub id: String,
    pub name: String,
    pub session_id: String,
}

impl Place {
    /// Start a detached relay pane.
    pub async fn start_pane(&self, launch: &PaneLaunch<'_>) -> Result<Pane> {
        for id in [launch.session_id, launch.launch_id] {
            ensure!(
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
                "invalid pane identity"
            );
        }
        let name = format!("ariadne-{}", launch.launch_id);
        if let Self::Herdr(socket) = self {
            return herdr_start(socket, launch, name).await;
        }
        let mut command = self.command(launch.home)?;
        command
            .args(["new-session", "-d", "-s", &name, "-c"])
            .arg(tmux_literal(launch.cwd));
        for (key, value) in std::env::vars().chain(launch.env.iter().cloned()) {
            command
                .arg("-e")
                .arg(tmux_literal(format!("{key}={value}")));
        }
        command
            .args(["-P", "-F", "#{pane_id}"])
            .arg(tmux_literal(launch.daemon))
            .arg("relay")
            .arg("--home")
            .arg(tmux_literal(launch.home))
            .args(
                [
                    "--session",
                    launch.session_id,
                    "--agent",
                    launch.agent_id,
                    "--task",
                    launch.task_id,
                    "--",
                    launch.program,
                ]
                .map(tmux_literal),
            )
            .args(launch.args.iter().map(tmux_literal))
            .args([
                ";",
                "set-option",
                "-t",
                &name,
                "@ariadne_session",
                launch.session_id,
            ]);
        let output = run(command).await?;
        ensure!(
            output.status.success(),
            "starting tmux relay pane: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(Pane {
            id: String::from_utf8(output.stdout)?.trim().into(),
            name,
            session_id: launch.session_id.into(),
        })
    }

    /// List the live relay panes in this home's private server.
    ///
    /// Every tmux call names its target. `list-panes -a` fails with "no current
    /// target" on a server that has no session, so the sessions are listed
    /// first and each relay session is asked for its own panes.
    pub async fn live_panes(&self, home: &std::path::Path) -> Result<Vec<Pane>> {
        if let Self::Herdr(socket) = self {
            return herdr_panes(socket, home).await;
        }
        let (names, exited) = self.relay_sessions(home).await?;
        let Some(names) = names else {
            if exited {
                tracing::warn!("the tmux server exited while its relay sessions were listed");
            }
            return Ok(Vec::new());
        };
        let mut panes = Vec::new();
        for name in names {
            let mut command = self.command(home)?;
            // Non-UTF-8 tmux clients replace control separators with underscores.
            // Pane and session identifiers cannot contain this printable separator.
            command.args(["list-panes", "-s", "-t"]);
            command.arg(format!("={name}:"));
            command.args(["-F", "#{pane_id} #{session_name} #{@ariadne_session}"]);
            let output = run(command).await?;
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                // The session ended, or the server with it, since the listing.
                match self.relay_sessions(home).await?.0 {
                    Some(names) if names.contains(&name) => {}
                    Some(_) => continue,
                    None => {
                        tracing::warn!("the tmux server exited while its relay panes were listed");
                        return Ok(Vec::new());
                    }
                }
                bail!("listing tmux relay panes: {}", error.trim());
            }
            for line in String::from_utf8(output.stdout)?.lines() {
                let fields: Vec<_> = line.split(' ').collect();
                if fields.len() == 3 && fields[1] == name && !fields[2].is_empty() {
                    panes.push(Pane {
                        id: fields[0].into(),
                        name: fields[1].into(),
                        session_id: fields[2].into(),
                    });
                }
            }
        }
        Ok(panes)
    }

    /// The relay session names, or `None` when no tmux server runs, and whether
    /// the server exited during the call.
    ///
    /// A server that closes with its last session can answer a client that is
    /// already connected with "server exited unexpectedly". Ask again: once it
    /// has closed, no server runs. A server that keeps failing is an error.
    async fn relay_sessions(&self, home: &std::path::Path) -> Result<(Option<Vec<String>>, bool)> {
        let mut attempts = 0;
        loop {
            let mut command = self.command(home)?;
            command.args(["list-sessions", "-F", "#{session_name}"]);
            let output = run(command).await?;
            if output.status.success() {
                let names = String::from_utf8(output.stdout)?
                    .lines()
                    .filter(|name| name.starts_with("ariadne-"))
                    .map(String::from)
                    .collect();
                return Ok((Some(names), false));
            }
            let error = String::from_utf8_lossy(&output.stderr);
            if error.contains("no server running") || error.contains("No such file") {
                return Ok((None, attempts > 0));
            }
            attempts += 1;
            if !error.contains("server exited unexpectedly") || attempts == 5 {
                bail!("listing tmux relay sessions: {}", error.trim());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Stop the relay first so its agent process group cannot outlive the pane.
    pub async fn stop_pane(&self, home: &std::path::Path, pane: &Pane) -> Result<()> {
        let socket_path = home
            .join("relays")
            .join(format!("{}.sock", pane.session_id));
        let mut told = false;
        match tokio::net::UnixStream::connect(&socket_path).await {
            Ok(mut socket) => {
                told = true;
                tokio::time::timeout(crate::timeouts::Timeouts::default().probe, async {
                    socket.write_all(b"{\"type\":\"stop\"}\n").await?;
                    let mut discarded = Vec::new();
                    socket.read_to_end(&mut discarded).await?;
                    std::io::Result::Ok(())
                })
                .await
                .context("waiting for relay stop")??;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(error) => return Err(error).context("connecting to relay for stop"),
        }
        if let Self::Herdr(socket) = self {
            if told {
                herdr_relay_exits(&socket_path, socket, home, pane).await;
            }
            return herdr_close(socket, home, pane).await;
        }
        if self
            .live_panes(home)
            .await?
            .iter()
            .any(|live| live.id == pane.id)
        {
            let mut command = self.command(home)?;
            command.args(["kill-session", "-t", &pane.name]);
            let output = run(command).await?;
            if !output.status.success()
                && self
                    .live_panes(home)
                    .await?
                    .iter()
                    .any(|live| live.id == pane.id)
            {
                bail!(
                    "stopping tmux relay pane: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }
        Ok(())
    }

    fn command(&self, home: &std::path::Path) -> Result<tokio::process::Command> {
        let Self::Tmux(binary) = self else {
            bail!("only tmux runs a command for a relay pane");
        };
        let mut command = tokio::process::Command::new(binary);
        command
            .arg("-S")
            .arg(tmux_literal(home.join("tmux.sock")))
            .kill_on_drop(true);
        Ok(command)
    }
}

// tmux parses a trailing semicolon as a command separator, even in an argv element.
fn tmux_literal(value: impl AsRef<OsStr>) -> OsString {
    let bytes = value.as_ref().as_bytes();
    if let Some(prefix) = bytes.strip_suffix(b";") {
        let mut escaped = prefix.to_vec();
        escaped.extend_from_slice(b"\\;");
        OsString::from_vec(escaped)
    } else {
        value.as_ref().to_owned()
    }
}

async fn run(mut command: tokio::process::Command) -> Result<std::process::Output> {
    tokio::time::timeout(crate::timeouts::Timeouts::default().probe, command.output())
        .await
        .context("tmux command timed out")?
        .context("running tmux")
}

/// Where herdr listens, by its own rules: `HERDR_SOCKET_PATH`, then the
/// named session in `HERDR_SESSION`, then the default session.
fn herdr_socket() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_SOCKET_PATH").filter(|path| !path.is_empty()) {
        return Some(path.into());
    }
    let config = PathBuf::from(std::env::var_os("HOME")?).join(".config/herdr");
    Some(match std::env::var("HERDR_SESSION") {
        Ok(name) if !name.is_empty() => config.join("sessions").join(name).join("herdr.sock"),
        _ => config.join("herdr.sock"),
    })
}

/// Whether a herdr server answers a ping on `socket` within the probe time.
fn herdr_answers(socket: &Path) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let probe = crate::timeouts::Timeouts::default().probe;
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return false;
    };
    if stream.set_read_timeout(Some(probe)).is_err()
        || stream.set_write_timeout(Some(probe)).is_err()
        || stream
            .write_all(b"{\"id\":\"ping\",\"method\":\"ping\",\"params\":{}}\n")
            .is_err()
    {
        return false;
    }
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).is_ok()
        && serde_json::from_str::<Value>(&line).is_ok_and(|reply| reply["result"]["type"] == "pong")
}

/// Send one request on a fresh connection and return its result.
async fn herdr_call(socket: &Path, method: &str, params: Value) -> Result<Value> {
    tokio::time::timeout(crate::timeouts::Timeouts::default().probe, async {
        let mut stream = tokio::io::BufReader::new(
            tokio::net::UnixStream::connect(socket)
                .await
                .context("connecting to herdr")?,
        );
        let request = json!({"id": "ariadne", "method": method, "params": params});
        stream
            .write_all(format!("{request}\n").as_bytes())
            .await
            .context("writing to herdr")?;
        let mut line = String::new();
        ensure!(
            stream
                .read_line(&mut line)
                .await
                .context("reading from herdr")?
                > 0,
            "herdr closed the connection without an answer"
        );
        let mut reply: Value = serde_json::from_str(&line).context("reading the herdr answer")?;
        if let Some(error) = reply.get("error") {
            bail!(
                "herdr {method}: {}: {}",
                error["code"].as_str().unwrap_or("error"),
                error["message"].as_str().unwrap_or("no message")
            );
        }
        Ok(reply["result"].take())
    })
    .await
    .with_context(|| format!("herdr {method} timed out"))?
}

/// A short stable tag for a home, so one herdr server can hold the panes of
/// several daemon homes and each lists only its own.
fn home_tag(home: &Path) -> String {
    let hash = home
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("{hash:016x}")
}

async fn herdr_start(socket: &Path, launch: &PaneLaunch<'_>, name: String) -> Result<Pane> {
    let label = format!("{name}/{}@{}", launch.session_id, home_tag(launch.home));
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_owned)
            .context("herdr needs UTF-8 paths")
    };
    let mut command = vec![
        text(launch.daemon)?,
        "relay".into(),
        "--home".into(),
        text(launch.home)?,
        "--session".into(),
        launch.session_id.into(),
        "--agent".into(),
        launch.agent_id.into(),
        "--task".into(),
        launch.task_id.into(),
        "--".into(),
        launch.program.into(),
    ];
    command.extend(launch.args.iter().cloned());
    let env: serde_json::Map<String, Value> = std::env::vars()
        .chain(launch.env.iter().cloned())
        .map(|(key, value)| (key, Value::String(value)))
        .collect();
    let cwd = text(launch.cwd)?;
    let created = herdr_call(
        socket,
        "workspace.create",
        json!({"label": label, "cwd": cwd, "focus": false}),
    )
    .await
    .context("creating herdr workspace")?;
    let workspace = created["workspace"]["workspace_id"]
        .as_str()
        .context("herdr named no workspace")?
        .to_owned();
    // The new workspace holds a shell tab. Replace it, so the relay's pane is
    // the only one and closing it leaves nothing behind. Herdr refuses a
    // request that names both the tab and its workspace.
    let applied = herdr_call(
        socket,
        "layout.apply",
        json!({
            "tab_id": created["tab"]["tab_id"],
            "tab_label": label,
            "focus": false,
            "root": {"type": "pane", "label": label, "cwd": cwd, "command": command, "env": env},
        }),
    )
    .await;
    let id = match applied {
        Ok(applied) => applied["layout"]["root"]["pane_id"]
            .as_str()
            .map(str::to_owned),
        Err(error) => {
            let _ = herdr_call(
                socket,
                "workspace.close",
                json!({"workspace_id": workspace}),
            )
            .await;
            return Err(error.context("starting herdr relay pane"));
        }
    };
    let id = match id {
        Some(id) => id,
        None => herdr_panes(socket, launch.home)
            .await?
            .into_iter()
            .find(|pane| pane.name == name)
            .map(|pane| pane.id)
            .context("herdr named no relay pane")?,
    };
    Ok(Pane {
        id,
        name,
        session_id: launch.session_id.into(),
    })
}

async fn herdr_panes(socket: &Path, home: &Path) -> Result<Vec<Pane>> {
    let tag = format!("@{}", home_tag(home));
    let listed = match herdr_call(socket, "pane.list", json!({})).await {
        Ok(listed) => listed,
        Err(error)
            if error.chain().any(|cause| {
                cause.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    )
                })
            }) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.context("listing herdr relay panes")),
    };
    Ok(listed["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|pane| {
            let (name, rest) = pane["label"].as_str()?.split_once('/')?;
            let session = rest.strip_suffix(&tag)?;
            (name.starts_with("ariadne-") && !session.is_empty()).then(|| Pane {
                id: pane["pane_id"].as_str().unwrap_or_default().into(),
                name: name.into(),
                session_id: session.into(),
            })
        })
        .collect())
}

/// Closing a pane hangs up its terminal, and a relay that is hung up before
/// it finishes stopping leaves its socket behind. Give the relay the time
/// to remove it, until the pane ends or the probe time runs out.
async fn herdr_relay_exits(relay_socket: &Path, socket: &Path, home: &Path, pane: &Pane) {
    let _ = tokio::time::timeout(crate::timeouts::Timeouts::default().probe, async {
        while relay_socket.exists()
            && herdr_panes(socket, home)
                .await
                .is_ok_and(|panes| panes.iter().any(|live| live.id == pane.id))
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
}

async fn herdr_close(socket: &Path, home: &Path, pane: &Pane) -> Result<()> {
    let live = |panes: Vec<Pane>| panes.iter().any(|live| live.id == pane.id);
    if live(herdr_panes(socket, home).await?) {
        let closed = herdr_call(socket, "pane.close", json!({"pane_id": pane.id})).await;
        if let Err(error) = closed
            && live(herdr_panes(socket, home).await?)
        {
            return Err(error.context("stopping herdr relay pane"));
        }
    }
    Ok(())
}
