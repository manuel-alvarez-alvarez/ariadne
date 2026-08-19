//! `ariadne doctor` — one report on the whole installation.
//!
//! Every check is a line with a verdict (`ok`, `warn`, `fail`) and, when
//! something is wrong, the one thing to do about it. Nothing here changes
//! anything: doctor diagnoses, and says what would fix it.
//!
//! The report has two halves on purpose. What this shell sees is one thing;
//! what `ariadned` sees is another, and it is the one that matters, because
//! the daemon is what spawns sessions. A daemon started by launchd or systemd
//! carries the PATH its service file was written with, so an agent CLI
//! installed after the install can be perfectly present here and invisible
//! there — which looks, from the outside, like a profile that will not run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

use ariadne_api::agents::AgentConfigDto;
use ariadne_api::doctor::{BinaryDto, DaemonReportDto};
use ariadne_api::profiles::ProfileDto;
use ariadne_client::endpoint::{self, ConfigError, FileConfig};
use ariadne_client::{Client, ClientError};
use ariadne_core::AgentKind;

use crate::output::{Format, note, print_json};

/// How long any probe — a `--version`, a `launchctl` call — may take. A hung
/// or half-installed binary must not hang the report.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// launchd label and systemd unit `scripts/install.sh` registers.
const LAUNCHD_LABEL: &str = "dev.ariadne.daemon";
const SYSTEMD_UNIT: &str = "ariadned.service";

/// How a check came out. Ordered by severity so the worst one wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// One line of the report: what was checked, how it came out, and — when it
/// did not come out well — what to do next.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Check {
    fn new(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            hint: None,
        }
    }

    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Ok, detail)
    }

    fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Warn, detail)
    }

    fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Fail, detail)
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A group of checks, rendered under one heading.
#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub name: String,
    pub checks: Vec<Check>,
}

impl Section {
    fn new(name: impl Into<String>, checks: Vec<Check>) -> Self {
        Self {
            name: name.into(),
            checks,
        }
    }
}

/// The whole report, and the verdict the exit code comes from.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The worst status of any check.
    pub status: Status,
    /// The daemon this was measured against.
    pub endpoint: String,
    pub sections: Vec<Section>,
}

impl Report {
    fn new(endpoint: impl Into<String>, sections: Vec<Section>) -> Self {
        let status = sections
            .iter()
            .flat_map(|s| s.checks.iter())
            .map(|c| c.status)
            .max()
            .unwrap_or(Status::Ok);
        Self {
            status,
            endpoint: endpoint.into(),
            sections,
        }
    }

    /// Checks at a given status, for the closing verdict.
    fn count(&self, status: Status) -> usize {
        self.sections
            .iter()
            .flat_map(|s| s.checks.iter())
            .filter(|c| c.status == status)
            .count()
    }

    /// 1 as soon as anything failed, 0 otherwise: warnings are things to look
    /// at, failures are things that stop Ariadne working.
    fn exit_code(&self) -> ExitCode {
        match self.status {
            Status::Fail => ExitCode::from(1),
            _ => ExitCode::SUCCESS,
        }
    }
}

/// `ariadne doctor` — build the report, print it, and answer with it.
pub async fn run(client: &Client, format: Format) -> Result<ExitCode> {
    let report = examine(client).await;
    match format {
        Format::Json => print_json(&report)?,
        Format::Table => {
            for line in render(&report) {
                println!("{line}");
            }
            note(&verdict(&report));
        }
    }
    Ok(report.exit_code())
}

// ---- gathering ---------------------------------------------------------

/// A binary as this shell can see it.
#[derive(Debug, Clone)]
struct Local {
    name: String,
    path: Option<PathBuf>,
    version: Option<String>,
}

impl Local {
    /// "claude 1.2.3 at /usr/local/bin/claude", as far as it is known.
    fn describe(&self) -> String {
        match (&self.path, &self.version) {
            (Some(path), Some(version)) => format!("{version} at {}", path.display()),
            (Some(path), None) => format!("{} (no version answer)", path.display()),
            (None, _) => format!("{} not found on PATH", self.name),
        }
    }
}

