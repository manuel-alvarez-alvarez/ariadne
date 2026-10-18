//! Daemon configuration.
//!
//! Everything lives under a single root directory (default `~/.ariadne`),
//! optionally overridden by `ARIADNE_HOME` or `--home`. A `config.toml` in
//! the root can override individual paths and enable the TCP listener.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::http::HeaderValue;
use url::Url;

use ariadne_client::endpoint::{self, AcpAgentConfig};

use crate::ai_permissions::hardware::HardwareOverride;

/// Fully resolved daemon configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub socket_path: PathBuf,
    pub db_path: PathBuf,
    /// The telemetry database: every log record and metric sample (033).
    pub telemetry_db_path: PathBuf,
    /// How long a raw metric sample is kept before retention deletes it
    /// (it is rolled into an hourly bucket well before then).
    pub telemetry_samples_days: i64,
    /// How long a log record is kept before retention deletes it.
    pub telemetry_logs_days: i64,
    /// How long an hourly metric rollup is kept before retention deletes it.
    pub telemetry_rollups_days: i64,
    pub worktree_root: PathBuf,
    pub run_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub pid_file: PathBuf,
    pub tcp_listen: Option<SocketAddr>,
    pub tcp_allowed_origins: Vec<String>,
    /// Separate signed ingress. None binds loopback on an operating-system port.
    pub webhook_listen: Option<SocketAddr>,
    pub webhook_public_url: Option<String>,
    /// The localtunnel server the webhook tunnel registers with (027).
    pub tunnel_host: String,
    /// The subdomain the tunnel asks for. It wins over the stored one.
    pub tunnel_subdomain: Option<String>,
    pub log_filter: String,
    /// The `ariadne` binary every session's MCP server is launched with.
    pub cli_bin: String,
    pub delete_merged_branches: bool,
    pub delete_merged_worktrees: bool,
    pub prevent_sleep: bool,
    pub auto_switch: bool,
    pub exhausted_patterns: Vec<String>,
    /// Classify a failed ACP session's error with the AI permission model's
    /// local Kev service, as an advisory `session.diagnosis` event (024).
    pub ai_failure_diagnosis: bool,
    /// User-defined ACP agent commands appended to the built-in registry.
    pub acp_agents: Vec<AcpAgentConfig>,
    /// ACP registry index URL, fetched only on an explicit refresh.
    pub acp_registry_url: String,
    /// The Python interpreter the model's install runs on (022). `None` finds
    /// Python 3.13, Python 3.12, then `python3` on the daemon's PATH.
    pub python_bin: Option<String>,
    /// The `nvidia-smi` the hardware probe runs (022, flavours and devices).
    /// `None` looks up `nvidia-smi` on the daemon's PATH.
    pub nvidia_smi_bin: Option<String>,
    /// The `gh` the GitHub integration runs (025). `None` looks up `gh` on
    /// the daemon's PATH.
    pub gh_bin: Option<String>,
    /// A command that stands in for the whole model install — the venv, pip
    /// and the weights — so the suite proves the install's states without
    /// downloading two gigabytes. Set by the test harness alone: it is not a
    /// key of `config.toml`.
    pub ai_permissions_installer: Option<Vec<String>>,
    /// A command that stands in for the model server's Python in the suite:
    /// the daemon adds the entry point and the arguments. Like the installer
    /// seam, it is not a `config.toml` key.
    pub ai_permissions_serve_command: Option<Vec<String>>,
    /// Where the model server answers, in place of one the daemon started.
    /// Set by the test harness alone, for the same reason.
    pub ai_permissions_endpoint: Option<String>,
    /// The whole hardware probe, in place of the real machine. Set by the
    /// test harness alone, for the same reason.
    pub ai_permissions_hardware: Option<HardwareOverride>,
}

/// Default `ariadne` CLI: sibling of the running ariadned, else PATH lookup.
fn default_cli_bin() -> String {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join("ariadne");
        if sibling.is_file() {
            return sibling.display().to_string();
        }
    }
    "ariadne".to_string()
}

