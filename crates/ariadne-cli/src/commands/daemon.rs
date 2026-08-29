//! `ariadne daemon ...` — the daemon of one home: start it, stop it, restart
//! it, and say how it is being managed.
//!
//! Every one of them is about a *home* rather than an endpoint: the process,
//! its pidfile and its socket all live in one, so `--home` on the group is
//! what points them at a daemon, and `ariadned` is never told anything else.
//! Where a service manager holds that home's daemon it is asked instead of the
//! process being spawned or signalled — see [`service`].

pub mod service;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;

use ariadne_client::{Client, endpoint};

use self::service::{Action, Service};
use super::{ariadne_home, find_ariadned};
use crate::output::{Format, print};

/// How long `daemon start` waits for a daemon it just launched to answer.
const READY_TIMEOUT: Duration = Duration::from_secs(5);

/// How often a wait for a socket — appearing or disappearing — looks again.
const POLL: Duration = Duration::from_millis(100);

/// What a start or a stop did, in the words its line is written from.
struct Done {
    /// The service command that did it, when a manager did.
    service: Option<Vec<String>>,
    /// The process this shell spawned or signalled, when there was no manager.
    pid: Option<String>,
}

impl Done {
    fn by(service: &[String]) -> Self {
        Self {
            service: Some(service.to_vec()),
            pid: None,
        }
    }

    fn by_process(pid: impl ToString) -> Self {
        Self {
            service: None,
            pid: Some(pid.to_string()),
        }
    }
}

/// `ariadne daemon start` — bring up the daemon of `home` and wait for it.
///
/// The client is built for `home` rather than taken from the caller:
/// `--endpoint` / `ARIADNE_ENDPOINT` — never passed to ariadned — would send
/// both the already-running check and the readiness poll at a different daemon.
pub async fn start(home: Option<PathBuf>, format: Format) -> Result<()> {
    let client = Client::for_home(home.clone());
    if client.health().await.is_ok() {
        let payload = json!({"started": false, "endpoint": client.endpoint()});
        return print(format, &payload, || {
            println!("daemon already running at {}", client.endpoint())
        });
    }

    let root = ariadne_home(home.clone());
    let done = launch(&root, home).await?;
    ready(&client, &done).await?;
    let payload = json!({
        "started": true,
        "pid": done.pid,
        "service": done.service,
        "endpoint": client.endpoint(),
    });
    print(format, &payload, || println!("{}", started_line(&done)))
}

/// `ariadne daemon stop` — stop the daemon of `home` and wait for its socket
/// to go, so the command is over when the daemon is.
pub async fn stop(home: Option<PathBuf>, timeout: u64, format: Format) -> Result<()> {
    let root = ariadne_home(home);
    let socket = endpoint::socket_path(&root);
    let (done, waited) = halt(&root, &socket, timeout).await?;
    let payload = json!({
        "stopped": true,
        "pid": done.pid,
        "service": done.service,
        "socket": socket.display().to_string(),
        "waited_secs": waited.as_secs_f64(),
    });
    print(format, &payload, || {
        println!("{}", stopped_line(&done));
        println!("{}", gone_line(&socket, waited));
    })
}

/// `ariadne daemon restart` — the service's own restart where one manages this
/// home, and a stop followed by a start where none does.
pub async fn restart(home: Option<PathBuf>, timeout: u64, format: Format) -> Result<()> {
    let root = ariadne_home(home.clone());
    let client = Client::for_home(home.clone());
    let socket = endpoint::socket_path(&root);

    let mut stopped = None;
    let done = match Service::detect(&root)
        .await
        .and_then(|s| s.command(Action::Restart))
    {
        // One command takes the daemon down and brings it back, which is the
        // manager's own answer to a restart: nothing in between for the socket
        // to be raced over.
        Some(argv) => {
            run(&argv)?;
            Done::by(&argv)
        }
        // No manager: the daemon has to be gone before the socket is free for
        // the one that replaces it — and a daemon that is not running at all
        // is simply started.
        None => {
            if client.health().await.is_ok() {
                stopped = Some(halt(&root, &socket, timeout).await?);
            }
            launch(&root, home).await?
        }
    };

    ready(&client, &done).await?;
    let payload = json!({
        "restarted": true,
        "pid": done.pid,
        "service": done.service,
        "endpoint": client.endpoint(),
    });
    print(format, &payload, || {
        if let Some((done, waited)) = &stopped {
            println!("{}", stopped_line(done));
            println!("{}", gone_line(&socket, *waited));
        }
        println!("{}", started_line(&done));
    })
}

