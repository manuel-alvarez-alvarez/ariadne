//! Host selection uses the registry of the locally resolved daemon.
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_client::Client;
use serde_json::Value;

use crate::cli::{Cli, Command, DaemonCommand};
use crate::commands;
use crate::commands::forge::ForgeCommand;
use crate::commands::permissions::{AiPermissionsCommand, PermissionsCommand};
use crate::commands::repo;
use crate::commands::session::SessionCommand;
use crate::error::{self, Exit, Failure};
use crate::output::listing::{self, Listing};
use crate::output::{self, Format, UNCAPPED, col};

pub(crate) async fn list(client: &Client, format: Format) -> Result<()> {
    let hosts: Vec<HostDto> = client.get_json("/v1/hosts").await?;
    print_hosts(&hosts, format)
}

fn print_hosts(hosts: &[HostDto], format: Format) -> Result<()> {
    output::print_list(
        format,
        hosts,
        &[
            col("name", UNCAPPED),
            col("kind", UNCAPPED),
            col("status", UNCAPPED),
            col("endpoint", UNCAPPED),
            col("remote", UNCAPPED),
            col("sync", UNCAPPED),
            col("missing", UNCAPPED),
            col("skipped", UNCAPPED),
            col("error", UNCAPPED),
            col("sync_error", UNCAPPED),
        ],
        |host| {
            let value = serde_json::to_value(host).expect("host serializes");
            let text = |v: &serde_json::Value| v.as_str().unwrap_or("-").to_owned();
            let sync = &value["sync"];
            vec![
                text(&value["name"]),
                text(&value["kind"]),
                text(&value["status"]),
                text(&value["endpoint"]),
                text(&value["remote"]),
                text(&sync["state"]),
                host.sync.missing_repositories.len().to_string(),
                host.sync.skipped_permissions.to_string(),
                text(&value["error"]),
                text(&sync["error"]),
            ]
        },
        "",
    )
}

/// `host add`: store a host in the local daemon and print its row.
pub(crate) async fn add(
    client: &Client,
    name: String,
    endpoint: String,
    remote: Option<String>,
    color: Option<String>,
    format: Format,
) -> Result<()> {
    let body = serde_json::json!({
        "name": name, "endpoint": endpoint, "remote": remote, "color": color,
    });
    let host: HostDto = client
        .post_json("/v1/hosts", &body)
        .await
        .map_err(refused)?;
    print_host(host, format)
}

/// `host edit`: send only the given fields. `Some(None)` sends `null`.
pub(crate) async fn edit(
    client: &Client,
    name: &str,
    endpoint: Option<String>,
    remote: Option<Option<String>>,
    color: Option<Option<String>>,
    format: Format,
) -> Result<()> {
    let mut body = serde_json::Map::new();
    if let Some(endpoint) = endpoint {
        body.insert("endpoint".into(), endpoint.into());
    }
    if let Some(remote) = remote {
        body.insert("remote".into(), remote.into());
    }
    if let Some(color) = color {
        body.insert("color".into(), color.into());
    }
    if body.is_empty() {
        return Err(Failure::usage(
            "nothing to change: give --endpoint, --remote, --no-remote, --color or --no-color",
        )
        .err());
    }
    let host: HostDto = client
        .patch_json(&host_path(name), &Value::Object(body))
        .await
        .map_err(refused)?;
    print_host(host, format)
}

/// One host: its `host ls` row, or its `HostDto` as JSON.
fn print_host(host: HostDto, format: Format) -> Result<()> {
    if format == Format::Json {
        output::print_json(&host)
    } else {
        print_hosts(&[host], format)
    }
}

/// `host rm`: remove a host from the local daemon and print its name.
pub(crate) async fn remove(client: &Client, name: &str, format: Format) -> Result<()> {
    client
        .send_no_content::<()>(http::Method::DELETE, &host_path(name), None)
        .await
        .map_err(refused)?;
    if format == Format::Json {
        output::print_json(&serde_json::json!({ "name": name }))?;
    } else {
        println!("{name}");
    }
    Ok(())
}

/// A daemon refusal of a host write exits 1 whatever its status, and keeps
/// the daemon's message and envelope. A daemon that does not answer keeps its
/// own code.
fn refused(error: ariadne_client::ClientError) -> anyhow::Error {
    match error {
        ariadne_client::ClientError::Api { .. } => {
            anyhow::Error::new(error::ExitsAs::new(Exit::Failed, error))
        }
        other => other.into(),
    }
}

fn host_path(name: &str) -> String {
    format!("/v1/hosts/{}", crate::commands::path_segment(name))
}

