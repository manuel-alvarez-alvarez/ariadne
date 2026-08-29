//! The service manager behind a home's daemon, and what to ask it.
//!
//! `scripts/install.sh` registers `ariadned` as a user service — launchd on
//! macOS, `systemd --user` on Linux — and records what it wrote in that home's
//! `install.env`. A daemon under a service is not this shell's to spawn or to
//! signal: a process started beside a loaded service fights it for the socket,
//! and a plain SIGTERM to a launchd job whose `KeepAlive` says
//! `SuccessfulExit=false` is a clean exit it will not be restarted from. So
//! `daemon start`, `stop` and `restart` ask the manager wherever there is one,
//! and say which command they asked with.
//!
//! Nothing here registers, loads or repairs anything: the manifest is read as
//! data and the manager is asked one read-only question, exactly as `ariadne
//! doctor` asks it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ariadne_core::probe;

/// launchd label and systemd unit `scripts/install.sh` registers.
pub const LAUNCHD_LABEL: &str = "dev.ariadne.daemon";
pub const SYSTEMD_UNIT: &str = "ariadned.service";

/// The service manager of this host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Launchd,
    Systemd,
}

/// What is being asked of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Manager {
    /// The manager this OS runs user services under, where Ariadne knows one.
    pub fn of_this_host() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Launchd)
        } else if cfg!(target_os = "linux") {
            Some(Self::Systemd)
        } else {
            None
        }
    }

    /// How a report names it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Launchd => "launchd",
            Self::Systemd => "systemd --user",
        }
    }

    /// What the manager calls the service.
    pub fn unit(self) -> &'static str {
        match self {
            Self::Launchd => LAUNCHD_LABEL,
            Self::Systemd => SYSTEMD_UNIT,
        }
    }

    /// What the file that registers it is called, in a report's words.
    pub fn unit_file_kind(self) -> &'static str {
        match self {
            Self::Launchd => "launchd plist",
            Self::Systemd => "systemd unit",
        }
    }

    /// `install.env` key naming that file.
    pub fn manifest_key(self) -> &'static str {
        match self {
            Self::Launchd => "ARIADNE_PLIST",
            Self::Systemd => "ARIADNE_UNIT",
        }
    }

    /// The manager's own word for a service it is holding up: launchd loads a
    /// job, systemd activates a unit.
    pub fn up_word(self) -> &'static str {
        match self {
            Self::Launchd => "loaded",
            Self::Systemd => "active",
        }
    }

    /// Where `scripts/install.sh` writes that file when nothing says otherwise
    /// — the same paths `scripts/lib.sh` resolves.
    pub fn default_unit_file(self) -> PathBuf {
        match self {
            Self::Launchd => dirs::home_dir()
                .unwrap_or_default()
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCHD_LABEL}.plist")),
            Self::Systemd => std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
                .join("systemd/user")
                .join(SYSTEMD_UNIT),
        }
    }

    /// The file registering the daemon, as this home's manifest names it.
    pub fn unit_file(self, manifest: &BTreeMap<String, String>) -> PathBuf {
        manifest
            .get(self.manifest_key())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_unit_file())
    }

    /// The read-only question "are you holding this service up?", as a
    /// command: a launchd job it has loaded, a systemd unit it has running.
    pub fn status_probe(self) -> Vec<&'static str> {
        match self {
            Self::Launchd => vec!["launchctl", "list", LAUNCHD_LABEL],
            Self::Systemd => vec!["systemctl", "--user", "is-active", SYSTEMD_UNIT],
        }
    }

    /// That question, asked.
    pub async fn up(self) -> bool {
        let probe = self.status_probe();
        probe::probe_status(probe[0], &probe[1..]).await
    }
}

/// A service manager holding the daemon of one home.
#[derive(Debug, Clone)]
pub struct Service {
    pub manager: Manager,
    /// The plist or unit file that registers it.
    pub unit_file: PathBuf,
    /// What [`Manager::status_probe`] answered: a launchd job this domain has
    /// loaded, a systemd unit that is running.
    pub up: bool,
    /// The user whose domain the service lives in — launchd addresses it as
    /// `gui/<uid>/<label>`.
    uid: u32,
}

