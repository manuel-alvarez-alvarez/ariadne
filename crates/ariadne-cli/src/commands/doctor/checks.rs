//! The host, the home and the daemon: what this shell can see for itself.
//!
//! Everything a binary is asked comes from [`ariadne_core::probe`], which the
//! daemon's own `/v1/doctor` uses too — the two halves of the report have to
//! ask the same questions the same way — and what this shell found is carried
//! in the [`BinaryDto`] the daemon answers with, so one [`describe`] writes
//! both halves.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use ariadne_api::doctor::BinaryDto;
use ariadne_client::endpoint::{self, ConfigError, FileConfig};
use ariadne_client::{Client, ClientError};
use ariadne_core::{AgentKind, probe};

use super::{Check, Status};

/// launchd label and systemd unit `scripts/install.sh` registers.
const LAUNCHD_LABEL: &str = "dev.ariadne.daemon";
const SYSTEMD_UNIT: &str = "ariadned.service";

/// Whose lookup a binary was missing from, as the report names it.
pub const HERE: &str = "PATH";
pub const THERE: &str = "the daemon's PATH";

const FIRST_START: &str = "the daemon creates it on its first start";

// ---- probing -----------------------------------------------------------

/// A coding-agent CLI as this shell can see it.
pub async fn agent(kind: AgentKind) -> BinaryDto {
    BinaryDto {
        agent_kind: Some(kind),
        ..tool(kind.binary(), "--version", false).await
    }
}

/// A binary on this shell's PATH, asked for its version — and, for a forge
/// CLI, whether it is signed in.
pub async fn tool(name: &str, version_flag: &str, authenticates: bool) -> BinaryDto {
    found(name, crate::commands::on_path(name), version_flag, authenticates).await
}

/// `ariadned` as `daemon start` would find it: next to this binary, else on PATH.
pub async fn ariadned() -> BinaryDto {
    found(
        "ariadned",
        crate::commands::find_ariadned().ok(),
        "--version",
        false,
    )
    .await
}

async fn found(name: &str, path: Option<PathBuf>, flag: &str, auths: bool) -> BinaryDto {
    let (version, authenticated) = match &path {
        Some(p) => (
            probe::probe_version(p, flag).await,
            match auths {
                true => probe::probe_auth(p).await,
                false => None,
            },
        ),
        None => (None, None),
    };
    BinaryDto {
        name: name.to_string(),
        agent_kind: None,
        path: path.map(|p| p.display().to_string()),
        version,
        authenticated,
    }
}

/// "claude 1.2.3 at /usr/local/bin/claude", as far as it is known. `lookup` is
/// whose PATH it was missing from, since the two halves of the report search
/// different ones.
pub fn describe(binary: &BinaryDto, lookup: &str) -> String {
    // Credentials read beside the version: "installed" and "usable" are not
    // the same question for a forge CLI.
    let signed = match binary.authenticated {
        Some(true) => ", signed in",
        Some(false) => ", not signed in",
        None => "",
    };
    match (&binary.path, &binary.version) {
        (Some(path), Some(version)) => format!("{version} at {path}{signed}"),
        (Some(path), None) => format!("{path} (no version answer){signed}"),
        (None, _) => format!("{} not found on {lookup}", binary.name),
    }
}

// ---- sections ----------------------------------------------------------

pub fn client(ariadned: &BinaryDto, daemon_reachable: bool) -> Vec<Check> {
    let missing = "not found next to ariadne or on PATH";
    vec![
        Check::ok("ariadne", format!("ariadne {}", env!("CARGO_PKG_VERSION"))),
        match (&ariadned.path, daemon_reachable) {
            (Some(_), _) => Check::ok("ariadned", describe(ariadned, HERE)),
            // A daemon that is answering is evidently installed somewhere; not
            // finding its binary from here only costs `ariadne daemon start`.
            (None, true) => Check::warn("ariadned", missing).hint(
                "the running daemon was started from elsewhere; `ariadne daemon start` needs it here",
            ),
            (None, false) => Check::fail("ariadned", missing)
                .hint("install it beside ariadne: scripts/install.sh"),
        },
    ]
}