/// Ask the whole installation how it is doing.
///
/// One pass, whether or not there is a daemon to ask: a stopped daemon is one
/// failing check inside a full report, not a command that gives up. What
/// needed the daemon is marked as unmeasured, and everything else — the
/// binaries, the home, the service registration, which is exactly what one
/// wants to see when the daemon is down — is measured as usual.
async fn examine(client: &Client) -> Report {
    let home = endpoint::home(None);
    let config = home.as_deref().map(endpoint::parse_config);

    // Probes are processes: run them at once rather than three seconds apart.
    let (claude, codex, opencode, tmux, git, ariadned) = tokio::join!(
        probe(AgentKind::ClaudeCode.binary(), "--version"),
        probe(AgentKind::Codex.binary(), "--version"),
        probe(AgentKind::Opencode.binary(), "--version"),
        probe("tmux", "-V"),
        probe("git", "--version"),
        probe_ariadned(),
    );
    let agents = vec![claude, codex, opencode];

    let health = client.health().await;
    let reachable = health.is_ok();
    // Everything here only exists for a daemon that answered at all.
    let (version, daemon, profiles, flags) = match reachable {
        true => {
            let (version, daemon, profiles, flags) = tokio::join!(
                client.version(),
                client.daemon_report(),
                client.get_json::<Vec<ProfileDto>>("/v1/profiles"),
                client.list_agent_configs(),
            );
            (
                version.ok().map(|v| v.version),
                daemon.ok(),
                profiles.unwrap_or_default(),
                flags.unwrap_or_default(),
            )
        }
        false => (None, None, Vec::new(), Vec::new()),
    };
    let available = Availability::new(daemon.as_ref(), &agents);

    Report::new(
        client.endpoint(),
        vec![
            Section::new("client", client_checks(&ariadned, reachable)),
            Section::new("home", home_checks(home.as_deref(), config)),
            Section::new(
                "daemon",
                daemon_checks(client, &health, version, home.as_deref()).await,
            ),
            Section::new("tools", tool_checks(&[tmux, git])),
            Section::new("agents", agent_checks(&agents, &flags, &available)),
            Section::new(
                "profiles",
                match reachable {
                    true => profile_checks(&profiles, &available),
                    // Nothing was listed, which is not the same as no profiles.
                    false => vec![Check::warn(
                        "profiles",
                        "not checked — the daemon did not answer",
                    )],
                },
            ),
            Section::new(
                "daemon environment",
                daemon_env_checks(daemon.as_ref(), &available),
            ),
        ],
    )
}

fn unreachable_check(health: &Result<ariadne_api::HealthResponse, ClientError>) -> Check {
    let error = match health {
        Err(e) => e,
        // Only ever called with a failure; a success has nothing to report.
        Ok(_) => return Check::ok("reachable", "yes"),
    };
    let check = Check::fail("reachable", error.human());
    match error.hint() {
        Some(hint) => check.hint(hint),
        None => check,
    }
}

/// Find a binary on PATH and ask it for its version, fail-soft: a missing
/// binary, one that refuses to run, or one that never answers all leave the
/// report standing.
async fn probe(name: &str, version_flag: &str) -> Local {
    let path = super::which(name);
    let version = match &path {
        Some(path) => probe_version(path, version_flag).await,
        None => None,
    };
    Local {
        name: name.to_string(),
        path,
        version,
    }
}

/// `ariadned` as `daemon start` would find it: next to this binary, else on
/// PATH.
async fn probe_ariadned() -> Local {
    let path = super::find_ariadned().ok();
    let version = match &path {
        Some(path) => probe_version(path, "--version").await,
        None => None,
    };
    Local {
        name: "ariadned".to_string(),
        path,
        version,
    }
}