/// `host reconnect NAME|all`: ask the local daemon to reconnect each target,
/// wait until each is `ready` or `down`, and print their rows. Any host that
/// is not `ready` at the end fails the command.
pub(crate) async fn reconnect(
    client: &Client,
    name: &str,
    timeout: u64,
    format: Format,
) -> Result<ExitCode> {
    let hosts: Vec<HostDto> = client.get_json("/v1/hosts").await?;
    let named: Vec<&str> = hosts
        .iter()
        .map(|host| host.name.as_str())
        .filter(|host| *host != "local")
        .collect();
    let targets: Vec<String> = if name == "local" {
        return Err(Failure::usage("host local is this daemon and cannot be reconnected").err());
    } else if name == "all" {
        if named.is_empty() {
            return Err(Failure::usage("no named host is configured").err());
        }
        named.iter().map(|host| host.to_string()).collect()
    } else if named.contains(&name) {
        vec![name.to_owned()]
    } else {
        let configured = if named.is_empty() {
            "none".to_owned()
        } else {
            named.join(", ")
        };
        return Err(Failure::usage(format!(
            "unknown host {name}; configured hosts: {configured}"
        ))
        .err());
    };
    for target in &targets {
        let _: HostDto = client
            .post_empty(&format!(
                "/v1/hosts/{}/reconnect",
                crate::commands::path_segment(target)
            ))
            .await?;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let settled = loop {
        let now: Vec<HostDto> = client.get_json("/v1/hosts").await?;
        let rows: Vec<HostDto> = targets
            .iter()
            .filter_map(|target| now.iter().find(|host| &host.name == target).cloned())
            .collect();
        let done = rows.len() == targets.len()
            && rows
                .iter()
                .all(|host| host.status != HostStatus::Connecting);
        if done || std::time::Instant::now() >= deadline {
            break rows;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    };
    print_hosts(&settled, format)?;
    let ready = settled.len() == targets.len()
        && settled.iter().all(|host| host.status == HostStatus::Ready);
    Ok(if ready {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// What a command does, which decides the hosts that `--host` may name for it
/// (spec 014, rules 44 to 48). Each command path declares one in [`KINDS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A list each host answers with its own rows.
    List,
    /// A read of one ID, which one host has.
    ReadOne,
    /// A read that every host answers in full.
    ReadEach,
    /// Lines that a host writes until the stream ends or is stopped.
    Stream,
    /// A write that any one ready host accepts.
    Write,
    /// A write of a host's own setting in an area that `local` syncs: a named
    /// host takes it, and refuses each flag of a synced setting.
    HostOwned,
    /// A command that runs on the local daemon only, for the reason given.
    Local(&'static str),
}

const SYNCED: &str = "these settings are written on local and synced to every host";
const SYNCED_PERMISSIONS: &str = "these settings are written on local and synced to every host; \
     only `permissions ai set --device` is per host";
const LIFECYCLE: &str = "daemon lifecycle commands run on the local daemon";
const REGISTRY_WRITE: &str = "hosts are stored in the database of the local daemon";
const REGISTRY: &str = "`host ls` reads the registry of the local daemon";

/// The kind of every command path, as `main::command_path` spells it.
const KINDS: &[(&str, Kind)] = &[
    ("agent ls", Kind::List),
    ("agent refresh", Kind::Write),
    ("agent update", Kind::Write),
    ("attach", Kind::ReadOne),
    ("attention", Kind::List),
    ("attention dismiss", Kind::Write),
    ("completions", Kind::ReadEach),
    (
        "completions install",
        Kind::Local("`completions install` writes the shell configuration of this machine"),
    ),
    ("daemon logs", Kind::Stream),
    ("daemon restart", Kind::Local(LIFECYCLE)),
    ("daemon start", Kind::Local(LIFECYCLE)),
    ("daemon status", Kind::ReadEach),
    ("daemon stop", Kind::Local(LIFECYCLE)),
    ("dashboard get", Kind::ReadOne),
    ("dashboard ls", Kind::List),
    ("dashboard reset", Kind::Write),
    ("dashboard rm", Kind::Write),
    ("dashboard set", Kind::Write),
    ("doctor", Kind::ReadEach),
    ("events", Kind::Stream),
    // `forge tunnel on|off` is a write: [`kind`] reads the argument.
    ("forge tunnel", Kind::ReadEach),
    ("goal attach", Kind::ReadOne),
    ("goal cancel", Kind::Write),
    ("goal complete", Kind::Write),
    ("goal create", Kind::Write),
    ("goal inspect", Kind::ReadOne),
    ("goal ls", Kind::List),
    ("goal rm", Kind::Write),
    ("host ls", Kind::Local(REGISTRY)),
    (
        "host setup",
        Kind::Local("worker setup runs on the local daemon"),
    ),
    (
        "host setup-status",
        Kind::Local("worker setup progress belongs to the local daemon"),
    ),
    ("host add", Kind::Local(REGISTRY_WRITE)),
    ("host edit", Kind::Local(REGISTRY_WRITE)),
    ("host rm", Kind::Local(REGISTRY_WRITE)),
    (
        "host reconnect",
        Kind::Local("`host reconnect` reads and writes the registry of the local daemon"),
    ),
    ("issue ls", Kind::List),
    (
        "mcp serve",
        Kind::Local("the MCP server serves the local daemon"),
    ),
    ("models disable", Kind::Write),
    ("models enable", Kind::Write),
    ("models ls", Kind::List),
    ("models rank", Kind::Write),
    ("models show", Kind::ReadEach),
    ("permissions ai disable", Kind::Local(SYNCED_PERMISSIONS)),
    ("permissions ai enable", Kind::Local(SYNCED_PERMISSIONS)),
    ("permissions ai refresh", Kind::Local(SYNCED_PERMISSIONS)),
    ("permissions ai set", Kind::HostOwned),
    ("permissions ai show", Kind::ReadEach),
    ("permissions ai test", Kind::ReadEach),
    ("permissions learned list", Kind::List),
    ("permissions learned rm", Kind::Local(SYNCED_PERMISSIONS)),
    ("permissions learned scope", Kind::Local(SYNCED_PERMISSIONS)),
    ("permissions learned show", Kind::ReadOne),
    ("pr babysit", Kind::Write),
    ("pr inspect", Kind::ReadOne),
    ("pr ls", Kind::List),
    ("pr refresh", Kind::Write),
    ("pr review", Kind::Write),
    ("pr search", Kind::List),
    ("query", Kind::ReadEach),
    ("repo add", Kind::HostOwned),
    ("repo inspect", Kind::ReadOne),
    ("repo ls", Kind::List),
    ("repo rm", Kind::HostOwned),
    ("repo update", Kind::HostOwned),
    ("session inspect", Kind::ReadOne),
    ("session kill", Kind::Write),
    ("session logs", Kind::Stream),
    ("session ls", Kind::List),
    ("session resume", Kind::Write),
    ("session send", Kind::Write),
    ("session switch", Kind::Write),
    ("skill create", Kind::Local(SYNCED)),
    ("skill get", Kind::ReadEach),
    ("skill inspect", Kind::ReadEach),
    ("skill ls", Kind::List),
    ("skill reset", Kind::Local(SYNCED)),
    ("skill rm", Kind::Local(SYNCED)),
    ("skill set", Kind::Local(SYNCED)),
    ("task attach", Kind::ReadOne),
    ("task cancel", Kind::Write),
    ("task create", Kind::Write),
    ("task diff", Kind::ReadOne),
    ("task history", Kind::ReadOne),
    ("task inspect", Kind::ReadOne),
    ("task logs", Kind::Stream),
    ("task ls", Kind::List),
    ("task messages", Kind::ReadOne),
    ("task retry", Kind::Write),
    ("task update", Kind::Write),
    ("version", Kind::ReadEach),
    ("workflow check", Kind::ReadEach),
    ("workflow create", Kind::Local(SYNCED)),
    ("workflow ls", Kind::List),
    ("workflow reset", Kind::Local(SYNCED)),
    ("workflow rm", Kind::Local(SYNCED)),
    ("workflow show", Kind::ReadEach),
    ("workflow update", Kind::Local(SYNCED)),
];

/// The declared kind of `command`, at `path`.
pub(crate) fn kind(path: &str, command: &Command) -> Kind {
    if let Command::Forge {
        command: ForgeCommand::Tunnel { switch: Some(_) },
    } = command
    {
        return Kind::Write;
    }
    KINDS
        .iter()
        .find(|(declared, _)| *declared == path)
        .map(|(_, kind)| *kind)
        .unwrap_or_else(|| panic!("`{path}` declares no host kind"))
}

/// Why a named host refuses this host-owned write: a flag of a setting that
/// `local` writes and syncs (spec 037, rules 26 and 28).
fn synced_flag(command: &Command) -> Option<String> {
    match command {
        Command::Permissions { .. } if !device_only(command) => Some(SYNCED_PERMISSIONS.into()),
        Command::Repo { command } => {
            let (flag, takes) = repo::synced_flag(command)?;
            Some(format!(
                "`--{flag}` is written on local and synced to every host; \
                 on a named host this command takes {takes}"
            ))
        }
        _ => None,
    }
}

/// A host status as the registry spells it.
fn status(status: HostStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(crate) async fn run(cli: Cli, local: Arc<Client>, path: &str) -> Result<ExitCode> {
    let Some(name) = cli.host.as_deref() else {
        return crate::run(cli, local).await;
    };
    if path == "host reconnect" {
        return Err(Failure::usage(
            "host reconnect takes a NAME or all, not --host; the local daemon serves it",
        )
        .err());
    }
    // The local daemon fans a query out itself, under `/v1/hosts/{host}` (037).
    if path == "query" {
        return crate::run(cli, local).await;
    }
    let kind = kind(path, &cli.command);
    if name == "local" {
        return crate::run(cli, local).await;
    }
    if name == "all"
        && let Kind::Local(reason) = kind
    {
        return Err(Failure::usage(format!("{reason}; run it with --host local")).err());
    }
    let hosts: Vec<HostDto> = match local.get_json("/v1/hosts").await {
        Ok(hosts) => hosts,
        Err(error) => {
            if let Kind::Local(reason) = kind {
                return Err(Failure::usage(format!("{reason}; run it with --host local")).err());
            }
            return Err(error.into());
        }
    };
    let host = if name == "all" {
        None
    } else {
        let host = hosts.iter().find(|host| host.name == name).ok_or_else(|| {
            let names = hosts
                .iter()
                .map(|host| format!("{} ({})", host.name, status(host.status)))
                .collect::<Vec<_>>()
                .join(", ");
            Failure::usage(format!("no host is named {name}; the hosts are {names}")).err()
        })?;
        Some(host)
    };
    if name == "all" {
        let accepting = match kind {
            Kind::HostOwned if synced_flag(&cli.command).is_some() => Some(vec!["local"]),
            Kind::Write | Kind::HostOwned => Some(
                hosts
                    .iter()
                    .filter(|host| host.status == HostStatus::Ready)
                    .map(|host| host.name.as_str())
                    .collect(),
            ),
            _ => None,
        };
        if let Some(accepting) = accepting {
            return Err(Failure::usage(format!(
                "--host all is for reads; select one host for this write: {}",
                accepting.join(", ")
            ))
            .err());
        }
        if matches!(
            path,
            "attach" | "goal attach" | "task attach" | "task logs" | "session logs"
        ) {
            let args: Vec<String> = std::env::args().collect();
            // Clap owns the positional ID, including flags before or after it.
            let matches = crate::cli::command().get_matches_from(args);
            let mut leaf = &matches;
            while let Some((_, nested)) = leaf.subcommand() {
                leaf = nested;
            }
            let id = leaf.get_one::<String>("id").expect("attach has an ID");
            let host = host_with_id(&hosts, &local, id).await?;
            let selected = client(host, &local)?;
            if matches!(path, "task logs" | "session logs")
                && forwarded_args()
                    .iter()
                    .any(|arg| arg == "-f" || arg == "--follow")
            {
                return follow_owned(host, &selected).await;
            }
            return crate::run(cli, selected).await;
        }
        if let Command::Daemon {
            command: DaemonCommand::Logs { follow: false, .. },
            ..
        } = cli.command
        {
            return logs_each(&hosts, &local).await;
        }
        if kind == Kind::Stream {
            return stream_all(&cli, &hosts, &local).await;
        }
        if kind == Kind::List {
            if commands::watches(&cli.command) {
                return watch_all(&cli, hosts, &local).await;
            }
            return merge_lists(&cli, &hosts, &local, &mut Vec::new()).await;
        }
        return match kind {
            Kind::ReadOne => read_one(cli.format, &hosts, &local).await,
            Kind::ReadEach => read_each(cli.format, &hosts, &local).await,
            _ => fan_out(&cli, &hosts, &local, false, &mut Vec::new()).await,
        };
    }
    let host = host.expect("a named host was checked before command guards");
    if host.status != HostStatus::Ready {
        let error = host
            .error
            .as_deref()
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        return Err(Failure::usage(format!("host {name} is {}{error}", status(host.status))).err());
    }
    if let Kind::Local(reason) = kind {
        return Err(Failure::usage(format!("{reason}; run it with --host local")).err());
    }
    if kind == Kind::HostOwned
        && let Some(message) = synced_flag(&cli.command)
    {
        return Err(Failure::usage(message).err());
    }
    let selected = client(host, &local)?;
    crate::run(cli, selected).await
}

fn client(host: &HostDto, local: &Arc<Client>) -> Result<Arc<Client>> {
    if host.name == "local" {
        return Ok(Arc::clone(local));
    }
    let endpoint = host
        .endpoint
        .as_deref()
        .ok_or_else(|| Failure::usage(format!("host {} has no endpoint", host.name)).err())?;
    Ok(Arc::new(Client::resolve(Some(endpoint), None)))
}

/// `permissions ai set` that names only `--device`: the one AI setting a host
/// keeps for itself. Every other `set` is written on local and synced.
fn device_only(command: &Command) -> bool {
    matches!(
        command,
        Command::Permissions {
            command: PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: None,
                deny_threshold: None,
                default_thresholds: false,
                flavour: None,
                device: Some(_),
            }),
        }
    )
}

/// The command line as typed, without `--host` and `--endpoint`, so each host
/// runs it alone, on the endpoint the caller adds.
fn forwarded_args() -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    let mut original = std::env::args_os().skip(1);
    while let Some(arg) = original.next() {
        if arg == "--host" || arg == "--endpoint" {
            original.next();
        } else if !arg.to_string_lossy().starts_with("--host=")
            && !arg.to_string_lossy().starts_with("--endpoint=")
        {
            args.push(arg);
        }
    }
    args
}

// Reuse each command's rendering and error handling in an isolated invocation.
// This also keeps a failed host from printing partial output into a listing.
async fn fan_out(
    cli: &Cli,
    hosts: &[HostDto],
    local: &Arc<Client>,
    listing: bool,
    failed: &mut Vec<String>,
) -> Result<ExitCode> {
    let args: Vec<_> = forwarded_args()
        .into_iter()
        .filter(|arg| arg != "--watch")
        .collect();
    let mut answered = false;
    let mut rows = Vec::new();
    let mut header = None;
    let host_width = hosts
        .iter()
        .map(|host| host.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    for host in hosts {
        if host.status != HostStatus::Ready {
            eprintln!("warning: {}: {}", host.name, status(host.status));
            continue;
        }
        let selected = client(host, local)?;
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args(&args)
            .arg("--endpoint")
            .arg(selected.endpoint())
            .kill_on_drop(true)
            .output()
            .await?;
        if !output.status.success() {
            failed.push(host.name.clone());
            if listing {
                eprintln!(
                    "warning: {}: {}",
                    host.name,
                    String::from_utf8_lossy(&output.stderr)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            continue;
        }
        answered = true;
        if cli.format == Format::Json {
            let value: Value = serde_json::from_slice(&output.stdout)?;
            let items = value
                .as_array()
                .or_else(|| value.get("items").and_then(Value::as_array))
                .or_else(|| value.get("sessions").and_then(Value::as_array))
                // `attention` keeps its items in `recovery`.
                .or_else(|| value.get("recovery").and_then(Value::as_array));
            if value.get("complete") == Some(&Value::Bool(false)) {
                eprintln!("warning: {}: the list is incomplete", host.name);
            }
            if let Some(items) = items {
                for item in items {
                    let mut item = item.clone();
                    item["host"] = host.name.clone().into();
                    rows.push(item);
                }
            }
        } else if cli.quiet {
            // Quiet output has no header: every line is an id, kept as it is.
            print!("{}", String::from_utf8_lossy(&output.stdout));
        } else {
            // A table's first line is its header; every later line is data.
            let text = String::from_utf8_lossy(&output.stdout);
            let mut lines = text.lines().filter(|line| !line.is_empty());
            if let Some(first) = lines.next()
                && header.is_none()
            {
                println!("{:<host_width$}   {first}", "HOST");
                header = Some(first.to_owned());
            }
            for line in lines {
                println!("{:<host_width$}   {line}", host.name);
            }
        }
    }
    if !answered {
        return Err(Failure::usage("no host answered").err());
    }
    if cli.format == Format::Json {
        output::print_json(&rows)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// Keep transcript chunks intact while the selected host owns the follow.
async fn follow_owned(host: &HostDto, selected: &Client) -> Result<ExitCode> {
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(forwarded_args())
        .arg("--endpoint")
        .arg(selected.endpoint())
        .env("ARIADNE_HOST_STREAM_ONCE", "1")
        .kill_on_drop(true)
        .spawn()?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            child.kill().await?;
            Ok(ExitCode::SUCCESS)
        }
        exit = child.wait() => {
            eprintln!("warning: {}: stream closed", host.name);
            Ok(ExitCode::from(exit?.code().unwrap_or(1) as u8))
        }
    }
}

/// What one host gave a list of every host.
enum Read {
    Listed(Box<Listing>),
    /// The host lacks the ID that the list is filtered by.
    NotFound(anyhow::Error),
    /// The host is not ready, has no endpoint, or failed: a warning.
    Warning(String),
}

/// A list on every ready host, read at the same time and drawn as one list
/// in registry order (spec 014, rules 46 and 46a). A host that lacks the ID of a
/// filter adds no rows; each other host that cannot answer gets a warning.
async fn merge_lists(
    cli: &Cli,
    hosts: &[HostDto],
    local: &Arc<Client>,
    failed: &mut Vec<String>,
) -> Result<ExitCode> {
    if let Command::Session {
        command: SessionCommand::Ls {
            cursor: Some(_), ..
        },
    } = &cli.command
    {
        return Err(Failure::usage(
            "a cursor continues the list of one host; run it with --host NAME",
        )
        .err());
    }
    let reads = hosts.iter().map(|host| async move {
        if host.status != HostStatus::Ready {
            return Read::Warning(status(host.status));
        }
        let selected = match client(host, local) {
            Ok(selected) => selected,
            Err(error) => return Read::Warning(error::human_line(&error)),
        };
        let read = commands::resolve::CHECK_WHOLE_IDS
            .scope(true, commands::listing(&selected, &cli.command, cli.format))
            .await;
        match read {
            Some(Ok(listing)) => Read::Listed(Box::new(listing)),
            Some(Err(error)) if error::exit(&error) == Exit::NotFound => Read::NotFound(error),
            Some(Err(error)) => Read::Warning(error::human_line(&error)),
            None => unreachable!("a list that is not watched has a listing"),
        }
    });
    let reads = futures_util::future::join_all(reads).await;
    let mut parts = Vec::new();
    let mut not_found = None;
    for (host, read) in hosts.iter().zip(reads) {
        match read {
            Read::Listed(listing) => parts.push((host.name.clone(), *listing)),
            Read::NotFound(error) => {
                not_found.get_or_insert(error);
            }
            Read::Warning(warning) => {
                failed.push(host.name.clone());
                eprintln!("warning: {}: {warning}", host.name);
            }
        }
    }
    if parts.is_empty() {
        return Err(not_found.unwrap_or_else(|| Failure::usage("no host answered").err()));
    }
    listing::print_merged(&parts, cli.format)?;
    Ok(ExitCode::SUCCESS)
}

/// Follow all ready hosts independently. Dropping the task set cancels its children.
async fn stream_all(cli: &Cli, hosts: &[HostDto], local: &Arc<Client>) -> Result<ExitCode> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut tasks = tokio::task::JoinSet::new();
    let (send, mut receive) = tokio::sync::mpsc::channel(128);
    for host in hosts {
        if host.status != HostStatus::Ready {
            eprintln!("warning: {}: {}", host.name, status(host.status));
            continue;
        }
        let selected = client(host, local)?;
        let name = host.name.clone();
        let send = send.clone();
        let args = forwarded_args();
        tasks.spawn(async move {
            let following = args.iter().any(|arg| arg == "-f" || arg == "--follow");
            let result: Result<()> = async {
                let mut child = tokio::process::Command::new(std::env::current_exe()?)
                    .args(args)
                    .arg("--endpoint")
                    .arg(selected.endpoint())
                    .env("ARIADNE_HOST_STREAM_ONCE", "1")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn()?;
                let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
                while let Some(line) = lines.next_line().await? {
                    if send.send((name.clone(), Some(line))).await.is_err() {
                        return Ok(());
                    }
                }
                anyhow::ensure!(child.wait().await?.success(), "host command failed");
                Ok(())
            }
            .await;
            if following || result.is_err() {
                let _ = send.send((name, None)).await;
            }
        });
    }
    drop(send);
    if tasks.is_empty() {
        return Err(Failure::usage("no host answered").err());
    }
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(ExitCode::SUCCESS),
            message = receive.recv() => match message {
                Some((name, Some(line))) => {
                    if cli.format == Format::Json && let Ok(mut value) = serde_json::from_str::<Value>(&line) {
                        value["host"] = name.into();
                        println!("{}", serde_json::to_string(&value)?);
                    } else { println!("{name}   {line}"); }
                }
                Some((name, None)) => eprintln!("warning: {name}: stream closed"),
                None => return Ok(ExitCode::SUCCESS),
            }
        }
    }
}

/// Subscribe before the first snapshot so a change cannot fall between them.
async fn watch_all(cli: &Cli, mut hosts: Vec<HostDto>, local: &Arc<Client>) -> Result<ExitCode> {
    use std::io::{IsTerminal, Write};
    // Keep the signal subscription across redraws and snapshot requests.
    let interrupted = tokio::signal::ctrl_c();
    tokio::pin!(interrupted);
    let mut tasks = tokio::task::JoinSet::new();
    let (send, mut receive) = tokio::sync::mpsc::channel(128);
    for host in &hosts {
        if host.status != HostStatus::Ready {
            eprintln!("warning: {}: {}", host.name, status(host.status));
            continue;
        }
        let selected = client(host, local)?;
        let name = host.name.clone();
        let send = send.clone();
        tasks.spawn(async move {
            let result: Result<()> = async {
                let mut stream = selected.stream("/v1/events/stream").await?;
                send.send((name.clone(), false)).await?;
                while let Some(frame) = stream.next().await {
                    frame?;
                    send.send((name.clone(), false)).await?;
                }
                Ok(())
            }
            .await;
            let _ = result;
            let _ = send.send((name, true)).await;
        });
    }
    hosts.retain(|host| host.status == HostStatus::Ready);
    drop(send);
    if tasks.is_empty() {
        return Err(Failure::usage("no host answered").err());
    }
    loop {
        tokio::select! {
            _ = &mut interrupted => return Ok(ExitCode::SUCCESS),
            message = receive.recv() => match message {
                Some((name, dropped)) => {
                    // A failed snapshot already warned and retired this host.
                    if !hosts.iter().any(|host| host.name == name) { continue; }
                    if dropped {
                        eprintln!("warning: {name}: stream closed");
                        hosts.retain(|host| host.name != name);
                    }
                    if hosts.is_empty() { return Ok(ExitCode::SUCCESS); }
                    if std::io::stdout().is_terminal() { print!("\x1b[H\x1b[2J"); }
                    let mut failed = Vec::new();
                    tokio::select! {
                        _ = &mut interrupted => return Ok(ExitCode::SUCCESS),
                        result = merge_lists(cli, &hosts, local, &mut failed) => { let _ = result?; }
                    }
                    hosts.retain(|host| !failed.contains(&host.name));
                    if hosts.is_empty() { return Ok(ExitCode::SUCCESS); }
                    std::io::stdout().flush()?;
                }
                None => return Ok(ExitCode::SUCCESS),
            }
        }
    }
}

/// Whether the daemon behind `client` has the session, task or goal `id`
/// names. A host that lacks it fails with not found.
async fn has_id(client: &Client, id: &str) -> Result<()> {
    let id = crate::commands::resolve::attachable(client, id).await?;
    let mut missing = None;
    for kind in ["sessions", "tasks", "goals"] {
        match client
            .get_json::<Value>(&format!(
                "/v1/{kind}/{}",
                crate::commands::path_segment(&id)
            ))
            .await
        {
            Ok(_) => return Ok(()),
            Err(error) => {
                let error = anyhow::Error::from(error);
                if error::exit(&error) != Exit::NotFound {
                    return Err(error);
                }
                missing = Some(error);
            }
        }
    }
    Err(missing.expect("three kinds were read"))
}

/// The one ready host that has the session, task or goal `id` names, for
/// `attach` and for a follow of one ID (spec 014, rule 46b). Each host that
/// is not ready or fails gets a warning.
pub(crate) async fn host_with_id<'h>(
    hosts: &'h [HostDto],
    local: &Arc<Client>,
    id: &str,
) -> Result<&'h HostDto> {
    let replies = on_each(hosts, local, |_, client| {
        let id = id.to_owned();
        async move { has_id(&client, &id).await }
    })
    .await;
    let (host, ()) = the_one(replies)?;
    Ok(host)
}

/// What one host answered a read of every host.
enum Reply<T> {
    Answered(T),
    /// The host lacks the ID that the read names.
    NotFound(anyhow::Error),
    /// The host is not ready, has no endpoint, or failed.
    Failed(String, Exit),
}

/// `read` on each ready host at the same time, in registry order.
async fn on_each<'h, T, F, R>(
    hosts: &'h [HostDto],
    local: &Arc<Client>,
    read: F,
) -> Vec<(&'h HostDto, Reply<T>)>
where
    F: Fn(&'h HostDto, Arc<Client>) -> R,
    R: std::future::Future<Output = Result<T>>,
{
    let read = &read;
    let replies = hosts.iter().map(|host| async move {
        if host.status != HostStatus::Ready {
            return Reply::Failed(status(host.status), Exit::Usage);
        }
        let selected = match client(host, local) {
            Ok(selected) => selected,
            Err(error) => return Reply::Failed(error::human_line(&error), error::exit(&error)),
        };
        match read(host, selected).await {
            Ok(answer) => Reply::Answered(answer),
            Err(error) if error::exit(&error) == Exit::NotFound => Reply::NotFound(error),
            Err(error) => Reply::Failed(error::human_line(&error), error::exit(&error)),
        }
    });
    hosts
        .iter()
        .zip(futures_util::future::join_all(replies).await)
        .collect()
}

/// The command line, parsed again, to run on `host` alone.
fn cli_on(host: &str) -> Cli {
    let matches = crate::cli::command().get_matches_from(std::env::args_os());
    let mut cli = <Cli as clap::FromArgMatches>::from_arg_matches(&matches)
        .expect("the command line parsed before");
    cli.host = Some(host.to_owned());
    cli
}

/// The command on each ready host, run in this process with its stdout held:
/// the exit code and the text of each host that answered.
async fn run_on_each<'h>(
    hosts: &'h [HostDto],
    local: &Arc<Client>,
) -> Vec<(&'h HostDto, Reply<(ExitCode, String)>)> {
    on_each(hosts, local, |host, client| async move {
        let run = Box::pin(crate::run(cli_on(&host.name), client));
        let (code, text) = output::held(run).await;
        code.map(|code| (code, text))
    })
    .await
}

/// One warning on stderr for each host that failed, and for each host that
/// lacks the ID where `not_found` says that a host should have it.
fn warn<T>(replies: &[(&HostDto, Reply<T>)], not_found: bool) {
    for (host, reply) in replies {
        match reply {
            Reply::Failed(warning, _) => eprintln!("warning: {}: {warning}", host.name),
            Reply::NotFound(error) if not_found => {
                eprintln!("warning: {}: {}", host.name, error::human_line(error))
            }
            _ => {}
        }
    }
}

/// Why no host answered: each ready host lacks the ID, or a host failed, in
/// the exit code of the first host that failed.
fn none_answered<T>(replies: &[(&HostDto, Reply<T>)]) -> anyhow::Error {
    let failed = replies.iter().find_map(|(host, reply)| match reply {
        Reply::Failed(_, exit) if host.status == HostStatus::Ready => Some(*exit),
        _ => None,
    });
    let lacking = replies
        .iter()
        .any(|(_, reply)| matches!(reply, Reply::NotFound(_)));
    match (failed, lacking) {
        (None, true) => Failure::not_found("no host has that ID").err(),
        (Some(exit), true) => Failure::with_exit(exit, "no host that answered has that ID").err(),
        (Some(exit), false) => Failure::with_exit(exit, "no host answered").err(),
        (None, false) => Failure::usage("no host answered").err(),
    }
}

/// The one host that has the ID: a prefix that matches on two hosts is
/// refused, and the refusal names each of them.
fn the_one<T>(replies: Vec<(&HostDto, Reply<T>)>) -> Result<(&HostDto, T)> {
    warn(&replies, false);
    let matched: Vec<&str> = replies
        .iter()
        .filter(|(_, reply)| matches!(reply, Reply::Answered(_)))
        .map(|(host, _)| host.name.as_str())
        .collect();
    if matched.len() > 1 {
        return Err(Failure::usage(format!(
            "the ID matches on hosts {}; select one with --host NAME",
            matched.join(", ")
        ))
        .err());
    }
    let error = none_answered(&replies);
    replies
        .into_iter()
        .find_map(|(host, reply)| match reply {
            Reply::Answered(answer) => Some((host, answer)),
            _ => None,
        })
        .ok_or(error)
}

/// The JSON a host printed, with its `host` field. A payload that is not an
/// object is the `value` of one.
fn with_host(host: &str, text: &str) -> Value {
    let value = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()));
    let mut tagged = match value {
        Value::Object(object) => object,
        value => serde_json::Map::from_iter([("value".to_owned(), value)]),
    };
    tagged.insert("host".into(), host.into());
    Value::Object(tagged)
}

