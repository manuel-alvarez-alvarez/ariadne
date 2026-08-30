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

use ariadne_api::logs::{LogLineDto, LogSnapshotResponse, LogsQuery};
use ariadne_client::{Client, endpoint};

use self::service::{Action, Service};
use super::follow::{self, Next};
use super::{ariadne_home, find_ariadned, query_path};
use crate::output::{Format, Kv, duration, local_time, note, pager, print, print_kv, style, view};

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
            println!(
                "{} {}",
                style::paint(view().color, style::META, "daemon already running at"),
                client.endpoint()
            )
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
    let root = ariadne_home(home);
    let service = Service::detect(&root).await;
    let version = client.version().await.ok();
    let (pid, tcp) = local(client, &root);
    let payload = json!({
        "status": h.status,
        "uptime_secs": h.uptime_secs,
        "uptime": duration(h.uptime_secs),
        "version": version.as_ref().map(|v| v.version.clone()),
        "pid": pid,
        "endpoint": client.endpoint(),
        "tcp_listen": tcp,
        "service": service.as_ref().map(|s| json!({
            "manager": s.manager.as_str(),
            "unit": s.manager.unit(),
            "unit_file": s.unit_file.display().to_string(),
            "up": s.up,
        })),
    });
    print(format, &payload, || {
        print_kv(&[
            // A daemon that answered says `ok`: a verdict rather than a
            // lifecycle status, and it reads here in the green `✓` `ariadne
            // doctor` gives the same word.
            ("status", Kv::check(&h.status)),
            (
                "version",
                version
                    .map_or("-".into(), |v| format!("{} {}", v.name, v.version))
                    .into(),
            ),
            // Seconds are a number to divide; `2d 3h` is the answer one was
            // after. The exact count stays in `--format json`.
            ("uptime", duration(h.uptime_secs).into()),
            ("pid", pid.map_or("-".into(), |pid| pid.to_string()).into()),
            ("socket", client.endpoint().into()),
            ("tcp", tcp.unwrap_or_else(|| "disabled".into()).into()),
            ("service", management(service.as_ref()).into()),
        ]);
    })
}

/// What only the home on this machine can say — the pid its daemon wrote and
/// whether it also listens on TCP — for the daemon `status` actually reached.
///
/// Both are read from `root`, and only when `root`'s socket is the endpoint we
/// talked to: `--endpoint` wins over `--home` here, and a daemon somewhere
/// else is another process entirely — reporting this home's pid for it would
/// be a lie a reader has no way to catch.
fn local(client: &Client, root: &Path) -> (Option<u32>, Option<String>) {
    if endpoint::socket_path(root) != Path::new(client.endpoint()) {
        return (None, None);
    }
    let pid = std::fs::read_to_string(endpoint::pid_file(root))
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok());
    let tcp = endpoint::parse_config(root)
        .ok()
        .flatten()
        .and_then(|config| config.tcp_listen)
        .map(|addr| addr.to_string());
    (pid, tcp)
}

/// How many lines of the daemon's buffer `daemon logs` shows, and how many of
/// the file the fallback tails.
const LOG_TAIL: usize = 200;

/// `ariadne daemon logs [-f]` — the daemon's own log, from the daemon.
///
/// Asked of the API rather than of a file, because under a service manager
/// there is usually no file: `ariadned`'s stdout is the journal's, and the
/// only place its log is reliably readable is the ring buffer it keeps for
/// `/v1/logs`. It also means `--endpoint` reaches the daemon it names, which
/// a path under a home on this machine never could.
///
/// The file is the fallback, for the one case the API cannot serve: a daemon
/// that is not answering, which is exactly when its last lines are wanted.
pub async fn logs(client: &Client, home: Option<PathBuf>, follow: bool) -> Result<()> {
    let served = match follow {
        true => follow_log(client).await,
        false => print_log(client).await,
    };
    match served {
        Err(e) if follow::unreachable(&e) => {
            note(&format!(
                "cannot reach the ariadne daemon at {} — reading the log file instead",
                client.endpoint()
            ));
            tail_log_file(home, follow)
        }
        other => other,
    }
}

/// The daemon's buffer as it stands.
///
/// Through the pager, like every other log snapshot: two hundred lines is
/// several screens, and there is an end to page to. A follow has none and
/// writes straight out.
async fn print_log(client: &Client) -> Result<()> {
    let path = query_path(
        "/v1/logs",
        &LogsQuery {
            tail: Some(LOG_TAIL),
        },
    )?;
    let snapshot: LogSnapshotResponse = client.get_json(&path).await?;
    let color = view().color;
    let lines: Vec<String> = snapshot.lines.iter().map(|l| log_line(l, color)).collect();
    pager::page(&format!("{}\n", lines.join("\n")))
}