/// The first line of `<binary> <flag>`, bounded by [`PROBE_TIMEOUT`].
///
/// `kill_on_drop` matters as much as the timeout: without it a binary that
/// hangs outlives the command that gave up on it.
async fn probe_version(binary: &Path, flag: &str) -> Option<String> {
    let output = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(binary)
            .arg(flag)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    // Most CLIs answer on stdout; the ones that answer on stderr are still
    // answering.
    let text = match output.stdout.is_empty() {
        true => String::from_utf8_lossy(&output.stderr),
        false => String::from_utf8_lossy(&output.stdout),
    };
    let line = text.lines().next().unwrap_or_default().trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Run a command only for its exit status, bounded by [`PROBE_TIMEOUT`].
async fn probe_status(program: &str, args: &[&str]) -> bool {
    tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .is_ok_and(|out| out.is_ok_and(|out| out.status.success()))
}

// ---- sections ----------------------------------------------------------

fn client_checks(ariadned: &Local, daemon_reachable: bool) -> Vec<Check> {
    let mut checks = vec![Check::ok(
        "ariadne",
        format!("ariadne {}", env!("CARGO_PKG_VERSION")),
    )];
    checks.push(match &ariadned.path {
        Some(_) => Check::ok("ariadned", ariadned.describe()),
        // A daemon that is answering is evidently installed somewhere; not
        // finding its binary from here only costs `ariadne daemon start`.
        None if daemon_reachable => Check::warn("ariadned", "not found next to ariadne or on PATH")
            .hint("the running daemon was started from elsewhere; `ariadne daemon start` needs it here"),
        None => Check::fail("ariadned", "not found next to ariadne or on PATH")
            .hint("install it beside ariadne: scripts/install.sh"),
    });
    checks
}

fn home_checks(
    home: Option<&Path>,
    config: Option<Result<Option<FileConfig>, ConfigError>>,
) -> Vec<Check> {
    let Some(home) = home else {
        return vec![
            Check::fail("home", "no ariadne home could be resolved")
                .hint("set ARIADNE_HOME, or run from an account with a home directory"),
        ];
    };
    let mut checks = vec![match home.is_dir() {
        true => Check::ok("home", home.display().to_string()),
        false => Check::warn("home", format!("{} does not exist yet", home.display()))
            .hint("the daemon creates it on its first start"),
    }];

    // Only `None` for a home that could not be resolved, handled above.
    let config = config.unwrap_or(Ok(None));
    let config_path = endpoint::config_file(home);
    checks.push(match &config {
        Ok(None) => Check::ok("config.toml", "none — built-in defaults"),
        Ok(Some(_)) => Check::ok("config.toml", config_path.display().to_string()),
        Err(e) => Check::fail("config.toml", e.to_string())
            .hint("the daemon refuses to start on a config it cannot read"),
    });

    let socket = endpoint::socket_path(home);
    checks.push(match socket.exists() {
        true => Check::ok("socket", socket.display().to_string()),
        false => Check::warn("socket", format!("{} does not exist", socket.display()))
            .hint("no daemon has listened on this home yet"),
    });

    // Wherever the config puts it: a report on the default path would be
    // about a file the daemon never opens.
    let db = config
        .ok()
        .flatten()
        .and_then(|c| c.db_path)
        .unwrap_or_else(|| home.join("ariadne.db"));
    checks.push(match db.is_file() {
        true => Check::ok("database", db.display().to_string()),
        false => Check::warn("database", format!("{} does not exist yet", db.display()))
            .hint("the daemon creates it on its first start"),
    });

    let pid_file = endpoint::pid_file(home);
    checks.push(match std::fs::read_to_string(&pid_file) {
        Ok(pid) => Check::ok(
            "pidfile",
            format!("{} (pid {})", pid_file.display(), pid.trim()),
        ),
        Err(_) => Check::warn("pidfile", format!("{} does not exist", pid_file.display()))
            .hint("written by a daemon started outside a service manager"),
    });
    checks
}

async fn daemon_checks(
    client: &Client,
    health: &Result<ariadne_api::HealthResponse, ClientError>,
    daemon_version: Option<String>,
    home: Option<&Path>,
) -> Vec<Check> {
    let mut checks = vec![match health {
        Ok(h) => Check::ok(
            "reachable",
            format!("{} (up {}s)", client.endpoint(), h.uptime_secs),
        ),
        Err(_) => unreachable_check(health),
    }];

    let client_version = env!("CARGO_PKG_VERSION");
    match (&daemon_version, health.is_ok()) {
        (Some(v), _) if v == client_version => {
            checks.push(Check::ok("version", format!("ariadned {v}")))
        }
        // An upgrade installed under a daemon that is still running the old
        // binary: everything works and nothing is what you installed.
        (Some(v), _) => checks.push(
            Check::warn("version", format!("daemon {v}, client {client_version}"))
                .hint("restart the daemon so it runs the version you installed"),
        ),
        (None, true) => checks.push(Check::warn(
            "version",
            "the daemon did not answer /v1/version",
        )),
        // Nothing answered at all; the check above already said so.
        (None, false) => {}
    }

    checks.extend(service_checks(home).await);
    checks
}

/// Whether the daemon is registered with the OS service manager, read-only:
/// doctor reports what it finds and never registers, loads or repairs.
async fn service_checks(home: Option<&Path>) -> Vec<Check> {
    let manifest = home.map(read_manifest).unwrap_or_default();
    let mut checks = vec![match home.map(|h| h.join("install.env")) {
        Some(path) if path.is_file() => Check::ok("install manifest", path.display().to_string()),
        _ => Check::warn("install manifest", "no install.env")
            .hint("installed by hand? scripts/install.sh writes one"),
    }];

    let check = if cfg!(target_os = "macos") {
        let plist = manifest
            .get("ARIADNE_PLIST")
            .map(PathBuf::from)
            .unwrap_or_else(default_plist);
        if !plist.is_file() {
            Check::warn(
                "service",
                format!("no launchd plist at {}", plist.display()),
            )
            .hint("the daemon will not come back after a reboot; scripts/install.sh registers it")
        } else if probe_status("launchctl", &["list", LAUNCHD_LABEL]).await {
            Check::ok("service", format!("launchd {LAUNCHD_LABEL} loaded"))
        } else {
            Check::warn(
                "service",
                format!("launchd {LAUNCHD_LABEL} installed but not loaded"),
            )
            .hint(
                "load it with: launchctl bootstrap gui/$(id -u) ".to_string()
                    + &plist.display().to_string(),
            )
        }
    } else if cfg!(target_os = "linux") {
        let unit = manifest
            .get("ARIADNE_UNIT")
            .map(PathBuf::from)
            .unwrap_or_else(default_unit);
        if !unit.is_file() {
            Check::warn("service", format!("no systemd unit at {}", unit.display())).hint(
                "the daemon will not come back after a reboot; scripts/install.sh registers it",
            )
        } else if probe_status("systemctl", &["--user", "is-active", SYSTEMD_UNIT]).await {
            Check::ok("service", format!("systemd --user {SYSTEMD_UNIT} active"))
        } else {
            Check::warn(
                "service",
                format!("systemd --user {SYSTEMD_UNIT} installed but not active"),
            )
            .hint(format!(
                "start it with: systemctl --user start {SYSTEMD_UNIT}"
            ))
        }
    } else {
        Check::warn("service", "no service manager Ariadne knows on this OS")
            .hint("run ariadned yourself, or with whatever supervisor you use")
    };
    checks.push(check);
    checks
}

fn default_plist() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
}

