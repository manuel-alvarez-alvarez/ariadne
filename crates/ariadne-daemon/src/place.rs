//! Session locations and multiplexer discovery.
use anyhow::{Context, Result, bail, ensure};
use ariadne_core::probe;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A resolved session location.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Place {
    None,
    Tmux(PathBuf),
}
impl Place {
    /// Stable session storage name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Tmux(_) => "tmux",
        }
    }

    fn find_herdr(_path: Option<&OsStr>) -> Option<Self> {
        None
    }

    /// Select a location on the daemon's search path.
    pub fn find(forced: Option<&str>, path: Option<&OsStr>) -> Result<Self> {
        // Herdr discovery deliberately reports unavailable until its backend lands.
        let tmux = || {
            path.and_then(|path| probe::which(path, "tmux"))
                .map(Self::Tmux)
        };
        match forced {
            Some("none") => Ok(Self::None),
            Some("herdr") => Self::find_herdr(path)
                .ok_or_else(|| anyhow::anyhow!("forced session place herdr was not found")),
            Some("tmux") => {
                tmux().ok_or_else(|| anyhow::anyhow!("forced session place tmux was not found"))
            }
            None => Ok(Self::find_herdr(path).or_else(tmux).unwrap_or(Self::None)),
            Some(value) => bail!("invalid session place {value:?}; expected none, tmux, or herdr"),
        }
    }
}

/// Report discovery, including unavailable forced locations.
pub(crate) fn report(forced: Option<&str>, path: Option<&OsStr>) -> ariadne_api::doctor::PlaceDto {
    use ariadne_api::doctor::PlaceDto;
    match Place::find(forced, path) {
        Ok(place) => PlaceDto {
            name: place.name().into(),
            reason: if forced.is_some() {
                "forced by session_place".into()
            } else {
                match place {
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
        match tokio::net::UnixStream::connect(&socket_path).await {
            Ok(mut socket) => {
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
            bail!("a direct child has no relay pane");
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
