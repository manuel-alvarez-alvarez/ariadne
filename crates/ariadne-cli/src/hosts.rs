//! Host selection uses the registry of the locally resolved daemon.
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use ariadne_api::hosts::{HostDto, HostStatus};
use ariadne_client::Client;
use serde_json::Value;

use crate::cli::{Cli, Command};
use crate::commands::permissions::{AiPermissionsCommand, PermissionsCommand};
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

pub(crate) async fn run(cli: Cli, local: Arc<Client>, path: &str) -> Result<ExitCode> {
    let Some(name) = cli.host.as_deref() else {
        return crate::run(cli, local).await;
    };
    // The local daemon fans a query out itself, under `/v1/hosts/{host}` (037).
    if path == "query" || name == "local" {
        return crate::run(cli, local).await;
    }
    let hosts: Vec<HostDto> = local.get_json("/v1/hosts").await?;
    let names = hosts
        .iter()
        .map(|host| {
            format!(
                "{} ({})",
                host.name,
                format!("{:?}", host.status).to_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let verb = path.split_whitespace().last().unwrap_or("");
    let listing = matches!(verb, "ls" | "list" | "search") || path == "attention";
    let reading = listing
        || matches!(
            verb,
            "inspect"
                | "show"
                | "get"
                | "logs"
                | "status"
                | "history"
                | "messages"
                | "diff"
                | "attach"
        )
        || path == "attach"
        || path == "version";
    if name == "all" {
        if !reading {
            return Err(Failure::usage(format!("select one host for writes: {names}")).err());
        }
        if path == "attach" || verb == "attach" {
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
        return fan_out(&cli, &hosts, &local, listing).await;
    }
    let host = hosts
        .iter()
        .find(|host| host.name == name && host.status == HostStatus::Ready)
        .ok_or_else(|| {
            Failure::usage(format!("host {name} is unavailable; hosts: {names}")).err()
        })?;
    if name != "local" {
        if path.starts_with("daemon ") && !matches!(verb, "status" | "logs") {
            return Err(Failure::usage("daemon lifecycle commands require --host local").err());
        }
        if !reading
            && !device_only(&cli.command)
            && ["skill ", "workflow ", "permissions ", "repo "]
                .iter()
                .any(|prefix| path.starts_with(prefix))
        {
            let message = if path.starts_with("permissions ") {
                "these settings are written on local and synced to every host; \
                 only `permissions ai set --device` is per host"
            } else {
                "these settings are written on local and synced to every host"
            };
            return Err(Failure::usage(message).err());
        }
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

/// The command line as typed, without `--host`, so each host runs it alone.
fn forwarded_args() -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    let mut original = std::env::args_os().skip(1);
    while let Some(arg) = original.next() {
        if arg == "--host" {
            original.next();
        } else if !arg.to_string_lossy().starts_with("--host=") {
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
            eprintln!("warning: {}: {:?}", host.name, host.status);
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
            eprintln!("warning: {}: {:?}", host.name, host.status);
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