/// A read of one ID with `--host all`: the answer of the one host that has
/// the ID, after a `host:` line in text, or with a `host` field in JSON
/// (spec 014, rule 46b).
async fn read_one(format: Format, hosts: &[HostDto], local: &Arc<Client>) -> Result<ExitCode> {
    let (host, (code, text)) = the_one(run_on_each(hosts, local).await)?;
    if format == Format::Json {
        output::print_json(&with_host(&host.name, &text))?;
    } else {
        println!("host: {}", host.name);
        print!("{text}");
    }
    Ok(code)
}

/// A read that every host answers with `--host all`: one block for each host
/// that answered, headed by its name, or a JSON array whose items each have a
/// `host` field (spec 014, rule 46b).
async fn read_each(format: Format, hosts: &[HostDto], local: &Arc<Client>) -> Result<ExitCode> {
    let replies = run_on_each(hosts, local).await;
    warn(&replies, true);
    let answered: Vec<(&str, &ExitCode, &str)> = replies
        .iter()
        .filter_map(|(host, reply)| match reply {
            Reply::Answered((code, text)) => Some((host.name.as_str(), code, text.as_str())),
            _ => None,
        })
        .collect();
    if answered.is_empty() {
        return Err(none_answered(&replies));
    }
    if format == Format::Json {
        let items: Vec<Value> = answered
            .iter()
            .map(|(host, _, text)| with_host(host, text))
            .collect();
        output::print_json(&items)?;
    } else {
        for (index, (host, _, text)) in answered.iter().enumerate() {
            if index > 0 {
                println!();
            }
            println!("{host}");
            print!("{text}");
        }
    }
    Ok(first_failure(answered.iter().map(|(_, code, _)| *code)))
}