/// The same, and then every line the daemon writes from here on.
///
/// The buffer arrives as the stream's own opening `snapshot`, so there is no
/// window between what was printed and what is followed. Every connection
/// opens with one, reconnects included, and what a reconnect's carries is
/// mostly lines this tail has already shown — so it is printed from where
/// [`Shown`] says the two stop overlapping.
async fn follow_log(client: &Client) -> Result<()> {
    let mut shown = Shown::default();
    let color = view().color;
    follow::frames_reconnecting(client, "/v1/logs/stream", move |frame| {
        match frame.event.as_str() {
            "snapshot" => {
                let snapshot: LogSnapshotResponse = serde_json::from_str(&frame.data)?;
                // The first connection wants the recent past rather than the
                // whole buffer; a later one's unseen lines are the gap it
                // opened, and all of them are wanted.
                let from = match shown.is_empty() {
                    true => snapshot.lines.len().saturating_sub(LOG_TAIL),
                    false => shown.boundary(&snapshot.lines),
                };
                for line in &snapshot.lines[from..] {
                    println!("{}", log_line(line, color));
                    shown.note(line);
                }
            }
            "delta" => {
                let line: LogLineDto = serde_json::from_str(&frame.data)?;
                println!("{}", log_line(&line, color));
                shown.note(&line);
            }
            _ => {}
        }
        Ok(Next::Go)
    })
    .await
}

/// How many shown lines are kept to line a reconnect's snapshot up against.
///
/// Only ever read on a reconnect, and the longest run that lines up is what
/// decides, so this is the length of the run that has to be ambiguous before
/// the boundary can land on the wrong copy of a repeated line — far more of
/// the daemon's own log than ever repeats.
const OVERLAP: usize = 32;

/// The tail of what a daemon-log follow has already shown, so that a
/// reconnect's snapshot can be printed from where the last one left off.
///
/// The overlap is found by matching whole records rather than by comparing
/// timestamps. A timestamp is not an identity: `tracing` stamps two events in
/// the same microsecond often enough, and "newer than the last one shown"
/// silently drops the second of them — a live `delta` sharing its stamp with
/// the snapshot line before it included.
#[derive(Default)]
struct Shown(std::collections::VecDeque<LogLineDto>);

impl Shown {
    /// Where in `snapshot` the lines this tail has not shown begin.
    ///
    /// The longest run of what was shown that lines up inside the snapshot
    /// wins, a longer match being a surer boundary. Nothing lining up at all
    /// — a daemon that restarted, or a gap longer than its buffer — means the
    /// two do not overlap, and every line of it is new.
    fn boundary(&self, snapshot: &[LogLineDto]) -> usize {
        let shown: Vec<&LogLineDto> = self.0.iter().collect();
        for len in (1..=shown.len().min(snapshot.len())).rev() {
            let tail = &shown[shown.len() - len..];
            let lines_up = |run: &[LogLineDto]| run.iter().zip(tail).all(|(a, b)| same(a, b));
            if let Some(at) = snapshot.windows(len).rposition(lines_up) {
                return at + len;
            }
        }
        0
    }

    /// Remember one line as shown, forgetting the oldest beyond [`OVERLAP`].
    fn note(&mut self, line: &LogLineDto) {
        if self.0.len() == OVERLAP {
            self.0.pop_front();
        }
        self.0.push_back(line.clone());
    }