impl Config {
    /// Resolve config: `--home` flag > `ARIADNE_HOME` > `~/.ariadne`,
    /// then apply `config.toml` overrides, then create the directories.
    ///
    /// The home and socket ordering comes from `ariadne_client::endpoint`, so
    /// the CLI and the MCP server address exactly the daemon this starts.
    pub fn load(home_override: Option<PathBuf>) -> Result<Self> {
        let root = endpoint::home(home_override)
            .context("cannot determine home directory; pass --home")?;

        // The strict reading of `config.toml` lives beside the socket
        // resolution it shares a file with, so `ariadne doctor` reports on
        // exactly what would stop this daemon from starting.
        let file = endpoint::parse_config(&root)?.unwrap_or_default();
        validate_telemetry_retention_days(&file)?;

        let config = Config {
            socket_path: file
                .socket_path
                .unwrap_or_else(|| endpoint::default_socket_path(&root)),
            db_path: file.db_path.unwrap_or_else(|| root.join("ariadne.db")),
            telemetry_db_path: file
                .telemetry_db_path
                .unwrap_or_else(|| root.join("telemetry.db")),
            telemetry_samples_days: file.telemetry_samples_days.unwrap_or(30),
            telemetry_logs_days: file.telemetry_logs_days.unwrap_or(14),
            telemetry_rollups_days: file.telemetry_rollups_days.unwrap_or(365),
            worktree_root: file.worktree_root.unwrap_or_else(|| root.join("worktrees")),
            run_dir: file.run_dir.unwrap_or_else(|| root.join("run")),
            skills_dir: file.skills_dir.unwrap_or_else(|| root.join("skills")),
            pid_file: endpoint::pid_file(&root),
            tcp_listen: file.tcp_listen,
            tcp_allowed_origins: file.tcp_allowed_origins,
            webhook_listen: file.webhook_listen,
            webhook_public_url: file.webhook_public_url,
            tunnel_host: file
                .tunnel_host
                .unwrap_or_else(|| "https://localtunnel.me".into()),
            tunnel_subdomain: file.tunnel_subdomain,
            log_filter: file.log_filter.unwrap_or_else(|| "info".to_string()),
            cli_bin: file.cli_bin.unwrap_or_else(default_cli_bin),
            delete_merged_branches: file.delete_merged_branches.unwrap_or(true),
            delete_merged_worktrees: file.delete_merged_worktrees.unwrap_or(true),
            prevent_sleep: file.prevent_sleep.unwrap_or(true),
            auto_switch: file.auto_switch.unwrap_or(true),
            exhausted_patterns: file
                .exhausted_patterns
                .unwrap_or_else(default_exhausted_patterns),
            ai_failure_diagnosis: file.ai_failure_diagnosis.unwrap_or(false),
            acp_agents: file.acp_agents,
            acp_registry_url: file.acp_registry_url.unwrap_or_else(|| {
                "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json".into()
            }),
            python_bin: file.python_bin,
            nvidia_smi_bin: file.nvidia_smi_bin,
            gh_bin: file.gh_bin,
            ai_permissions_installer: None,
            ai_permissions_serve_command: None,
            ai_permissions_endpoint: None,
            ai_permissions_hardware: None,
            root,
        };

        validate_tcp_config(&config)?;

        for dir in [
            &config.root,
            &config.worktree_root,
            &config.run_dir,
            &config.skills_dir,
        ] {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }

        Ok(config)
    }

    /// `ariadned --check-config`: read `<home>/config.toml` the way [`load`]
    /// reads it, and say what it found.
    ///
    /// Only the reading: no directory is created, no database is opened and no
    /// socket is touched, so it is safe to run against the home of a daemon
    /// that is up. A file this refuses is a file the next start would refuse,
    /// in the same words — that is the whole point of it.
    ///
    /// [`load`]: Config::load
    pub fn check(home_override: Option<PathBuf>) -> Result<String> {
        let root = endpoint::home(home_override)
            .context("cannot determine home directory; pass --home")?;
        let path = endpoint::config_file(&root);
        let file = endpoint::parse_config(&root)?;
        let has_file = file.is_some();
        let file = file.unwrap_or_default();
        validate_tcp_file_config(&file)?;
        validate_telemetry_retention_days(&file)?;
        let skills_dir = file
            .skills_dir
            .as_ref()
            .cloned()
            .unwrap_or_else(|| root.join("skills"));
        let verdict = if has_file {
            format!("{}: ok", path.display())
        } else {
            format!("{}: none — every default applies", path.display())
        };
        Ok(format!("{verdict}\nskills_dir = {}", skills_dir.display()))
    }
}