fn default_unit() -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    config.join("systemd/user").join(SYSTEMD_UNIT)
}

/// `install.env` as `scripts/install.sh` writes it: `KEY="value"` lines and
/// comments, read as data — nothing is executed.
fn read_manifest(home: &Path) -> BTreeMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(home.join("install.env")) else {
        return BTreeMap::new();
    };
    raw.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

/// tmux and git: without either, no session can be spawned at all.
fn tool_checks(tools: &[Local]) -> Vec<Check> {
    tools
        .iter()
        .map(|tool| match tool.path {
            Some(_) => Check::ok(tool.name.clone(), tool.describe()),
            None => Check::fail(tool.name.clone(), tool.describe()).hint(format!(
                "install {} — Ariadne cannot run sessions without it",
                tool.name
            )),
        })
        .collect()
}

/// The coding agents as this shell sees them, with the flags each is launched
/// with when the daemon could be asked.
fn agent_checks(
    agents: &[Local],
    flags: &[AgentConfigDto],
    available: &Availability,
) -> Vec<Check> {
    let mut checks: Vec<Check> = AgentKind::ALL
        .iter()
        .zip(agents)
        .map(|(kind, local)| {
            let flags = flags
                .iter()
                .find(|c| c.agent_kind == *kind)
                .map(|c| match c.extra_flags.is_empty() {
                    true => "; no extra flags".to_string(),
                    false => format!("; flags: {}", c.extra_flags.join(" ")),
                })
                .unwrap_or_default();
            match local.path {
                Some(_) => Check::ok(kind.as_str(), format!("{}{flags}", local.describe())),
                // Only one agent is needed to run sessions, so a missing one
                // is only a failure for the profiles that name it — which the
                // profiles section reports, by name.
                None => Check::warn(kind.as_str(), local.describe())
                    .hint("not needed unless a profile runs on it"),
            }
        })
        .collect();

    if available.effective().is_empty() {
        let check = Check::fail(
            "any agent",
            format!("no coding agent CLI on {}", available.viewpoint()),
        );
        checks.push(match available.stale_service_path() {
            true => check.hint(
                "they are on your PATH but not the daemon's — re-run scripts/install.sh so the service picks them up",
            ),
            false => check
                .hint("install claude, codex or opencode — sessions cannot be spawned without one"),
        });
    }
    checks
}

/// Which agent binaries can actually be launched, from both points of view.
#[derive(Debug, Default, Clone)]
pub struct Availability {
    /// Kinds on the daemon's PATH; `None` when no daemon answered.
    daemon: Option<Vec<AgentKind>>,
    /// Kinds on this shell's PATH.
    client: Vec<AgentKind>,
}