pub async fn home(
    home: Option<&Path>,
    config: Option<Result<Option<FileConfig>, ConfigError>>,
) -> Vec<Check> {
    let Some(home) = home else {
        return vec![
            Check::fail("home", "no ariadne home could be resolved")
                .hint("set ARIADNE_HOME, or run from an account with a home directory"),
        ];
    };
    // Only `None` for a home that could not be resolved, handled above.
    let config = config.unwrap_or(Ok(None));
    // Wherever the config puts it: a report on the default path would be
    // about a file the daemon never opens.
    let db = config
        .as_ref()
        .ok()
        .and_then(|c| c.as_ref()?.db_path.clone())
        .unwrap_or_else(|| home.join("ariadne.db"));

    vec![
        there("home", home, home.is_dir(), "does not exist yet", FIRST_START),
        match &config {
            Ok(None) => Check::ok("config.toml", "none — built-in defaults"),
            Ok(Some(_)) => Check::ok(
                "config.toml",
                endpoint::config_file(home).display().to_string(),
            ),
            Err(e) => Check::fail("config.toml", e.to_string())
                .hint("the daemon refuses to start on a config it cannot read"),
        },
        {
            let socket = endpoint::socket_path(home);
            there(
                "socket",
                &socket,
                socket.exists(),
                "does not exist",
                "no daemon has listened on this home yet",
            )
        },
        // A database from before the schema was squashed is why a daemon that
        // used to start does not, and nothing else in the report would say so:
        // the daemon refuses to open it, so it is not running to be asked and
        // this shell reads the file itself.
        match ariadne_store::pre_squash_database(&db).await {
            Some(why) => Check::fail("database", why).hint(format!("rm {}", db.display())),
            None => there(
                "database",
                &db,
                db.is_file(),
                "does not exist yet",
                FIRST_START,
            ),
        },
        {
            let pid_file = endpoint::pid_file(home);
            match std::fs::read_to_string(&pid_file) {
                Ok(pid) => Check::ok(
                    "pidfile",
                    format!("{} (pid {})", pid_file.display(), pid.trim()),
                ),
                Err(_) => there(
                    "pidfile",
                    &pid_file,
                    false,
                    "does not exist",
                    "written by a daemon started outside a service manager",
                ),
            }
        },
    ]
}

/// A path the daemon needs: named plainly when it is there, and "<path>
/// <missing>" with the way out when it is not. Never a failure — each is
/// something the daemon creates, or a daemon nobody has started yet.
fn there(name: &str, path: &Path, exists: bool, missing: &str, hint: &str) -> Check {
    match exists {
        true => Check::ok(name, path.display().to_string()),
        false => Check::warn(name, format!("{} {missing}", path.display())).hint(hint),
    }
}

pub async fn daemon(
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
        Err(e) => match e.hint() {
            Some(hint) => Check::fail("reachable", e.human()).hint(hint),
            None => Check::fail("reachable", e.human()),
        },
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

    checks.push(
        match home.map(|h| h.join("install.env")).filter(|p| p.is_file()) {
            Some(path) => Check::ok("install manifest", path.display().to_string()),
            None => Check::warn("install manifest", "no install.env")
                .hint("installed by hand? scripts/install.sh writes one"),
        },
    );
    checks.push(service(&home.map(read_manifest).unwrap_or_default()).await);
    checks
}