const TCP_LISTEN_LOOPBACK_ERROR: &str = "tcp_listen must use a loopback address; use an SSH tunnel to reach the daemon from another host";

fn validate_tcp_config(config: &Config) -> Result<()> {
    validate_tcp_listen(config.tcp_listen)?;
    validate_tcp_allowed_origins(&config.tcp_allowed_origins)
}

fn validate_tcp_file_config(config: &endpoint::FileConfig) -> Result<()> {
    validate_tcp_listen(config.tcp_listen)?;
    validate_tcp_allowed_origins(&config.tcp_allowed_origins)
}

fn validate_tcp_listen(tcp_listen: Option<SocketAddr>) -> Result<()> {
    if tcp_listen.is_some_and(|address| !address.ip().is_loopback()) {
        anyhow::bail!(TCP_LISTEN_LOOPBACK_ERROR);
    }
    Ok(())
}

fn validate_tcp_allowed_origins(origins: &[String]) -> Result<()> {
    for origin in origins {
        origin
            .parse::<HeaderValue>()
            .with_context(|| format!("tcp_allowed_origins has an invalid origin: {origin}"))?;
        let parsed = Url::parse(origin).ok();
        if parsed.is_none_or(|parsed| parsed.origin().ascii_serialization() != *origin) {
            anyhow::bail!(
                "tcp_allowed_origins must contain a serialized origin (scheme://host[:port], without a path or trailing slash): {origin}"
            );
        }
    }
    Ok(())
}

fn validate_telemetry_retention_days(config: &endpoint::FileConfig) -> Result<()> {
    for (key, days) in [
        ("telemetry_samples_days", config.telemetry_samples_days),
        ("telemetry_logs_days", config.telemetry_logs_days),
        ("telemetry_rollups_days", config.telemetry_rollups_days),
    ] {
        if days.is_some_and(|days| days <= 0) {
            anyhow::bail!("{key} must be a positive number of days");
        }
    }
    Ok(())
}