impl Availability {
    fn new(daemon: Option<&DaemonReportDto>, agents: &[Local]) -> Self {
        Self {
            daemon: daemon.map(|d| {
                d.agents
                    .iter()
                    .filter(|a| a.path.is_some())
                    .filter_map(|a| a.agent_kind)
                    .collect()
            }),
            client: AgentKind::ALL
                .iter()
                .zip(agents)
                .filter(|(_, local)| local.path.is_some())
                .map(|(kind, _)| *kind)
                .collect(),
        }
    }

    /// What the process that spawns sessions can launch: the daemon's view
    /// when there is one, this shell's as the only stand-in when there is not.
    pub fn effective(&self) -> &[AgentKind] {
        self.daemon.as_deref().unwrap_or(&self.client)
    }

    pub fn has(&self, kind: AgentKind) -> bool {
        self.effective().contains(&kind)
    }

    /// Installed here but not where it counts — the shape a stale service
    /// PATH takes.
    fn only_on_client(&self, kind: AgentKind) -> bool {
        self.daemon.is_some() && !self.has(kind) && self.client.contains(&kind)
    }

    /// Nothing to launch where it counts, while this shell has agents: the
    /// same stale service PATH, seen across all three at once.
    fn stale_service_path(&self) -> bool {
        self.daemon.is_some() && self.effective().is_empty() && !self.client.is_empty()
    }

    /// Whose PATH a verdict was reached on, so a failure says where to look.
    fn viewpoint(&self) -> &'static str {
        match self.daemon.is_some() {
            true => "the daemon's PATH",
            false => "PATH",
        }
    }
}

/// Every profile's agent, checked against what can actually be launched.
///
/// A profile pinned to an agent kind cannot spawn anything without that
/// binary, so a missing one is a failure naming both the profile and the
/// binary. An `auto` profile resolves to whatever is installed at spawn time
/// and only fails when nothing at all is.
pub fn profile_checks(profiles: &[ProfileDto], available: &Availability) -> Vec<Check> {
    if profiles.is_empty() {
        return vec![Check::ok("profiles", "none defined")];
    }

    let named = |kind: AgentKind| -> Vec<&str> {
        profiles
            .iter()
            .filter(|p| p.agent_kind == Some(kind))
            .map(|p| p.name.as_str())
            .collect()
    };
    let auto: Vec<&str> = profiles
        .iter()
        .filter(|p| p.agent_kind.is_none())
        .map(|p| p.name.as_str())
        .collect();

    // In `AgentKind::ALL` order, and only for the kinds some profile names.
    let mut checks: Vec<Check> = AgentKind::ALL
        .into_iter()
        .filter(|kind| !named(*kind).is_empty())
        .map(|kind| {
            let names = named(kind).join(", ");
            if available.has(kind) {
                return Check::ok(
                    kind.as_str(),
                    format!("{} available — {names}", kind.binary()),
                );
            }
            let check = Check::fail(
                kind.as_str(),
                format!(
                    "{} is not on {} — these profiles cannot spawn sessions: {names}",
                    kind.binary(),
                    available.viewpoint()
                ),
            );
            match available.only_on_client(kind) {
                // The classic stale-service-PATH shape: present here, absent
                // in the process that would launch it.
                true => check.hint(format!(
                    "{} is on your PATH but not the daemon's — re-run scripts/install.sh so the service picks it up",
                    kind.binary()
                )),
                false => check.hint(format!(
                    "install {}, or point those profiles at an installed agent",
                    kind.binary()
                )),
            }
        })
        .collect();

    if !auto.is_empty() {
        let names = auto.join(", ");
        checks.push(match available.effective().first() {
            Some(kind) => Check::ok("auto", format!("resolves to {} — {names}", kind.as_str())),
            None => {
                let check = Check::fail(
                    "auto",
                    format!(
                        "no agent CLI on {} — these profiles cannot spawn sessions: {names}",
                        available.viewpoint()
                    ),
                );
                match available.stale_service_path() {
                    true => check.hint(
                        "the agents are on your PATH but not the daemon's — re-run scripts/install.sh so the service picks them up",
                    ),
                    false => check.hint("install claude, codex or opencode"),
                }
            }
        });
    }
    checks
}