impl Service {
    /// A service from answers rather than from this host, which is what a test
    /// hands it.
    pub fn new(manager: Manager, unit_file: PathBuf, up: bool, uid: u32) -> Self {
        Self {
            manager,
            unit_file,
            up,
            uid,
        }
    }

    /// The service managing the daemon of `home`, when one does.
    ///
    /// The gate is that home's `install.env`: it is the only thing that ties a
    /// service to a *home*, and without it a throwaway home under
    /// `ARIADNE_HOME` would find the plist of the real installation and stop
    /// the daemon nobody asked about. A hand-registered service therefore goes
    /// unnoticed here — `ariadne doctor` is where that is reported.
    pub async fn detect(home: &Path) -> Option<Self> {
        let manager = Manager::of_this_host()?;
        let unit_file = manager.unit_file(&manifest(home)?);
        if !unit_file.is_file() {
            return None;
        }
        Some(Self::new(
            manager,
            unit_file,
            manager.up().await,
            rustix::process::getuid().as_raw(),
        ))
    }

    /// "launchd (dev.ariadne.daemon)" — the manager and what it calls this.
    pub fn describe(&self) -> String {
        format!("{} ({})", self.manager.as_str(), self.manager.unit())
    }

    /// The command that asks this manager for `action`, or `None` when it has
    /// nothing to do for it — a booted-out launchd job has no process to stop,
    /// so the caller falls back to the pidfile.
    ///
    /// `kickstart -k` starts a job and kills whatever it finds running first,
    /// which is what both starting and restarting want: `daemon start` only
    /// reaches this once the daemon has failed to answer, and a job that is up
    /// but deaf is exactly what should not survive it. `bootout` is the
    /// inverse of the `bootstrap` the installer ran, and `bootstrap` is how a
    /// job that was booted out comes back.
    pub fn command(&self, action: Action) -> Option<Vec<String>> {
        let domain = format!("gui/{}", self.uid);
        let target = format!("{domain}/{}", LAUNCHD_LABEL);
        let argv = match (self.manager, action, self.up) {
            (Manager::Launchd, Action::Stop, true) => vec!["launchctl", "bootout", &target],
            (Manager::Launchd, Action::Stop, false) => return None,
            (Manager::Launchd, _, true) => vec!["launchctl", "kickstart", "-k", &target],
            (Manager::Launchd, _, false) => {
                let unit_file = self.unit_file.to_string_lossy().into_owned();
                return Some(
                    ["launchctl", "bootstrap", &domain, &unit_file]
                        .map(str::to_owned)
                        .into(),
                );
            }
            (Manager::Systemd, action, _) => {
                let verb = match action {
                    Action::Start => "start",
                    Action::Stop => "stop",
                    Action::Restart => "restart",
                };
                vec!["systemctl", "--user", verb, SYSTEMD_UNIT]
            }
        };
        Some(argv.into_iter().map(str::to_owned).collect())
    }
}