/// `ariadne daemon status` — what the daemon answers, and who is holding it up.
///
/// Read-only in every part: the health call, and the one question the service
/// manager is asked about the service this home was installed with.
pub async fn status(client: &Client, home: Option<PathBuf>, format: Format) -> Result<()> {
    let h = client.health().await?;
    let service = Service::detect(&ariadne_home(home)).await;
    let payload = json!({
        "status": h.status,
        "uptime_secs": h.uptime_secs,
        "endpoint": client.endpoint(),
        "service": service.as_ref().map(|s| json!({
            "manager": s.manager.as_str(),
            "unit": s.manager.unit(),
            "unit_file": s.unit_file.display().to_string(),
            "up": s.up,
        })),
    });
    print(format, &payload, || {
        println!("status:  {}", h.status);
        println!("uptime:  {}s", h.uptime_secs);
        println!("socket:  {}", client.endpoint());
        println!("service: {}", management(service.as_ref()));
    })
}

/// `ariadne daemon logs [-f]` — show the log of this home's daemon via tail.
pub fn logs(home: Option<PathBuf>, follow: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let log = log_file(&ariadne_home(home));
    if !log.is_file() {
        bail!("no daemon log at {}", log.display());
    }
    let mut cmd = Command::new("tail");
    cmd.arg("-n").arg("200");
    if follow {
        cmd.arg("-f");
    }
    let err = cmd.arg(&log).exec();
    bail!("failed to exec tail: {err}");
}

// ---- doing it ----------------------------------------------------------

/// Bring the daemon of `root` up: the service manager's command where one
/// holds this home, else `ariadned` spawned detached with its output appended
/// to the log `daemon logs` reads.
async fn launch(root: &Path, home: Option<PathBuf>) -> Result<Done> {
    if let Some(argv) = Service::detect(root)
        .await
        .and_then(|s| s.command(Action::Start))
    {
        run(&argv)?;
        return Ok(Done::by(&argv));
    }

    let binary = find_ariadned()?;
    let mut cmd = Command::new(&binary);
    if let Some(home) = &home {
        cmd.arg("--home").arg(home);
    }
    std::fs::create_dir_all(root).ok();
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file(root))
        .with_context(|| format!("opening log file in {}", root.display()))?;
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone().context("cloning log handle")?)
        .stderr(log);
    let child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", binary.display()))?;
    Ok(Done::by_process(child.id()))
}

/// Take the daemon of `root` down and wait for its socket to go with it.
///
/// The socket is the end of a daemon a caller can see from outside it: the
/// process removes it as it stops, and while it is there the next daemon
/// cannot bind. Waiting on the pid instead would say nothing about that.
async fn halt(root: &Path, socket: &Path, timeout: u64) -> Result<(Done, Duration)> {
    let done = match Service::detect(root)
        .await
        .and_then(|s| s.command(Action::Stop))
    {
        Some(argv) => {
            run(&argv)?;
            Done::by(&argv)
        }
        None => Done::by_process(signal_pidfile(root)?),
    };

    let started = Instant::now();
    let timeout = Duration::from_secs(timeout);
    if !wait_for(timeout, || async { !socket.exists() }).await {
        bail!(
            "ariadned is still on {} {}s after {}",
            socket.display(),
            timeout.as_secs(),
            how(&done)
        );
    }
    Ok((done, started.elapsed()))
}

/// Wait for a daemon that was just brought up to answer.
async fn ready(client: &Client, done: &Done) -> Result<()> {
    if wait_for(READY_TIMEOUT, || async { client.health().await.is_ok() }).await {
        return Ok(());
    }
    bail!(
        "ariadned did not answer on {} within {}s ({})",
        client.endpoint(),
        READY_TIMEOUT.as_secs(),
        how(done)
    )
}

/// SIGTERM to whatever the pidfile of `root` names, and the pid it named.
fn signal_pidfile(root: &Path) -> Result<String> {
    let pid_file = endpoint::pid_file(root);
    let pid = std::fs::read_to_string(&pid_file)
        .with_context(|| {
            format!(
                "no pidfile at {} — is the daemon running?",
                pid_file.display()
            )
        })?
        .trim()
        .to_string();

    let status = Command::new("kill")
        .arg(&pid)
        .status()
        .context("running kill")?;
    if !status.success() {
        bail!(
            "kill {pid} failed — stale pidfile? ({})",
            pid_file.display()
        );
    }
    Ok(pid)
}