/// The first exit code that is not success, as `doctor` gives one for a
/// report that found something.
fn first_failure<'c>(codes: impl IntoIterator<Item = &'c ExitCode>) -> ExitCode {
    codes
        .into_iter()
        .find(|code| **code != ExitCode::SUCCESS)
        .copied()
        .unwrap_or(ExitCode::SUCCESS)
}

/// `daemon logs --host all`: the recent lines of each host that answered,
/// each led by its host name, in registry order (spec 014, rule 46b).
async fn logs_each(hosts: &[HostDto], local: &Arc<Client>) -> Result<ExitCode> {
    let replies = run_on_each(hosts, local).await;
    warn(&replies, true);
    let width = hosts.iter().map(|host| host.name.len()).max().unwrap_or(0);
    let mut answered = false;
    for (host, reply) in &replies {
        if let Reply::Answered((_, text)) = reply {
            answered = true;
            for line in text.lines() {
                println!("{:<width$}   {line}", host.name);
            }
        }
    }
    if !answered {
        return Err(none_answered(&replies));
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) async fn setup(
    client: &Client,
    name: &str,
    request: Option<&std::path::Path>,
    format: Format,
) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err(Failure::usage("invalid worker name").err());
    }
    let path = format!("/v1/hosts/{name}/setup");
    let progress: ariadne_api::hosts::SetupDto = if let Some(file) = request {
        let request: ariadne_api::hosts::SetupRequest =
            serde_json::from_slice(&std::fs::read(file)?)?;
        client.post_json(&path, &request).await?
    } else {
        client.get_json(&path).await?
    };
    if format == Format::Json {
        output::print_json(&progress)?;
    } else {
        println!("{}: {:?}", progress.name, progress.state);
        for step in progress.steps {
            println!(
                "{}: {}{}",
                step.name,
                if step.complete { "complete" } else { "pending" },
                step.error.map(|e| format!(" — {e}")).unwrap_or_default()
            );
        }
    }
    if matches!(
        progress.state,
        ariadne_api::hosts::SetupState::Failed | ariadne_api::hosts::SetupState::Incomplete
    ) {
        return Err(Failure::usage(
            "worker setup is incomplete; correct the cause and retry the original request",
        )
        .err());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every runnable command path below `cmd`, as `main::command_path`
    /// spells it.
    fn paths(cmd: &clap::Command, path: &str, out: &mut Vec<String>) {
        let subcommands: Vec<_> = cmd.get_subcommands().collect();
        // `attention` and `completions <SHELL>` run without their subcommand.
        if subcommands.is_empty() || cmd.get_positionals().next().is_some() || path == "attention" {
            out.push(path.to_owned());
        }
        for sub in subcommands {
            let below = match path {
                "" => sub.get_name().to_owned(),
                _ => format!("{path} {}", sub.get_name()),
            };
            paths(sub, &below, out);
        }
    }

    #[test]
    fn every_command_declares_one_kind() {
        let mut runnable = Vec::new();
        paths(&crate::cli::command(), "", &mut runnable);
        runnable.sort();
        let mut declared: Vec<String> = KINDS.iter().map(|(path, _)| (*path).into()).collect();
        declared.sort();
        assert_eq!(declared, runnable);
    }

    #[test]
    fn reads_that_every_host_answers_are_not_writes() {
        for path in [
            "events",
            "doctor",
            "version",
            "daemon status",
            "models show",
            "skill get",
            "skill inspect",
            "workflow show",
            "forge tunnel",
            "workflow check",
            "permissions ai test",
            "completions",
        ] {
            let kind = KINDS
                .iter()
                .find(|(declared, _)| *declared == path)
                .unwrap()
                .1;
            assert!(
                matches!(kind, Kind::ReadEach | Kind::Stream),
                "{path}: {kind:?}"
            );
        }
    }

    #[test]
    fn a_tunnel_switch_is_a_write_and_the_bare_tunnel_a_read() {
        let parse = |args: &[&str]| <Cli as clap::Parser>::try_parse_from(args).unwrap().command;
        assert_eq!(
            kind("forge tunnel", &parse(&["ariadne", "forge", "tunnel"])),
            Kind::ReadEach
        );
        assert_eq!(
            kind(
                "forge tunnel",
                &parse(&["ariadne", "forge", "tunnel", "off"])
            ),
            Kind::Write
        );
    }
}