/// Whether the daemon is registered with the OS service manager, read-only:
/// doctor reports what it finds and never registers, loads or repairs.
///
/// launchd and systemd differ only in the words: which file registers the
/// daemon, what "running" is called, and the command that starts it.
async fn service(manifest: &BTreeMap<String, String>) -> Check {
    let from_manifest = |key: &str, default: PathBuf| {
        manifest.get(key).map(PathBuf::from).unwrap_or(default)
    };
    let (unit, file, name, up, command, start) = if cfg!(target_os = "macos") {
        let plist = from_manifest("ARIADNE_PLIST", default_plist());
        let start = format!(
            "load it with: launchctl bootstrap gui/$(id -u) {}",
            plist.display()
        );
        let name = format!("launchd {LAUNCHD_LABEL}");
        let command = vec!["launchctl", "list", LAUNCHD_LABEL];
        (plist, "launchd plist", name, "loaded", command, start)
    } else if cfg!(target_os = "linux") {
        (
            from_manifest("ARIADNE_UNIT", default_unit()),
            "systemd unit",
            format!("systemd --user {SYSTEMD_UNIT}"),
            "active",
            vec!["systemctl", "--user", "is-active", SYSTEMD_UNIT],
            format!("start it with: systemctl --user start {SYSTEMD_UNIT}"),
        )
    } else {
        return Check::warn("service", "no service manager Ariadne knows on this OS")
            .hint("run ariadned yourself, or with whatever supervisor you use");
    };

    if !unit.is_file() {
        return Check::warn("service", format!("no {file} at {}", unit.display())).hint(
            "the daemon will not come back after a reboot; scripts/install.sh registers it",
        );
    }
    match probe::probe_status(command[0], &command[1..]).await {
        true => Check::ok("service", format!("{name} {up}")),
        false => Check::warn("service", format!("{name} installed but not {up}")).hint(start),
    }
}

fn default_plist() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
}

fn default_unit() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("systemd/user")
        .join(SYSTEMD_UNIT)
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

/// What this shell has of the two kinds of tool: tmux and git, without either
/// of which no session can be spawned at all, and the forge CLIs, which only a
/// task published to a forge needs.
pub fn tools(required: &[BinaryDto], forges: &[BinaryDto]) -> Vec<Check> {
    let mut checks: Vec<Check> = required.iter().map(|t| required_tool(t, HERE)).collect();
    checks.extend(forges.iter().map(|forge| forge_check(forge, HERE)));
    checks
}

/// tmux or git: without it nothing spawns, wherever it is reported from.
pub fn required_tool(tool: &BinaryDto, lookup: &str) -> Check {
    Check::when(
        tool.name.clone(),
        tool.path.is_some(),
        describe(tool, lookup),
        Status::Fail,
        match lookup {
            THERE => format!(
                "the daemon cannot spawn sessions without {} on its PATH",
                tool.name
            ),
            _ => format!(
                "install {} — Ariadne cannot run sessions without it",
                tool.name
            ),
        },
    )
}