    /// Whether anything has been shown yet — which is what tells the first
    /// connection from a reconnect.
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Whether two captured lines are the same record. `LogLineDto` carries no id
/// of its own, so all of it together is the closest thing to one.
fn same(a: &LogLineDto, b: &LogLineDto) -> bool {
    a.ts == b.ts && a.level == b.level && a.target == b.target && a.message == b.message
}

/// One captured line as a terminal reads it: when, how loud, from where, and
/// what it said.
///
/// `time` and `target` are context and dim to `META`; `message` is the line
/// itself and stays as plain as it always was; `level` carries
/// `style::level`'s colour, on the same five characters `{:<5}` has always
/// padded it to — padded first, so the escapes around it never count toward
/// the column's width.
fn log_line(line: &LogLineDto, color: bool) -> String {
    let level = format!("{:<5}", line.level);
    format!(
        "{}  {}  {}  {}",
        style::paint(color, style::META, &local_time(&line.ts)),
        style::paint(color, style::level(&line.level), &level),
        style::paint(color, style::META, &line.target),
        line.message
    )
}

/// The fallback: the file every daemon of this home appends to, via tail.
/// Only reachable when the daemon is not answering, so there is nothing left
/// to keep this process around for — `tail` takes it over.
fn tail_log_file(home: Option<PathBuf>, follow: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let log = log_file(&ariadne_home(home));
    if !log.is_file() {
        bail!(
            "no daemon log at {} either — a daemon started by launchd or systemd \
             writes to the system log instead",
            log.display()
        );
    }
    let mut cmd = Command::new("tail");
    cmd.arg("-n").arg(LOG_TAIL.to_string());
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
    let verb = style::paint(view().color, style::OK, "ariadned started");
    match &done.service {
        Some(argv) => format!("{verb} with: {}", argv.join(" ")),
        None => format!("{verb} ({})", how(done)),
    }
}

fn stopped_line(done: &Done) -> String {
    let color = view().color;
    match &done.service {
        Some(argv) => format!(
            "{} with: {}",
            style::paint(color, style::OK, "ariadned stopped"),
            argv.join(" ")
        ),
        None => format!(
            "{} ({})",
            style::paint(color, style::OK, "sent SIGTERM to ariadned"),
            how(done)
        ),
    }
}

fn gone_line(socket: &Path, waited: Duration) -> String {
    let color = view().color;
    format!(
        "{} {} {}",
        style::paint(color, style::META, "socket"),
        socket.display(),
        style::paint(
            color,
            style::META,
            &format!("gone after {:.1}s", waited.as_secs_f64())
        )
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
    /// One captured daemon-log line, stamped and worded as the caller says.
    fn log(ts: &str, message: &str) -> LogLineDto {
        LogLineDto {
            ts: ts.into(),
            level: "INFO".into(),
            target: "ariadned".into(),
            message: message.into(),
        }
    }

    /// What a tail has shown, in order.
    fn shown(lines: &[LogLineDto]) -> Shown {
        let mut shown = Shown::default();
        for line in lines {
            shown.note(line);
        }
        shown
    }

    /// The bug the overlap replaced: two records stamped in the same tick are
    /// two records, and "newer than the last timestamp shown" dropped the
    /// second of them for good — a live `delta` sharing the stamp of the
    /// snapshot line before it included.
    #[test]
    fn two_records_stamped_in_the_same_tick_are_both_shown() {
        let tick = "2026-08-29T02:00:00.000001Z";
        let (first, second) = (log(tick, "listening"), log(tick, "spawning planner"));
        let shown = shown(std::slice::from_ref(&first));
        assert_eq!(
            shown.boundary(&[first, second]),
            1,
            "only the record already shown is behind the boundary"
        );
    }

    /// A reconnect's snapshot is mostly what has already been printed, and
    /// what is past the overlap is the gap the disconnection opened.
    #[test]
    fn a_reconnect_shows_only_what_it_missed() {
        let buffer: Vec<LogLineDto> = (0..6)
            .map(|i| log(&format!("2026-08-29T02:00:0{i}Z"), &format!("line {i}")))
            .collect();
        let shown = shown(&buffer[..4]);
        assert_eq!(shown.boundary(&buffer), 4);
        assert_eq!(shown.boundary(&buffer[2..]), 2, "a buffer that has rolled");
    }

    /// Nothing lining up at all — a daemon that restarted, or a gap longer
    /// than its buffer — is a snapshot that is new from end to end.
    #[test]
    fn a_snapshot_that_overlaps_nothing_is_all_new() {
        let shown = shown(&[log("2026-08-29T02:00:00Z", "before the restart")]);
        let fresh = [
            log("2026-08-29T02:05:00Z", "starting ariadned 0.4.0"),
            log("2026-08-29T02:05:01Z", "listening"),
        ];
        assert_eq!(shown.boundary(&fresh), 0);
        assert_eq!(
            Shown::default().boundary(&fresh),
            0,
            "and so is a first one"
        );
    }

    /// The longest run that lines up decides, so a line the daemon repeats
    /// does not put the boundary on the wrong copy of itself.
    #[test]
    fn the_longest_run_that_lines_up_decides_the_boundary() {
        let (beat, work) = (
            log("2026-08-29T02:00:00Z", "sweeping sessions"),
            log("2026-08-29T02:00:01Z", "spawning planner"),
        );
        let buffer = [
            beat.clone(),
            work.clone(),
            beat.clone(),
            work.clone(),
            log("2026-08-29T02:00:02Z", "task merged"),
        ];
        // The tail shown ends on the *second* pair, and matching the whole run
        // of it is what says so — the last line alone appears twice.
        assert_eq!(shown(&buffer[..4]).boundary(&buffer), 4);
        assert_eq!(shown(&[beat, work]).boundary(&buffer), 4);
    }

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

    /// With colour off, a log line is exactly what it always was: no
    /// escapes, the level padded to five characters. With colour on, `ERROR`
    /// carries `style::level`'s colour for it and `DEBUG` carries its dimmed
    /// one — the two ends of that range — and the level column is still five
    /// wide either way, since it is padded before it is painted.
    #[test]
    fn a_log_line_is_plain_off_and_painted_on() {
        let error = LogLineDto {
            level: "ERROR".into(),
            ..log("2026-08-29T02:00:00Z", "planner crashed")
        };
        let plain = log_line(&error, false);
        assert_eq!(
            plain,
            format!(
                "{}  ERROR  ariadned  planner crashed",
                local_time("2026-08-29T02:00:00Z")
            )
        );
        assert!(!plain.contains('\u{1b}'), "{plain}");

        let painted = log_line(&error, true);
        assert!(
            painted.contains(&style::paint(true, style::level("ERROR"), "ERROR")),
            "{painted}"
        );
        assert!(
            painted.contains(&style::paint(true, style::META, "ariadned")),
            "{painted}"
        );
        assert!(painted.ends_with("planner crashed"), "{painted}");

        let debug = LogLineDto {
            level: "DEBUG".into(),
            ..log("2026-08-29T02:00:00Z", "polling socket")
        };
        assert!(log_line(&debug, true).contains(&style::paint(
            true,
            style::level("DEBUG"),
            "DEBUG"
        )));
        assert!(!log_line(&debug, false).contains('\u{1b}'));
    }
}