/// The daemon's own environment, or the absence of one.
fn daemon_env_checks(daemon: Option<&DaemonReportDto>, available: &Availability) -> Vec<Check> {
    let Some(daemon) = daemon else {
        return vec![
            Check::fail(
                "daemon environment",
                "not reported — the daemon did not answer",
            )
            .hint("start it (`ariadne daemon start`) and run doctor again"),
        ];
    };

    let mut checks = vec![Check::ok(
        "PATH",
        daemon.path.clone().unwrap_or_else(|| "unset".to_string()),
    )];

    for binary in &daemon.agents {
        let on_client = binary
            .agent_kind
            .is_some_and(|kind| available.only_on_client(kind));
        checks.push(match binary.path {
            Some(_) => Check::ok(binary.name.clone(), describe(binary)),
            None if on_client => Check::warn(
                binary.name.clone(),
                format!(
                    "{} not on the daemon's PATH, though it is on yours",
                    binary.name
                ),
            )
            .hint("the service PATH is fixed at install time — re-run scripts/install.sh"),
            None => Check::warn(binary.name.clone(), describe(binary))
                .hint("not needed unless a profile runs on it"),
        });
    }

    for tool in &daemon.tools {
        checks.push(match tool.path {
            Some(_) => Check::ok(tool.name.clone(), describe(tool)),
            None => Check::fail(tool.name.clone(), describe(tool)).hint(format!(
                "the daemon cannot spawn sessions without {} on its PATH",
                tool.name
            )),
        });
    }

    // `writable: None` is "could not be told without writing something",
    // which a report does not do — it is not a finding, so it reads as ok.
    checks.push(match (daemon.db.exists, daemon.db.writable) {
        (false, _) => Check::warn("database", format!("{} does not exist yet", daemon.db.path)),
        (true, Some(false)) => {
            Check::fail("database", format!("{} is not writable", daemon.db.path))
                .hint("the daemon cannot record anything it does")
        }
        (true, _) => Check::ok("database", daemon.db.path.clone()),
    });

    let root = &daemon.worktree_root;
    checks.push(match (root.exists, root.writable) {
        (false, _) => Check::fail("worktree root", format!("{} does not exist", root.path))
            .hint("task worktrees are created there; check worktree_root in config.toml"),
        (true, Some(false)) => {
            Check::fail("worktree root", format!("{} is not writable", root.path))
                .hint("the daemon cannot create task worktrees")
        }
        (true, _) => Check::ok("worktree root", root.path.clone()),
    });
    checks
}

/// A daemon-side binary in the same words as a local one.
fn describe(binary: &BinaryDto) -> String {
    match (&binary.path, &binary.version) {
        (Some(path), Some(version)) => format!("{version} at {path}"),
        (Some(path), None) => format!("{path} (no version answer)"),
        (None, _) => format!("{} not found on the daemon's PATH", binary.name),
    }
}

// ---- rendering ---------------------------------------------------------

/// The report as lines: a heading per section, a line per check, and the hint
/// of anything that is not `ok` under it.
fn render(report: &Report) -> Vec<String> {
    let width = report
        .sections
        .iter()
        .flat_map(|s| s.checks.iter())
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for section in &report.sections {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(section.name.to_uppercase());
        for check in &section.checks {
            lines.push(format!(
                "  {:<6}{:<width$}   {}",
                check.status.label(),
                check.name,
                check.detail
            ));
            if let Some(hint) = &check.hint {
                lines.push(format!("  {:<6}{:<width$}   {hint}", "", ""));
            }
        }
    }
    lines
}