pub fn default_exhausted_patterns() -> Vec<String> {
    [
        "hit your usage limit",
        "usage limit reached",
        "usage limit",
        "session limit",
        "weekly limit",
        "rate limit",
        "quota",
        "try again at",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home with `config.toml` in it, written from `toml`.
    fn home_with(toml: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        std::fs::write(dir.path().join("home/config.toml"), toml).unwrap();
        dir
    }

    /// What the file does not say keeps its default — and what it may say is
    /// only ever what `FileConfig` names, since the reading is strict: a key
    /// this daemon no longer has stops it rather than being ignored (see
    /// `endpoint::parse_config`).
    #[test]
    fn a_config_file_that_says_nothing_keeps_every_default() {
        let dir = home_with("");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert!(config.delete_merged_worktrees);
        assert!(config.delete_merged_branches);
        assert!(config.prevent_sleep);
        assert!(config.auto_switch);
        assert_eq!(
            config.telemetry_db_path,
            dir.path().join("home/telemetry.db")
        );
        assert_eq!(config.telemetry_samples_days, 30);
        assert_eq!(config.telemetry_logs_days, 14);
        assert_eq!(config.telemetry_rollups_days, 365);
        assert_eq!(config.exhausted_patterns, default_exhausted_patterns());
        assert!(!config.ai_failure_diagnosis);
        assert_eq!(
            config.acp_registry_url,
            "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json"
        );
        assert_eq!(config.python_bin, None);
        assert_eq!(config.nvidia_smi_bin, None);
        assert_eq!(config.gh_bin, None);
        assert_eq!(config.skills_dir, dir.path().join("home/skills"));
    }

    #[test]
    fn webhook_configuration_uses_a_random_port_unless_an_address_is_given() {
        let dir = home_with("");
        let cfg = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(cfg.webhook_listen, None);
        assert_eq!(cfg.webhook_public_url, None);
        assert_eq!(cfg.tunnel_host, "https://localtunnel.me");
        assert_eq!(cfg.tunnel_subdomain, None);
        let dir = home_with(
            "webhook_listen = \"127.0.0.1:8181\"\nwebhook_public_url = \"https://hooks.example\"\n\
             tunnel_host = \"https://tunnel.example\"\ntunnel_subdomain = \"widgets\"\n",
        );
        let cfg = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(cfg.webhook_listen, Some("127.0.0.1:8181".parse().unwrap()));
        assert_eq!(
            cfg.webhook_public_url.as_deref(),
            Some("https://hooks.example")
        );
        assert_eq!(cfg.tunnel_host, "https://tunnel.example");
        assert_eq!(cfg.tunnel_subdomain.as_deref(), Some("widgets"));
    }

    /// The forge CLI is a key a user may set, a path or a bare name.
    #[test]
    fn the_forge_cli_key_is_read() {
        let dir = home_with("gh_bin = \"/opt/bin/gh\"\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.gh_bin.as_deref(), Some("/opt/bin/gh"));
    }

    #[test]
    fn exhausted_settings_are_read_and_the_pattern_list_replaces_the_default() {
        let dir = home_with("auto_switch = false\nexhausted_patterns = [\"credits gone\"]\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert!(!config.auto_switch);
        assert_eq!(config.exhausted_patterns, ["credits gone"]);
    }

    /// The advisory diagnosis is off unless the file turns it on, and never
    /// moves `auto_switch` or the exhausted patterns.
    #[test]
    fn ai_failure_diagnosis_is_read_and_off_by_default() {
        let dir = home_with("");
        assert!(
            !Config::load(Some(dir.path().join("home")))
                .unwrap()
                .ai_failure_diagnosis
        );

        let dir = home_with("ai_failure_diagnosis = true\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert!(config.ai_failure_diagnosis);
        assert!(config.auto_switch);
        assert_eq!(config.exhausted_patterns, default_exhausted_patterns());
    }

    /// The AI permission model's Python and `nvidia-smi` keys are read; the
    /// test seams beside them are not keys at all, and a file naming one is
    /// refused like any other unknown key.
    #[test]
    fn the_ai_permissions_keys_a_user_may_set_are_read_and_the_test_seams_are_not() {
        let dir = home_with(
            "python_bin = \"/opt/python3.12/bin/python3\"\n\
             nvidia_smi_bin = \"/opt/bin/nvidia-smi\"\n",
        );
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(
            config.python_bin.as_deref(),
            Some("/opt/python3.12/bin/python3")
        );
        assert_eq!(
            config.nvidia_smi_bin.as_deref(),
            Some("/opt/bin/nvidia-smi")
        );
        assert_eq!(config.ai_permissions_installer, None);
        assert_eq!(config.ai_permissions_endpoint, None);
        assert_eq!(config.ai_permissions_hardware, None);

        for seam in [
            "ai_permissions_installer = [\"/bin/true\"]\n",
            "ai_permissions_endpoint = \"http://x\"\n",
            "ai_permissions_release_url = \"http://x\"\n",
            "ai_permissions_hardware = { os = \"linux\", arch = \"x86_64\", memory_gb = 16 }\n",
        ] {
            let dir = home_with(seam);
            assert!(
                Config::load(Some(dir.path().join("home"))).is_err(),
                "a test seam is no key of the user's config: {seam}"
            );
        }
    }

    /// A key the daemon no longer has stops the daemon rather than being ignored.
    #[test]
    fn an_unknown_key_stops_the_daemon() {
        let dir = home_with("obsolete_option = true\n");
        let error = Config::load(Some(dir.path().join("home"))).unwrap_err();
        assert!(
            format!("{error:#}").contains("obsolete_option"),
            "and says which key it could not read: {error:#}"
        );
    }

    /// The check is the start's own reading, without the start: a file the
    /// daemon would refuse is refused here by the key it could not read, and a
    /// home with no file at all is not an error — it is every default.
    #[test]
    fn check_config_reads_the_file_the_next_start_would_read() {
        let dir = home_with("prevent_sleep = false\n");
        let verdict = Config::check(Some(dir.path().join("home"))).unwrap();
        assert!(verdict.contains("config.toml: ok"), "{verdict}");
        assert!(verdict.contains("skills_dir ="), "{verdict}");

        let empty = tempfile::tempdir().unwrap();
        let verdict = Config::check(Some(empty.path().to_path_buf())).unwrap();
        assert!(verdict.contains("none"), "{verdict}");
        assert!(
            !empty.path().join("worktrees").exists(),
            "checking a config creates nothing"
        );

        let broken = home_with("prevent_slep = false\n");
        let error = Config::check(Some(broken.path().join("home"))).unwrap_err();
        assert!(error.to_string().contains("config.toml"), "{error}");
        assert!(
            format!("{:#}", error).contains("prevent_slep"),
            "and says which key it could not read: {error:#}"
        );
    }

    #[test]
    fn tcp_listener_requires_a_loopback_address_at_start_and_check() {
        for address in ["0.0.0.0:7676", "192.168.1.10:7676"] {
            let dir = home_with(&format!("tcp_listen = \"{address}\"\n"));
            let start = Config::load(Some(dir.path().join("home")))
                .expect_err("a non-loopback listener must stop startup");
            let check = Config::check(Some(dir.path().join("home")))
                .expect_err("the config check must report the startup refusal");
            assert_eq!(start.to_string(), check.to_string());
            assert!(start.to_string().contains("SSH tunnel"));
        }

        for address in ["127.0.0.1:7676", "[::1]:7676"] {
            let dir = home_with(&format!("tcp_listen = \"{address}\"\n"));
            assert!(Config::load(Some(dir.path().join("home"))).is_ok());
            assert!(Config::check(Some(dir.path().join("home"))).is_ok());
        }
    }

    #[test]
    fn tcp_allowed_origins_are_read_from_config() {
        let dir = home_with("tcp_allowed_origins = [\"https://console.example\"]\n");
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.tcp_allowed_origins, ["https://console.example"]);
    }

    #[test]
    fn tcp_allowed_origins_require_serialized_origins() {
        for origin in [
            "*",
            "null",
            "https://console.example/path",
            "https://console.example/",
        ] {
            let dir = home_with(&format!("tcp_allowed_origins = [\"{origin}\"]\n"));
            let start = Config::load(Some(dir.path().join("home")))
                .expect_err("an origin that a browser cannot send must stop startup");
            let check = Config::check(Some(dir.path().join("home")))
                .expect_err("the config check must report the startup refusal");
            assert_eq!(start.to_string(), check.to_string());
            assert!(start.to_string().contains("tcp_allowed_origins"));
        }
    }

    #[test]
    fn telemetry_keys_are_read_from_config() {
        let dir = home_with("");
        let telemetry_db_path = dir.path().join("home/custom-telemetry.db");
        std::fs::write(
            dir.path().join("home/config.toml"),
            format!(
                "telemetry_db_path = {:?}\ntelemetry_samples_days = 7\n\
                 telemetry_logs_days = 3\ntelemetry_rollups_days = 90\n",
                telemetry_db_path
            ),
        )
        .unwrap();
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.telemetry_db_path, telemetry_db_path);
        assert_eq!(config.telemetry_samples_days, 7);
        assert_eq!(config.telemetry_logs_days, 3);
        assert_eq!(config.telemetry_rollups_days, 90);
    }

    #[test]
    fn telemetry_retention_days_must_be_positive_at_start_and_check() {
        for key in [
            "telemetry_samples_days",
            "telemetry_logs_days",
            "telemetry_rollups_days",
        ] {
            let dir = home_with(&format!("{key} = 0\n"));
            let start = Config::load(Some(dir.path().join("home")))
                .expect_err("a non-positive retention window must stop startup");
            let check = Config::check(Some(dir.path().join("home")))
                .expect_err("the config check must report the startup refusal");
            assert_eq!(start.to_string(), check.to_string());
            assert!(start.to_string().contains(key));
        }
    }

    #[test]
    fn a_config_file_is_read_into_the_daemons_own_shape() {
        let dir = home_with("");
        let skills_dir = dir.path().join("custom-skills");
        std::fs::write(
            dir.path().join("home/config.toml"),
            format!(
                "log_filter = \"debug\"\nprevent_sleep = false\nskills_dir = {:?}\n",
                skills_dir
            ),
        )
        .unwrap();
        let config = Config::load(Some(dir.path().join("home"))).unwrap();
        assert_eq!(config.log_filter, "debug");
        assert!(!config.prevent_sleep);
        assert_eq!(config.skills_dir, skills_dir);
        assert!(
            config.delete_merged_worktrees,
            "and what the file does not say keeps its default"
        );
    }
}
