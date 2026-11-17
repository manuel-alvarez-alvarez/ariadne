//! Host selection uses the registry of the locally resolved daemon.
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_client::Client;
use serde_json::Value;

use crate::cli::{Cli, Command};
use crate::commands::forge::ForgeCommand;
use crate::commands::permissions::{AiPermissionsCommand, PermissionsCommand};
use crate::commands::repo;
use crate::error::Failure;
use crate::output::{self, Format, UNCAPPED, col};

pub(crate) async fn list(client: &Client, format: Format) -> Result<()> {
    let hosts: Vec<HostDto> = client.get_json("/v1/hosts").await?;
    output::print_list(
        format,
        &hosts,
        &[
            col("name", UNCAPPED),
            col("kind", UNCAPPED),
            col("status", UNCAPPED),
            col("endpoint", UNCAPPED),
            col("error", UNCAPPED),
        ],
        |host| {
            let value = serde_json::to_value(host).expect("host serializes");
            ["name", "kind", "status", "endpoint", "error"]
                .map(|key| value[key].as_str().unwrap_or("-").to_owned())
                .to_vec()
        },
        "",
    )
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
    ("issue ls", Kind::List),
    (
        "mcp serve",
        Kind::Local("the MCP server serves the local daemon"),
    ),
    ("models disable", Kind::Write),
    ("models enable", Kind::Write),
    ("models ls", Kind::List),
    ("models rank", Kind::Write),
    ("models show", Kind::ReadOne),
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
    ("skill get", Kind::ReadOne),
    ("skill inspect", Kind::ReadOne),
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
    ("workflow show", Kind::ReadOne),
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
    let hosts: Vec<HostDto> = local.get_json("/v1/hosts").await?;
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
        if matches!(path, "attach" | "goal attach" | "task attach") {
            let args: Vec<String> = std::env::args().collect();
            // Clap owns the positional ID, including flags before or after it.
            let matches = crate::cli::command().get_matches_from(args);
            let mut leaf = &matches;
            while let Some((_, nested)) = leaf.subcommand() {
                leaf = nested;
            }
            let id = leaf.get_one::<String>("id").expect("attach has an ID");
            for host in &hosts {
                if host.status != HostStatus::Ready {
                    continue;
                }
                let client = client(host, &local)?;
                if has_id(&client, id).await {
                    return crate::run(cli, client).await;
                }
            }
            return Err(Failure::not_found(format!("no host has {id}")).err());
        }
        if path == "permissions ai show" {
            return show_each(&cli, &hosts, &local).await;
        }
        return fan_out(&cli, &hosts, &local, kind == Kind::List).await;
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

/// `permissions ai show --host all`: one block per ready host, in registry
/// order, each headed by the host's name. A host that fails gets a warning.
async fn show_each(cli: &Cli, hosts: &[HostDto], local: &Arc<Client>) -> Result<ExitCode> {
    let args = forwarded_args();
    let mut answered = false;
    let mut rows = Vec::new();
    for host in hosts {
        if host.status != HostStatus::Ready {
            eprintln!("warning: {}: {}", host.name, status(host.status));
            continue;
        }
        let selected = match client(host, local) {
            Ok(selected) => selected,
            Err(error) => {
                eprintln!("warning: {}: {error}", host.name);
                continue;
            }
        };
        let output = tokio::process::Command::new(std::env::current_exe()?)
            .args(&args)
            .arg("--endpoint")
            .arg(selected.endpoint())
            .output()
            .await?;
        if !output.status.success() {
            eprintln!(
                "warning: {}: {}",
                host.name,
                String::from_utf8_lossy(&output.stderr)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            continue;
        }
        if cli.format == Format::Json {
            let mut value: Value = serde_json::from_slice(&output.stdout)?;
            value["host"] = host.name.clone().into();
            rows.push(value);
        } else {
            if answered {
                println!();
            }
            println!("{}", host.name);
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        answered = true;
    }
    if !answered {
        return Err(Failure::usage("no host answered").err());
    }
    if cli.format == Format::Json {
        output::print_json(&rows)?;
    }
    Ok(ExitCode::SUCCESS)
}

// Reuse each command's rendering and error handling in an isolated invocation.
// This also keeps a failed host from printing partial output into a listing.
async fn fan_out(
    cli: &Cli,
    hosts: &[HostDto],
    local: &Arc<Client>,
    listing: bool,
) -> Result<ExitCode> {
    let args = forwarded_args();
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
            .output()
            .await?;
        if !output.status.success() {
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
        if !listing {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            return Ok(ExitCode::SUCCESS);
        }
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
        return Err(if listing {
            Failure::usage("no host answered")
        } else {
            Failure::not_found("no host has that ID")
        }
        .err());
    }
    if cli.format == Format::Json {
        output::print_json(&rows)?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn has_id(client: &Client, id: &str) -> bool {
    let Ok(id) = crate::commands::resolve::attachable(client, id).await else {
        return false;
    };
    for kind in ["sessions", "tasks", "goals"] {
        if client
            .get_json::<Value>(&format!(
                "/v1/{kind}/{}",
                crate::commands::path_segment(&id)
            ))
            .await
            .is_ok()
        {
            return true;
        }
    }
    false
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