/// The one line that says whether to act on any of this.
fn verdict(report: &Report) -> String {
    let (failed, warned) = (report.count(Status::Fail), report.count(Status::Warn));
    match (failed, warned) {
        (0, 0) => "everything checks out".to_string(),
        (0, w) => format!("{w} warning(s) — nothing broken"),
        (f, 0) => format!("{f} failure(s) — Ariadne will not work until they are fixed"),
        (f, w) => format!("{f} failure(s) and {w} warning(s)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, agent_kind: Option<AgentKind>) -> ProfileDto {
        ProfileDto {
            id: format!("id-{name}"),
            name: name.to_string(),
            role: ariadne_core::Role::Engineer,
            agent_kind,
            model: None,
            system_prompt: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// Availability as the daemon reports it, whatever this shell has.
    fn daemon_sees(kinds: &[AgentKind]) -> Availability {
        Availability {
            daemon: Some(kinds.to_vec()),
            client: Vec::new(),
        }
    }

    fn report(sections: Vec<Section>) -> Report {
        Report::new("/tmp/ariadne.sock", sections)
    }

    fn section(checks: Vec<Check>) -> Vec<Section> {
        vec![Section::new("s", checks)]
    }

    #[test]
    fn a_report_takes_the_worst_status_of_its_checks() {
        assert_eq!(report(section(vec![])).status, Status::Ok);
        assert_eq!(
            report(section(vec![Check::ok("a", "-"), Check::warn("b", "-")])).status,
            Status::Warn
        );
        assert_eq!(
            report(section(vec![Check::warn("a", "-"), Check::fail("b", "-")])).status,
            Status::Fail
        );
    }

    /// Warnings are things to look at; only a failure is a broken install.
    #[test]
    fn only_a_failure_makes_the_command_exit_nonzero() {
        let ok = report(section(vec![Check::ok("a", "-"), Check::warn("b", "-")]));
        assert_eq!(
            format!("{:?}", ok.exit_code()),
            format!("{:?}", ExitCode::SUCCESS)
        );
        let bad = report(section(vec![Check::fail("a", "-")]));
        assert_eq!(
            format!("{:?}", bad.exit_code()),
            format!("{:?}", ExitCode::from(1))
        );
    }

    /// A profile pinned to an agent the daemon cannot launch is a failure,
    /// and the line has to name both the profile and the binary — that is the
    /// whole content of the report for whoever has to fix it.
    #[test]
    fn a_profile_whose_agent_is_missing_fails_by_name() {
        let profiles = [profile("Engineer", Some(AgentKind::Codex))];
        let checks = profile_checks(&profiles, &daemon_sees(&[AgentKind::ClaudeCode]));
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(checks[0].detail.contains("Engineer"), "{:?}", checks[0]);
        assert!(checks[0].detail.contains("codex"), "{:?}", checks[0]);
    }

    /// Every profile that names the same missing binary is on the same line.
    #[test]
    fn profiles_sharing_a_missing_agent_are_reported_together() {
        let profiles = [
            profile("Engineer", Some(AgentKind::Codex)),
            profile("Reviewer", Some(AgentKind::Codex)),
        ];
        let checks = profile_checks(&profiles, &daemon_sees(&[]));
        assert_eq!(checks.len(), 1);
        assert!(
            checks[0].detail.contains("Engineer, Reviewer"),
            "{:?}",
            checks[0]
        );
    }

    #[test]
    fn profiles_whose_agents_are_installed_are_ok() {
        let profiles = [
            profile("Engineer", Some(AgentKind::ClaudeCode)),
            profile("Planner", None),
        ];
        let checks = profile_checks(&profiles, &daemon_sees(&[AgentKind::ClaudeCode]));
        assert!(checks.iter().all(|c| c.status == Status::Ok), "{checks:?}");
    }

    /// An `auto` profile takes whatever is installed, so it only fails when
    /// nothing is — one missing agent among three is not its problem.
    #[test]
    fn an_auto_profile_fails_only_when_no_agent_is_available() {
        let profiles = [profile("Planner", None)];
        let ok = profile_checks(&profiles, &daemon_sees(&[AgentKind::Opencode]));
        assert_eq!(ok[0].status, Status::Ok);
        assert!(ok[0].detail.contains("opencode"), "{:?}", ok[0]);

        let bad = profile_checks(&profiles, &daemon_sees(&[]));
        assert_eq!(bad[0].status, Status::Fail);
        assert!(bad[0].detail.contains("Planner"), "{:?}", bad[0]);
    }

    /// The daemon's PATH decides, not the shell's: a binary this terminal can
    /// see is no use to the process that would spawn the session.
    #[test]
    fn availability_is_judged_by_what_the_daemon_sees() {
        let available = Availability {
            daemon: Some(vec![]),
            client: vec![AgentKind::Codex],
        };
        let checks = profile_checks(&[profile("Engineer", Some(AgentKind::Codex))], &available);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(
            checks[0]
                .hint
                .as_deref()
                .is_some_and(|h| h.contains("your PATH")),
            "{:?}",
            checks[0]
        );
    }

    /// With no daemon to ask, this shell's PATH is the only thing left to go
    /// on — better than reporting every profile as broken.
    #[test]
    fn without_a_daemon_the_client_path_stands_in() {
        let available = Availability {
            daemon: None,
            client: vec![AgentKind::ClaudeCode],
        };
        let checks = profile_checks(
            &[profile("Engineer", Some(AgentKind::ClaudeCode))],
            &available,
        );
        assert_eq!(checks[0].status, Status::Ok);
    }

    /// A missing agent no profile runs on is a warning: one agent is enough.
    #[test]
    fn an_unused_missing_agent_is_only_a_warning() {
        let agents = vec![
            Local {
                name: "claude".into(),
                path: Some("/bin/claude".into()),
                version: Some("1.0".into()),
            },
            Local {
                name: "codex".into(),
                path: None,
                version: None,
            },
            Local {
                name: "opencode".into(),
                path: None,
                version: None,
            },
        ];
        let available = Availability::new(None, &agents);
        let checks = agent_checks(&agents, &[], &available);
        assert_eq!(
            checks.len(),
            3,
            "no summary failure while one agent is there"
        );
        assert_eq!(checks[0].status, Status::Ok);
        assert_eq!(checks[1].status, Status::Warn);
        assert_eq!(checks[2].status, Status::Warn);
    }

    /// No agent at all is a different matter: nothing can be spawned.
    #[test]
    fn no_agent_at_all_is_a_failure() {
        let agents: Vec<Local> = ["claude", "codex", "opencode"]
            .iter()
            .map(|name| Local {
                name: (*name).into(),
                path: None,
                version: None,
            })
            .collect();
        let available = Availability::new(None, &agents);
        let checks = agent_checks(&agents, &[], &available);
        assert_eq!(checks.last().unwrap().status, Status::Fail);
    }

    /// A daemon that never answered still gets a section, and it is a failure
    /// — with no daemon there is nothing to spawn sessions.
    #[test]
    fn a_missing_daemon_report_is_a_failure_not_a_blank() {
        let checks = daemon_env_checks(None, &Availability::default());
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(checks[0].hint.is_some());
    }

    #[test]
    fn every_check_prints_under_its_section_with_its_hint() {
        let report = report(vec![Section::new(
            "client",
            vec![Check::fail("ariadned", "not found").hint("install it")],
        )]);
        let lines = render(&report);
        assert_eq!(lines[0], "CLIENT");
        assert!(lines[1].contains("fail"), "{lines:?}");
        assert!(lines[1].contains("ariadned"), "{lines:?}");
        assert!(lines[2].contains("install it"), "{lines:?}");
    }

    #[test]
    fn the_verdict_separates_warnings_from_failures() {
        assert_eq!(
            verdict(&report(section(vec![Check::ok("a", "-")]))),
            "everything checks out"
        );
        assert!(verdict(&report(section(vec![Check::warn("a", "-")]))).contains("nothing broken"));
        assert!(verdict(&report(section(vec![Check::fail("a", "-")]))).contains("1 failure"));
    }

    /// `--format json` has to carry every check with its status, or a script
    /// cannot act on the report.
    #[test]
    fn the_json_report_carries_every_check_and_its_status() {
        let report = report(vec![Section::new(
            "tools",
            vec![Check::fail("tmux", "not found").hint("install tmux")],
        )]);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["status"], "fail");
        assert_eq!(json["sections"][0]["name"], "tools");
        assert_eq!(json["sections"][0]["checks"][0]["status"], "fail");
        assert_eq!(json["sections"][0]["checks"][0]["hint"], "install tmux");
    }

    /// The manifest is data, not a script: `KEY="value"` lines, comments and
    /// blank lines, and nothing is executed to read them.
    #[test]
    fn the_install_manifest_is_read_as_key_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("install.env"),
            "# Written by scripts/install.sh\nARIADNE_PREFIX=\"/opt/bin\"\nARIADNE_APP=\"\"\n",
        )
        .unwrap();
        let manifest = read_manifest(dir.path());
        assert_eq!(manifest.get("ARIADNE_PREFIX").unwrap(), "/opt/bin");
        // An empty value names nothing and would only produce a bad path.
        assert!(!manifest.contains_key("ARIADNE_APP"));
    }

    #[test]
    fn a_home_with_a_broken_config_fails_the_config_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "prevent_slep = false\n").unwrap();
        let checks = home_checks(Some(dir.path()), Some(endpoint::parse_config(dir.path())));
        let config = checks.iter().find(|c| c.name == "config.toml").unwrap();
        assert_eq!(config.status, Status::Fail);
        assert!(config.detail.contains("prevent_slep"), "{config:?}");
    }

    /// A config that moves the database moves what is reported: a check on
    /// the default path would be about a file the daemon never opens.
    #[test]
    fn the_database_check_follows_the_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "db_path = \"/scratch/elsewhere.db\"\n",
        )
        .unwrap();
        let checks = home_checks(Some(dir.path()), Some(endpoint::parse_config(dir.path())));
        let db = checks.iter().find(|c| c.name == "database").unwrap();
        assert!(db.detail.contains("/scratch/elsewhere.db"), "{db:?}");
    }

    #[test]
    fn a_home_without_a_config_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let checks = home_checks(Some(dir.path()), Some(endpoint::parse_config(dir.path())));
        let config = checks.iter().find(|c| c.name == "config.toml").unwrap();
        assert_eq!(config.status, Status::Ok);
    }
}