/// `install.env` as `scripts/install.sh` writes it: `KEY="value"` lines and
/// comments, read as data — nothing is executed. `None` when the home has no
/// manifest at all, which is what says nothing was installed into it.
pub fn manifest(home: &Path) -> Option<BTreeMap<String, String>> {
    let raw = std::fs::read_to_string(home.join("install.env")).ok()?;
    Some(
        raw.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
            .filter(|(_, v)| !v.is_empty())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launchd(loaded: bool) -> Service {
        Service::new(
            Manager::Launchd,
            PathBuf::from("/Users/me/Library/LaunchAgents/dev.ariadne.daemon.plist"),
            loaded,
            501,
        )
    }

    fn systemd() -> Service {
        Service::new(
            Manager::Systemd,
            PathBuf::from("/home/me/.config/systemd/user/ariadned.service"),
            true,
            1000,
        )
    }

    /// A loaded launchd job is kickstarted and booted out — never signalled:
    /// `KeepAlive.SuccessfulExit=false` means a clean SIGTERM leaves it down
    /// with the service still loaded.
    #[test]
    fn a_loaded_launchd_job_is_driven_with_launchctl() {
        let service = launchd(true);
        assert_eq!(
            service.command(Action::Start).unwrap(),
            ["launchctl", "kickstart", "-k", "gui/501/dev.ariadne.daemon"]
        );
        assert_eq!(
            service.command(Action::Restart).unwrap(),
            ["launchctl", "kickstart", "-k", "gui/501/dev.ariadne.daemon"]
        );
        assert_eq!(
            service.command(Action::Stop).unwrap(),
            ["launchctl", "bootout", "gui/501/dev.ariadne.daemon"]
        );
    }

    /// A plist launchd is not holding: starting it is the `bootstrap` the
    /// installer ran, and stopping it is nothing at all — there is no job to
    /// boot out, so the caller signals the pidfile instead.
    #[test]
    fn a_booted_out_launchd_job_is_bootstrapped_back() {
        let service = launchd(false);
        assert_eq!(
            service.command(Action::Start).unwrap(),
            [
                "launchctl",
                "bootstrap",
                "gui/501",
                "/Users/me/Library/LaunchAgents/dev.ariadne.daemon.plist"
            ]
        );
        assert_eq!(service.command(Action::Stop), None);
    }

    /// systemd takes the same three verbs whether or not the unit is running:
    /// the unit file is there, and that is all `systemctl --user` needs.
    #[test]
    fn systemd_takes_its_own_three_verbs() {
        for (action, verb) in [
            (Action::Start, "start"),
            (Action::Stop, "stop"),
            (Action::Restart, "restart"),
        ] {
            assert_eq!(
                systemd().command(action).unwrap(),
                ["systemctl", "--user", verb, "ariadned.service"]
            );
        }
        let inactive = Service::new(Manager::Systemd, systemd().unit_file, false, 1000);
        assert_eq!(
            inactive.command(Action::Stop).unwrap(),
            ["systemctl", "--user", "stop", "ariadned.service"]
        );
    }

    /// The manifest is data, not a script: `KEY="value"` lines, comments and
    /// blank lines, and nothing is executed to read them. A home without one
    /// has no manifest at all, which is a different answer from an empty one.
    #[test]
    fn the_install_manifest_is_read_as_key_values() {
        let dir = tempfile::tempdir().unwrap();
        assert!(manifest(dir.path()).is_none());
        std::fs::write(
            dir.path().join("install.env"),
            "# Written by scripts/install.sh\nARIADNE_PREFIX=\"/opt/bin\"\nARIADNE_APP=\"\"\n",
        )
        .unwrap();
        let manifest = manifest(dir.path()).unwrap();
        assert_eq!(manifest.get("ARIADNE_PREFIX").unwrap(), "/opt/bin");
        // An empty value names nothing and would only produce a bad path.
        assert!(!manifest.contains_key("ARIADNE_APP"));
    }

    /// Which file registers the service is the manifest's to say; the default
    /// is only what the installer would have written.
    #[test]
    fn the_manifest_names_the_file_that_registers_the_service() {
        let manifest = BTreeMap::from([(
            "ARIADNE_PLIST".to_string(),
            "/somewhere/else.plist".to_string(),
        )]);
        assert_eq!(
            Manager::Launchd.unit_file(&manifest),
            PathBuf::from("/somewhere/else.plist")
        );
        assert_eq!(
            Manager::Launchd.unit_file(&BTreeMap::new()),
            Manager::Launchd.default_unit_file(),
        );
        assert!(
            Manager::Systemd
                .unit_file(&BTreeMap::new())
                .ends_with("systemd/user/ariadned.service")
        );
    }

    /// A home nothing was installed into is managed by nobody, whatever this
    /// host has registered for the real one: a throwaway `ARIADNE_HOME` must
    /// never reach the service of `~/.ariadne`.
    #[tokio::test]
    async fn a_home_without_a_manifest_has_no_service() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Service::detect(dir.path()).await.is_none());
    }
}