/// Run a service manager's command, letting it speak for itself: whatever
/// `launchctl` or `systemctl` has to say about a refusal is better than
/// anything this could paraphrase.
fn run(argv: &[String]) -> Result<()> {
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .with_context(|| format!("running {}", argv.join(" ")))?;
    if !status.success() {
        bail!("{} failed ({status})", argv.join(" "));
    }
    Ok(())
}

/// Poll `question` until it answers yes, or until `timeout` runs out.
async fn wait_for<F, Fut>(timeout: Duration, question: F) -> bool
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if question().await {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The log every daemon of a home appends to, whoever started it: the service
/// files `scripts/install.sh` writes name this same path.
fn log_file(root: &Path) -> PathBuf {
    root.join("ariadned.log")
}

// ---- wording -----------------------------------------------------------

/// What did it, for a sentence that has already said what was done.
fn how(done: &Done) -> String {
    match (&done.service, &done.pid) {
        (Some(argv), _) => argv.join(" "),
        (None, Some(pid)) => format!("pid {pid}"),
        (None, None) => "nothing".to_string(),
    }
}

fn started_line(done: &Done) -> String {
    match &done.service {
        Some(argv) => format!("ariadned started with: {}", argv.join(" ")),
        None => format!("ariadned started ({})", how(done)),
    }
}

fn stopped_line(done: &Done) -> String {
    match &done.service {
        Some(argv) => format!("ariadned stopped with: {}", argv.join(" ")),
        None => format!("sent SIGTERM to ariadned ({})", how(done)),
    }
}

fn gone_line(socket: &Path, waited: Duration) -> String {
    format!(
        "socket {} gone after {:.1}s",
        socket.display(),
        waited.as_secs_f64()
    )
}

/// Who is holding this home's daemon up, as `daemon status` says it.
fn management(service: Option<&Service>) -> String {
    match service {
        Some(s) if s.up => format!("managed by {}", s.describe()),
        // The service is installed for this home and the manager is not
        // holding it up: whatever is answering was started some other way.
        Some(s) => format!(
            "managed by {}, not {} — this daemon was started outside it",
            s.describe(),
            s.manager.up_word()
        ),
        None => "no service manages this home — a daemon started by hand".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::service::Manager;
    use super::*;

    fn launchd(up: bool) -> Service {
        Service::new(
            Manager::Launchd,
            PathBuf::from("/Users/me/Library/LaunchAgents/dev.ariadne.daemon.plist"),
            up,
            501,
        )
    }

    /// `daemon status` names the manager and what it calls the service, so the
    /// answer to "why did my daemon come back on its own" is in the status.
    #[test]
    fn status_says_which_service_manages_the_daemon() {
        assert_eq!(
            management(Some(&launchd(true))),
            "managed by launchd (dev.ariadne.daemon)"
        );
        assert_eq!(
            management(None),
            "no service manages this home — a daemon started by hand"
        );
        let detail = management(Some(&launchd(false)));
        assert!(
            detail.starts_with("managed by launchd (dev.ariadne.daemon), not loaded"),
            "{detail}"
        );
    }

    /// Every line says how it was done: the command the manager was asked
    /// with, or the process this shell spawned or signalled itself.
    #[test]
    fn every_line_says_what_did_it() {
        let by_service = Done::by(&["launchctl".into(), "bootout".into(), "gui/501/x".into()]);
        assert_eq!(
            stopped_line(&by_service),
            "ariadned stopped with: launchctl bootout gui/501/x"
        );
        let by_process = Done::by_process(1234);
        assert_eq!(
            stopped_line(&by_process),
            "sent SIGTERM to ariadned (pid 1234)"
        );
        assert_eq!(started_line(&by_process), "ariadned started (pid 1234)");
        assert_eq!(
            gone_line(Path::new("/tmp/a.sock"), Duration::from_millis(420)),
            "socket /tmp/a.sock gone after 0.4s"
        );
    }

    /// The wait asks again until its question turns true, and gives up when
    /// the timeout runs out rather than hanging on a daemon that will not go.
    #[tokio::test]
    async fn a_wait_ends_on_the_answer_or_on_the_timeout() {
        assert!(wait_for(Duration::ZERO, || async { true }).await);
        assert!(
            !wait_for(Duration::ZERO, || async { false }).await,
            "a timeout that has already run out is one look and no wait"
        );

        let asked = std::cell::Cell::new(0);
        assert!(
            wait_for(Duration::from_secs(5), || {
                asked.set(asked.get() + 1);
                async { asked.get() > 1 }
            })
            .await
        );
        assert_eq!(asked.get(), 2, "and the answer was not the first one");
    }
}