/// One forge CLI, wherever it is reported from: installed and signed in is the
/// only state a published task can be watched in, and the other two are
/// warnings rather than failures — a task landed locally needs neither.
pub fn forge_check(forge: &BinaryDto, lookup: &str) -> Check {
    let (name, detail) = (forge.name.clone(), describe(forge, lookup));
    let (forge_word, host) = match name.as_str() {
        "glab" => ("merge request", "GitLab"),
        _ => ("pull request", "GitHub"),
    };
    match (forge.path.is_some(), forge.authenticated) {
        (true, Some(false)) => Check::warn(name.clone(), detail).hint(format!(
            "run `{name} auth login` — Ariadne watches a published {forge_word} through it, \
             and every poll of one fails while it is signed out"
        )),
        (true, _) => Check::ok(name, detail),
        (false, _) => Check::warn(name.clone(), detail).hint(format!(
            "install {name} to publish tasks to {host} — not needed for tasks landed locally"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::tests::{binary, by_name};

    /// The forge CLIs are the two questions nothing else asks: installed, and
    /// signed in. Neither is a failure — a task landed locally needs neither —
    /// and both are worth a warning with the command that fixes them. tmux and
    /// git are the opposite: without them nothing spawns at all.
    #[test]
    fn a_forge_cli_is_reported_on_being_installed_and_being_signed_in() {
        let checks = tools(
            &[
                binary("tmux", None, true, None),
                binary("git", None, false, None),
            ],
            &[
                binary("gh", None, true, Some(false)),
                binary("glab", None, false, None),
            ],
        );
        assert_eq!(by_name(&checks, "tmux").status, Status::Ok);
        assert_eq!(by_name(&checks, "git").status, Status::Fail);

        // Installed and signed out: a warning that names the way out of it.
        let gh = by_name(&checks, "gh");
        assert_eq!(gh.status, Status::Warn);
        assert!(gh.detail.contains("not signed in"), "{gh:?}");
        assert!(
            gh.hint
                .as_deref()
                .is_some_and(|h| h.contains("gh auth login")),
            "{gh:?}"
        );
        // Not installed at all: also a warning, and about GitLab.
        let glab = by_name(&checks, "glab");
        assert_eq!(glab.status, Status::Warn);
        assert!(
            glab.hint.as_deref().is_some_and(|h| h.contains("GitLab")),
            "{glab:?}"
        );
        // And signed in is nothing to report.
        let checks = tools(&[], &[binary("gh", None, true, Some(true))]);
        assert_eq!(checks[0].status, Status::Ok);
        assert!(checks[0].detail.contains("signed in"), "{:?}", checks[0]);
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

    /// A config the daemon would refuse to start on fails the check by the
    /// line it could not read; a home with no config at all is fine. A config
    /// that moves the database moves what is reported, since a check on the
    /// default path would be about a file the daemon never opens.
    #[tokio::test]
    async fn a_home_is_checked_against_the_config_it_carries() {
        let dir = tempfile::tempdir().unwrap();
        let checks = home(Some(dir.path()), Some(endpoint::parse_config(dir.path()))).await;
        assert_eq!(by_name(&checks, "config.toml").status, Status::Ok);

        std::fs::write(
            dir.path().join("config.toml"),
            "db_path = \"/scratch/elsewhere.db\"\n",
        )
        .unwrap();
        let checks = home(Some(dir.path()), Some(endpoint::parse_config(dir.path()))).await;
        let db = by_name(&checks, "database");
        assert!(db.detail.contains("/scratch/elsewhere.db"), "{db:?}");

        std::fs::write(dir.path().join("config.toml"), "prevent_slep = false\n").unwrap();
        let checks = home(Some(dir.path()), Some(endpoint::parse_config(dir.path()))).await;
        let config = by_name(&checks, "config.toml");
        assert_eq!(config.status, Status::Fail);
        assert!(config.detail.contains("prevent_slep"), "{config:?}");
    }

    /// A database from before the schema was squashed fails the check, by the
    /// name of the file to delete. It is the one thing in the report the daemon
    /// cannot answer for: it refuses to open such a database, so it is not
    /// running to be asked.
    #[tokio::test]
    async fn a_database_from_before_the_squash_fails_the_check() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ariadne.db");
        drop(ariadne_store::Store::open(&db).await.unwrap());

        let checks = home(Some(dir.path()), Some(endpoint::parse_config(dir.path()))).await;
        assert_eq!(by_name(&checks, "database").status, Status::Ok);

        // A migration this release does not ship is what every database of
        // that era has, and what sqlx then refuses to run over.
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", db.display()))
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, installed_on, success,
                                           checksum, execution_time)
             VALUES (2, 'repositories', '2025-01-01 00:00:00', 1, x'00', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let checks = home(Some(dir.path()), Some(endpoint::parse_config(dir.path()))).await;
        let check = by_name(&checks, "database");
        assert_eq!(check.status, Status::Fail, "{check:?}");
        assert!(
            check.detail.contains(&db.display().to_string()),
            "{check:?}"
        );
        assert!(
            check.hint.as_deref().is_some_and(|h| h.contains("rm ")),
            "and says what to do about it: {check:?}"
        );
    }
}
