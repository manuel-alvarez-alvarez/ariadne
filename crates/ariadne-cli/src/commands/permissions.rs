//! `ariadne permissions ...` — the AI permission model behind the `ai` permission mode (022).

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::permissions::{
    AiPermissionsState, AiPermissionsStatusDto, Device, Flavour, LearnedPermissionDto, PythonDto,
    TestAiPermissionRequest, TestAiPermissionResponse, UpdateAiPermissionsRequest,
};
use ariadne_client::Client;

use super::resolve::{self, Kind};
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, dash, moment, ok_id_line, print, print_kv, print_list,
    print_table, style, view, yes_no,
};

#[derive(Subcommand)]
pub(crate) enum PermissionsCommand {
    /// Manage the AI permission model
    #[command(subcommand)]
    Ai(#[command(subcommand)] AiPermissionsCommand),
    /// Manage learned choices
    #[command(subcommand)]
    Learned(#[command(subcommand)] LearnedPermissionsCommand),
}

#[derive(Subcommand)]
pub(crate) enum LearnedPermissionsCommand {
    /// List learned choices
    List {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Show one learned choice
    Show { id: String },
    /// Remove a learned choice
    Rm { id: String },
}

#[derive(Subcommand)]
pub(crate) enum AiPermissionsCommand {
    /// Show the AI permission settings and where the install has got to
    Show,
    /// Turn the model on and start the install
    Enable {
        /// Wait until the install leaves "installing"
        #[arg(long)]
        wait: bool,
    },
    /// Turn the model off; the install stays on disk
    Disable,
    /// Run the install again, on the settings as they stand
    Refresh {
        /// Wait until the install leaves "installing"
        #[arg(long)]
        wait: bool,
    },
    /// Change the decision thresholds, or the flavour and device
    #[command(group = clap::ArgGroup::new("ai-set")
        .args(["allow_threshold", "deny_threshold", "flavour", "device"])
        .required(true)
        .multiple(true))]
    Set {
        /// Allow danger at or below this value, 0 to 1 (default 0.0531)
        #[arg(long, value_parser = parse_threshold)]
        allow_threshold: Option<f64>,
        /// Deny danger at or above this value, 0 to 1 (default 0.6522)
        #[arg(long, value_parser = parse_threshold)]
        deny_threshold: Option<f64>,
        /// The Kev flavour to run: 0.8b, 4b, 9b or 27b
        #[arg(long, value_parser = parse_flavour)]
        flavour: Option<Flavour>,
        /// The device to run it on: mlx, cuda or cpu
        #[arg(long, value_parser = parse_device)]
        device: Option<Device>,
    },
    /// Score one request with the AI permission model
    Test {
        /// The tool call title
        #[arg(long)]
        tool: String,
        /// The tool call kind
        #[arg(long)]
        kind: Option<String>,
        /// The tool call input as JSON
        #[arg(long, value_parser = parse_json)]
        input: serde_json::Value,
        /// An option name, repeated for each option
        #[arg(long = "option")]
        options: Vec<String>,
        /// The workspace used to derive whether a path is outside it
        #[arg(long)]
        workspace: Option<String>,
    },
}

pub(crate) async fn run(client: &Client, cmd: PermissionsCommand, format: Format) -> Result<()> {
    match cmd {
        PermissionsCommand::Ai(command) => run_ai(client, command, format).await,
        PermissionsCommand::Learned(command) => run_learned(client, command, format).await,
    }
}

fn learned_row(row: &LearnedPermissionDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    vec![
        row.id.clone(),
        row.repository_id.clone(),
        row.tool_name.clone(),
        row.level.as_str().into(),
        row.family.clone(),
        row.key.clone(),
        row.scope.as_str().into(),
        row.target.as_str().into(),
        row.selected_option.clone(),
        age(&row.created_at, now),
        age(&row.updated_at, now),
    ]
}

const LEARNED_LIST: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("repository", UNCAPPED),
    col("tool", 28),
    col("level", 7),
    col("family", 20),
    col("key", 60),
    col("scope", 10),
    col("target", 6),
    col("selected", 20),
    col("created", UNCAPPED),
    col("updated", UNCAPPED),
];

async fn run_learned(
    client: &Client,
    cmd: LearnedPermissionsCommand,
    format: Format,
) -> Result<()> {
    match cmd {
        LearnedPermissionsCommand::List { repo } => {
            let repo = match repo {
                Some(repo) => Some(resolve::id(client, Kind::Repo, &repo).await?),
                None => None,
            };
            let response = client.list_learned_permissions(repo.as_deref()).await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &response.items,
                LEARNED_LIST,
                |row| learned_row(row, now),
                "No learned choices yet.",
            )
        }
        LearnedPermissionsCommand::Show { id } => {
            let row = client.get_learned_permission(&id).await?;
            print(format, &row, || print_learned(&row))
        }
        LearnedPermissionsCommand::Rm { id } => {
            client.delete_learned_permission(&id).await?;
            if format == Format::Json {
                crate::output::print_json(&serde_json::json!({"id": id}))
            } else {
                println!("{}", ok_id_line(view().color, view().quiet, "deleted", &id));
                Ok(())
            }
        }
    }
}

fn print_learned(row: &LearnedPermissionDto) {
    print_kv(&learned_fields(row));
}

/// The risk tags as a comma-separated list, or a dash where there are none.
fn tags(risk_tags: &[String]) -> String {
    match risk_tags.is_empty() {
        true => "-".into(),
        false => risk_tags.join(", "),
    }
}

/// Every field of a learned choice, its JSON fields pretty-printed.
fn learned_fields(row: &LearnedPermissionDto) -> Vec<(&'static str, Kv)> {
    let json = |value: &serde_json::Value| serde_json::to_string_pretty(value).unwrap_or_default();
    vec![
        ("id", Kv::id(row.id.clone())),
        ("repository", row.repository_id.clone().into()),
        ("tool", row.tool_name.clone().into()),
        ("level", row.level.as_str().into()),
        ("family", row.family.clone().into()),
        ("key", row.key.clone().into()),
        ("risk tags", tags(&row.risk_tags).into()),
        ("scope", row.scope.as_str().into()),
        ("target", row.target.as_str().into()),
        ("selected option", row.selected_option.clone().into()),
        ("tool call", json(&row.tool_call).into()),
        ("options", json(&row.options).into()),
        (
            "output",
            row.output.as_ref().map_or_else(|| "-".into(), json).into(),
        ),
        ("created", Kv::meta(moment(&row.created_at))),
        ("updated", Kv::meta(moment(&row.updated_at))),
    ]
}

async fn run_ai(client: &Client, cmd: AiPermissionsCommand, format: Format) -> Result<()> {
    match cmd {
        AiPermissionsCommand::Show => show(client, format).await,
        AiPermissionsCommand::Enable { wait } => {
            settle(
                client,
                format,
                wait,
                client
                    .update_ai_permissions(&UpdateAiPermissionsRequest {
                        enabled: Some(true),
                        ..Default::default()
                    })
                    .await?,
            )
            .await
        }
        AiPermissionsCommand::Disable => {
            let status = client
                .update_ai_permissions(&UpdateAiPermissionsRequest {
                    enabled: Some(false),
                    ..Default::default()
                })
                .await?;
            print_result(format, &status)
        }
        AiPermissionsCommand::Refresh { wait } => {
            settle(client, format, wait, client.refresh_ai_permissions().await?).await
        }
        AiPermissionsCommand::Set {
            allow_threshold,
            deny_threshold,
            flavour,
            device,
        } => {
            let status = client
                .update_ai_permissions(&UpdateAiPermissionsRequest {
                    enabled: None,
                    allow_threshold,
                    deny_threshold,
                    flavour,
                    device,
                })
                .await?;
            print_result(format, &status)
        }
        AiPermissionsCommand::Test {
            tool,
            kind,
            input,
            options,
            workspace,
        } => {
            let response = client
                .test_ai_permission(&TestAiPermissionRequest {
                    tool,
                    kind,
                    input,
                    options: (!options.is_empty()).then_some(options),
                    workspace,
                })
                .await?;
            print(format, &response, || {
                println!("{}", test_one_line(&response))
            })
        }
    }
}

fn parse_json(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|error| format!("`--input` must be valid JSON: {error}"))
}

fn test_one_line(response: &TestAiPermissionResponse) -> String {
    let answer = match (&response.label, response.danger, &response.ai_error) {
        (Some(label), Some(_), None) => {
            let metrics = ariadne_api::permissions::ai_permission_metrics(
                &serde_json::to_value(response).expect("AI permission response serializes"),
            );
            metrics.map_or_else(|| label.clone(), |metrics| format!("{label} · {metrics}"))
        }
        (Some(label), None, None) => label.clone(),
        (_, _, Some(error)) => format!("no answer: {error}"),
        _ => "no answer".to_string(),
    };
    match &response.operation {
        Some(operation) => format!("{answer}; operation {operation}"),
        None => answer,
    }
}

/// `--flavour`, refused locally before anything is sent: the daemon would
/// refuse the same value with a 422, but a round trip is not needed to know
/// the four flavours from a typo.
fn parse_flavour(s: &str) -> Result<Flavour, String> {
    Flavour::parse(s).ok_or_else(|| format!("flavour must be 0.8b, 4b, 9b or 27b, not `{s}`"))
}

/// `--device`, refused locally the same way.
fn parse_device(s: &str) -> Result<Device, String> {
    Device::parse(s).ok_or_else(|| format!("device must be mlx, cuda or cpu, not `{s}`"))
}

async fn show(client: &Client, format: Format) -> Result<()> {
    let status = client.ai_permissions_status().await?;
    match format {
        Format::Json => crate::output::print_json(&status),
        Format::Table => {
            print_kv(&fields(&status));
            println!();
            print_table(FLAVOURS_TABLE, &flavour_rows(&status))
        }
    }
}

const FLAVOURS_TABLE: &[Column] = &[
    col("flavour", UNCAPPED),
    col("device", UNCAPPED),
    col("can run", UNCAPPED),
    col("reason", 40),
    col("slow", UNCAPPED),
];

/// One row per flavour and device combination, unavailable ones included.
fn flavour_rows(status: &AiPermissionsStatusDto) -> Vec<Vec<String>> {
    status
        .flavours
        .iter()
        .flat_map(|flavour_options| {
            flavour_options.devices.iter().map(move |device| {
                vec![
                    flavour_options.flavour.as_str().to_string(),
                    device.device.as_str().to_string(),
                    yes_no(device.can_run, "no"),
                    dash(device.reason.as_deref()),
                    yes_no(device.slow, "no"),
                ]
            })
        })
        .collect()
}

/// `--wait` on `enable` and `refresh`: neither answers with anything but
/// `installing` once it has actually started an install, so there is nothing
/// to wait for when the settings did not change.
async fn settle(
    client: &Client,
    format: Format,
    wait: bool,
    status: AiPermissionsStatusDto,
) -> Result<()> {
    if !wait {
        return print_result(format, &status);
    }
    let status = wait_for_settled(client, status).await?;
    if status.state == AiPermissionsState::Failed {
        anyhow::bail!(
            status
                .last_error
                .clone()
                .unwrap_or_else(|| "the install failed".to_string())
        );
    }
    print_result(format, &status)
}

/// Block until `state` leaves `installing`, by following `ai_permissions_updated` on
/// the daemon's domain event stream — the same stream `ariadne events`
/// already follows, which is what "the CLI has a helper for it" means here.
///
/// The stream carries no replay (`Client::stream`), and the install is a
/// background task that can finish between the `PUT`'s own answer and this
/// connection opening — instantly, on the stub installer a test uses. A
/// status read right after subscribing, before any frame is read, catches
/// that: nothing published from the subscribe onward is missed, so what is
/// missed can only be older than the status this reads. The same read again
/// once the connection ends without a settled event covers a dropped
/// connection the same way.
async fn wait_for_settled(
    client: &Client,
    initial: AiPermissionsStatusDto,
) -> Result<AiPermissionsStatusDto> {
    if initial.state != AiPermissionsState::Installing {
        return Ok(initial);
    }
    let mut stream = client.stream("/v1/events/stream").await?;
    let status = client.ai_permissions_status().await?;
    if status.state != AiPermissionsState::Installing {
        return Ok(status);
    }
    while let Some(frame) = stream.next().await {
        let frame = frame?;
        if frame.event == "ai_permissions_updated" {
            let status: AiPermissionsStatusDto = serde_json::from_str(&frame.data)?;
            if status.state != AiPermissionsState::Installing {
                return Ok(status);
            }
        }
    }
    let status = client.ai_permissions_status().await?;
    if status.state == AiPermissionsState::Installing {
        anyhow::bail!("the connection ended before the install finished");
    }
    Ok(status)
}

fn print_result(format: Format, status: &AiPermissionsStatusDto) -> Result<()> {
    print(format, status, || println!("{}", one_line(status)))
}

/// The one line a mutation ends on: `the AI permission model is now <state>`, coloured and
/// glyphed exactly as `ariadne events` paints the same word in a stream.
fn one_line(status: &AiPermissionsStatusDto) -> String {
    let color = view().color;
    let word = status.state.as_str();
    let (sty, glyph) = style::status(word);
    let painted = match (color, glyph) {
        (true, Some(glyph)) => format!("{glyph} {word}"),
        _ => word.to_string(),
    };
    format!(
        "the AI permission model is now {}",
        style::paint(color, sty, &painted)
    )
}

/// `show`'s key/value block: every field of the status, in the order the
/// ticket lists them.
fn fields(status: &AiPermissionsStatusDto) -> Vec<(&'static str, Kv)> {
    vec![
        ("enabled", yes_no(status.enabled, "no").into()),
        ("state", Kv::status(status.state.as_str())),
        ("python", python_field(&status.python).into()),
        ("allow threshold", status.allow_threshold.to_string().into()),
        ("deny threshold", status.deny_threshold.to_string().into()),
        ("flavour", status.flavour.as_str().into()),
        ("device", status.device.as_str().into()),
        ("hardware", hardware_field(&status.hardware).into()),
        (
            "installed release",
            dash(status.installed_release.as_deref()).into(),
        ),
        (
            "latest release",
            dash(status.latest_release.as_deref()).into(),
        ),
        ("weights", yes_no(status.weights_present, "no").into()),
        ("endpoint", dash(status.endpoint.as_deref()).into()),
        (
            "last refresh",
            last_refresh(status.last_refresh_at.as_deref()),
        ),
        ("last error", dash(status.last_error.as_deref()).into()),
    ]
}

fn python_field(python: &PythonDto) -> String {
    match (&python.path, &python.version) {
        (Some(path), Some(version)) => format!("{version} at {path}"),
        _ => "not found".to_string(),
    }
}

fn hardware_field(hardware: &ariadne_api::permissions::HardwareDto) -> String {
    let memory_gb = hardware.memory_bytes / (1024 * 1024 * 1024);
    match &hardware.gpu {
        Some(gpu) => format!(
            "{} {}, {memory_gb} GB RAM, {} ({} GB VRAM)",
            hardware.os,
            hardware.arch,
            gpu.name,
            gpu.vram_bytes / (1024 * 1024 * 1024)
        ),
        None => format!(
            "{} {}, {memory_gb} GB RAM, no GPU",
            hardware.os, hardware.arch
        ),
    }
}

fn last_refresh(at: Option<&str>) -> Kv {
    match at {
        Some(rfc3339) => age(rfc3339, chrono::Utc::now()).into(),
        None => "never".into(),
    }
}

/// A threshold flag, refused locally before anything is sent: the daemon would
/// refuse the same value with a 422, but a round trip is not needed to know
/// 0 to 1 from a typo.
fn parse_threshold(s: &str) -> Result<f64, String> {
    let value: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("threshold must be between 0 and 1, not {value}"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::sse::{Event, Sse};
    use axum::routing::{get, post, put};
    use axum::{Json, Router};
    use futures_util::stream;
    use serde_json::json;

    use ariadne_api::error::ErrorBody;
    use ariadne_client::ClientError;

    fn status(state: AiPermissionsState) -> AiPermissionsStatusDto {
        AiPermissionsStatusDto {
            enabled: true,
            allow_threshold: 0.2,
            deny_threshold: 0.8,
            flavour: Flavour::Kev4B,
            device: Device::Mlx,
            hardware: ariadne_api::permissions::HardwareDto {
                os: "macos".into(),
                arch: "aarch64".into(),
                memory_bytes: 64 * 1024 * 1024 * 1024,
                gpu: None,
            },
            flavours: flavour_options(),
            python: PythonDto {
                path: Some("/usr/bin/python3".into()),
                version: Some("3.12.1".into()),
                ok: true,
            },
            state,
            installed_release: Some("v0.1.4".into()),
            latest_release: Some("v0.1.4".into()),
            weights_present: true,
            endpoint: Some("http://127.0.0.1:8900".into()),
            last_refresh_at: Some("2026-09-20T10:00:00Z".into()),
            last_error: None,
        }
    }

    fn flavour_options() -> Vec<ariadne_api::permissions::FlavourOptionsDto> {
        Flavour::ALL
            .into_iter()
            .map(|flavour| ariadne_api::permissions::FlavourOptionsDto {
                flavour,
                devices: Device::ALL
                    .into_iter()
                    .map(|device| ariadne_api::permissions::DeviceOptionDto {
                        device,
                        can_run: flavour == Flavour::Kev4B && device == Device::Mlx,
                        reason: (flavour != Flavour::Kev4B || device != Device::Mlx)
                            .then(|| "needs 24 GB RAM, found 8 GB".to_string()),
                        slow: device == Device::Cpu && flavour != Flavour::Kev08B,
                    })
                    .collect(),
            })
            .collect()
    }

    fn learned() -> LearnedPermissionDto {
        LearnedPermissionDto {
            id: "01J00000000000000000000001".into(),
            repository_id: "01J00000000000000000000002".into(),
            tool_name: "Bash".into(),
            key: r#"{"command":"git show <HASH>"}"#.into(),
            level: ariadne_api::permissions::LearnedPermissionLevel::Command,
            family: "git show".into(),
            risk_tags: vec!["remote".into(), "force".into()],
            scope: ariadne_api::permissions::LearnedPermissionScope::Repository,
            tool_call: json!({"toolCallId": "call-1",
                              "rawInput": {"command": "git show 94f07c0b 2>&1 | tail -5"}}),
            options: json!([{"optionId": "yes", "kind": "allow_once"}]),
            selected_option: "yes".into(),
            target: ariadne_api::permissions::LearnedPermissionTarget::Ai,
            output: Some(json!({"label": "ask", "danger": 0.41})),
            created_at: "2026-09-28T00:00:00Z".into(),
            updated_at: "2026-09-29T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn learned_verbs_work_with_human_and_json_output() {
        async fn list() -> Json<ariadne_api::permissions::LearnedPermissionsResponse> {
            Json(ariadne_api::permissions::LearnedPermissionsResponse {
                items: vec![learned()],
            })
        }
        async fn one() -> Json<LearnedPermissionDto> {
            Json(learned())
        }
        async fn gone() -> StatusCode {
            StatusCode::NO_CONTENT
        }
        let app = Router::new()
            .route("/v1/permissions/learned", get(list))
            .route("/v1/permissions/learned/{id}", get(one).delete(gone));
        let (client, server) = serve(app).await;
        let id = "01J00000000000000000000001".to_string();
        for format in [Format::Table, Format::Json] {
            for command in [
                LearnedPermissionsCommand::List { repo: None },
                LearnedPermissionsCommand::Show { id: id.clone() },
                LearnedPermissionsCommand::Rm { id: id.clone() },
            ] {
                run(&client, PermissionsCommand::Learned(command), format)
                    .await
                    .unwrap();
            }
        }
        server.abort();
    }

    /// `list` shows the level, the family, the key and the scope, then the
    /// target and the selected option, beside the tool; `show` prints every
    /// field with the JSON fields pretty-printed, the raw input included.
    #[test]
    fn learned_list_and_show_print_the_new_fields() {
        let row = learned();
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T00:00:00Z")
            .unwrap()
            .to_utc();
        let listed = learned_row(&row, now);
        assert_eq!(
            LEARNED_LIST
                .iter()
                .map(|column| column.header)
                .collect::<Vec<_>>(),
            [
                "id",
                "repository",
                "tool",
                "level",
                "family",
                "key",
                "scope",
                "target",
                "selected",
                "created",
                "updated"
            ]
        );
        assert_eq!(
            listed[2..9],
            [
                "Bash",
                "command",
                "git show",
                r#"{"command":"git show <HASH>"}"#,
                "repository",
                "ai",
                "yes"
            ]
        );

        let text = crate::output::kv_block(&learned_fields(&row), &crate::output::View::plain());
        for expected in [
            "01J00000000000000000000001",
            "01J00000000000000000000002",
            "level",
            "command",
            "family",
            "git show",
            "key",
            r#"{"command":"git show <HASH>"}"#,
            "risk tags",
            "remote, force",
            "scope",
            "repository",
            "target",
            "selected option",
            "\"toolCallId\": \"call-1\"",
            "\"command\": \"git show 94f07c0b 2>&1 | tail -5\"",
            "\"kind\": \"allow_once\"",
            "\"danger\": 0.41",
            "created",
            "updated",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from:\n{text}"
            );
        }
    }

    #[test]
    fn threshold_is_a_number_between_zero_and_one() {
        assert_eq!(parse_threshold("0.6"), Ok(0.6));
        assert!(
            parse_threshold("1.5")
                .unwrap_err()
                .contains("between 0 and 1")
        );
        assert!(parse_threshold("abc").unwrap_err().contains("not a number"));
    }

    #[test]
    fn test_prints_the_score_line_and_names_no_answer() {
        let answered = TestAiPermissionResponse {
            label: Some("ask".into()),
            danger: Some(0.21),
            allow_threshold: 0.0531,
            deny_threshold: 0.6522,
            ai_error: None,
            operation: Some("read_workspace".into()),
            risk_tags: Some(vec![]),
            cap: None,
            probabilities: Some(serde_json::json!({"0": 0.62, "1": 0.33, "2": 0.05})),
        };
        assert_eq!(
            test_one_line(&answered),
            "ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%); operation read_workspace"
        );
        let mut capped = answered.clone();
        capped.danger = Some(0.02);
        capped.probabilities = Some(serde_json::json!({"0": 0.96, "1": 0.03, "2": 0.01}));
        capped.operation = Some("destructive_or_exfiltration".into());
        capped.risk_tags = Some(vec!["reviewer_directive".into()]);
        capped.cap = Some("reviewer_directive".into());
        assert_eq!(
            test_one_line(&capped),
            "ask · allow 96%, deny 1% · danger 2% (allow up to 5%, deny from 65%) · tags: reviewer_directive · capped by reviewer_directive; operation destructive_or_exfiltration"
        );
        assert_eq!(
            test_one_line(&TestAiPermissionResponse {
                label: None,
                danger: None,
                allow_threshold: 0.2,
                deny_threshold: 0.8,
                ai_error: Some("timed out".into()),
                operation: None,
                risk_tags: Some(vec![]),
                cap: None,
                probabilities: None,
            }),
            "no answer: timed out"
        );
    }

    /// Every field of the status shows up in the block, in words a person
    /// reads rather than the wire's own spelling: python names its path and
    /// version together, and the hardware names the OS, the RAM and the GPU.
    #[test]
    fn show_renders_every_field() {
        let text = crate::output::kv_block(
            &fields(&status(AiPermissionsState::Ready)),
            &crate::output::View::plain(),
        );
        for expected in [
            "yes",
            "ready",
            "3.12.1 at /usr/bin/python3",
            "0.8",
            "4b",
            "mlx",
            "macos aarch64",
            "64 GB RAM",
            "v0.1.4",
            "http://127.0.0.1:8900",
            "last refresh",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from:\n{text}"
            );
        }

        let missing_python = AiPermissionsStatusDto {
            python: PythonDto {
                path: None,
                version: None,
                ok: false,
            },
            last_refresh_at: None,
            ..status(AiPermissionsState::Disabled)
        };
        let text = crate::output::kv_block(&fields(&missing_python), &crate::output::View::plain());
        assert!(text.contains("not found"), "{text}");
        assert!(text.contains("never"), "{text}");
    }

    #[test]
    fn show_omits_the_built_in_configuration() {
        let status = AiPermissionsStatusDto {
            last_refresh_at: None,
            ..status(AiPermissionsState::Ready)
        };
        assert_eq!(
            crate::output::kv_block(&fields(&status), &crate::output::View::plain()),
            "enabled            yes\nstate              ● ready\npython             3.12.1 at /usr/bin/python3\nallow threshold    0.2\ndeny threshold     0.8\nflavour            4b\ndevice             mlx\nhardware           macos aarch64, 64 GB RAM, no GPU\ninstalled release  v0.1.4\nlatest release     v0.1.4\nweights            yes\nendpoint           http://127.0.0.1:8900\nlast refresh       never\nlast error         -"
        );
    }

    /// The flavour and device table lists every combination, unavailable
    /// ones included, with the reason and the slow note.
    #[test]
    fn flavour_rows_lists_every_flavour_and_device() {
        let rows = flavour_rows(&status(AiPermissionsState::Ready));
        assert_eq!(rows.len(), 4 * 3);
        let mlx_4b = rows
            .iter()
            .find(|row| row[0] == "4b" && row[1] == "mlx")
            .unwrap();
        assert_eq!(mlx_4b[2], "yes", "{mlx_4b:?}");
        let cuda_4b = rows
            .iter()
            .find(|row| row[0] == "4b" && row[1] == "cuda")
            .unwrap();
        assert_eq!(cuda_4b[2], "no", "{cuda_4b:?}");
        assert_eq!(cuda_4b[3], "needs 24 GB RAM, found 8 GB");
    }

    /// One route, capturing every body it is sent — what every body-shape
    /// test below reads back.
    async fn capturing_put() -> (
        Client,
        tokio::task::JoinHandle<()>,
        Arc<Mutex<Vec<serde_json::Value>>>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        // The raw body, not `UpdateAiPermissionsRequest` round-tripped back through
        // its own `Serialize`: that would fill in the very fields — absent
        // on the wire — this test exists to catch.
        async fn put_handler(
            State(seen): State<Arc<Mutex<Vec<serde_json::Value>>>>,
            Json(req): Json<serde_json::Value>,
        ) -> Json<AiPermissionsStatusDto> {
            seen.lock().unwrap().push(req);
            Json(status(AiPermissionsState::Ready))
        }
        let app = Router::new()
            .route("/v1/permissions/ai", put(put_handler))
            .with_state(seen.clone());
        let (client, server) = serve(app).await;
        (client, server, seen)
    }

    async fn serve(app: Router) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), server)
    }

    #[tokio::test]
    async fn test_sends_the_workspace_with_the_request() {
        let seen = Arc::new(Mutex::new(None));
        async fn handler(
            State(seen): State<Arc<Mutex<Option<serde_json::Value>>>>,
            Json(req): Json<serde_json::Value>,
        ) -> Json<TestAiPermissionResponse> {
            *seen.lock().unwrap() = Some(req);
            Json(TestAiPermissionResponse {
                label: Some("allow".into()),
                danger: Some(0.05),
                allow_threshold: 0.1647,
                deny_threshold: 0.626,
                ai_error: None,
                operation: Some("read_workspace".into()),
                risk_tags: Some(vec![]),
                cap: None,
                probabilities: Some(json!({"0": 0.9, "1": 0.1, "2": 0.0})),
            })
        }
        let app = Router::new()
            .route("/v1/permissions/ai/test", post(handler))
            .with_state(seen.clone());
        let (client, server) = serve(app).await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Test {
                tool: "Bash".into(),
                kind: Some("execute".into()),
                input: json!({"command": "git status"}),
                options: vec!["Allow".into()],
                workspace: Some("/repo/ariadne".into()),
            }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(
            seen.lock().unwrap().as_ref().unwrap()["workspace"],
            "/repo/ariadne"
        );
    }

    /// `enable` sends `{"enabled": true}` and nothing else.
    /// or threshold key at all, `null` or otherwise.
    #[tokio::test]
    async fn enable_sends_enabled_true_and_nothing_else() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Enable { wait: false }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(seen.lock().unwrap()[0], json!({"enabled": true}));
    }

    /// `set --flavour` alone sends `{"flavour": "9b"}` and nothing else.
    #[tokio::test]
    async fn set_flavour_sends_the_flavour_alone() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: None,
                deny_threshold: None,
                flavour: Some(Flavour::Kev9B),
                device: None,
            }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(seen.lock().unwrap()[0], json!({"flavour": "9b"}));
    }

    /// `set --flavour --device` sends both fields and nothing else.
    #[tokio::test]
    async fn set_flavour_and_device_sends_both_fields() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: None,
                deny_threshold: None,
                flavour: Some(Flavour::Kev08B),
                device: Some(Device::Cpu),
            }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(
            seen.lock().unwrap()[0],
            json!({"flavour": "0.8b", "device": "cpu"})
        );
    }

    /// The daemon's refusal of an unsupported combination survives whole.
    #[tokio::test]
    async fn set_prints_the_daemons_flavour_unsupported_refusal() {
        async fn refused() -> (StatusCode, Json<ErrorBody>) {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorBody::new(
                    "flavour_unsupported",
                    "needs 24 GB VRAM, found 8 GB",
                )),
            )
        }
        let app = Router::new().route("/v1/permissions/ai", put(refused));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: None,
                deny_threshold: None,
                flavour: Some(Flavour::Kev9B),
                device: Some(Device::Cuda),
            }),
            Format::Table,
        )
        .await
        .unwrap_err();
        server.abort();

        assert_eq!(
            err.downcast_ref::<ClientError>().unwrap().human(),
            "needs 24 GB VRAM, found 8 GB"
        );
    }

    /// Both threshold flags send both fields and nothing else.
    #[tokio::test]
    async fn set_thresholds_sends_both_fields_and_nothing_else() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: Some(0.2),
                deny_threshold: Some(0.8),
                flavour: None,
                device: None,
            }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(
            seen.lock().unwrap()[0],
            json!({"allow_threshold": 0.2, "deny_threshold": 0.8})
        );
    }

    /// The relationship between both settings depends on the stored value
    /// when either flag is absent, so the daemon validates it.
    #[tokio::test]
    async fn set_prints_the_daemons_threshold_pair_error() {
        async fn refused() -> (StatusCode, Json<ErrorBody>) {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrorBody::new(
                    "invalid_request",
                    "allow threshold must be less than deny threshold, not 0.8 and 0.8",
                )),
            )
        }
        let app = Router::new().route("/v1/permissions/ai", put(refused));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Set {
                allow_threshold: Some(0.8),
                deny_threshold: Some(0.8),
                flavour: None,
                device: None,
            }),
            Format::Table,
        )
        .await
        .unwrap_err();
        server.abort();

        assert_eq!(
            err.downcast_ref::<ClientError>().unwrap().human(),
            "allow threshold must be less than deny threshold, not 0.8 and 0.8"
        );
    }

    /// The daemon's message survives whole, and `ai_disabled` picks up the
    /// hint that names the command which fixes it.
    #[tokio::test]
    async fn refresh_keeps_the_daemons_message_and_adds_the_hint_on_ai_disabled() {
        async fn refused() -> (StatusCode, Json<ErrorBody>) {
            (
                StatusCode::CONFLICT,
                Json(ErrorBody::new(
                    "ai_disabled",
                    "the AI permission model is off; turn it on before refreshing it",
                )),
            )
        }
        let app = Router::new().route("/v1/permissions/ai/refresh", post(refused));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Refresh { wait: false }),
            Format::Table,
        )
        .await
        .unwrap_err();
        server.abort();

        let client_error = err.downcast_ref::<ClientError>().unwrap();
        assert_eq!(
            client_error.human(),
            "the AI permission model is off; turn it on before refreshing it"
        );
        assert_eq!(
            crate::error::human_line(&err),
            "the AI permission model is off; turn it on before refreshing it (run ariadne permissions ai enable)"
        );
    }

    /// `enable --wait` blocks on the event stream until `ai_permissions_updated`
    /// leaves `installing`, and returns the settled status.
    #[tokio::test]
    async fn enable_wait_returns_once_the_stream_answers_ready() {
        async fn put_installing() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Installing))
        }
        async fn get_installing() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Installing))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            let ready = Event::default()
                .event("ai_permissions_updated")
                .data(serde_json::to_string(&status(AiPermissionsState::Ready)).unwrap());
            Sse::new(stream::once(async move { Ok(ready) }))
        }
        let app = Router::new()
            .route(
                "/v1/permissions/ai",
                put(put_installing).get(get_installing),
            )
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Enable { wait: true }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();
    }

    /// `enable --wait` exits with the daemon's own failure message once the
    /// stream reports `failed`.
    #[tokio::test]
    async fn enable_wait_fails_with_the_last_error_on_a_failed_install() {
        async fn put_installing() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Installing))
        }
        async fn get_installing() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Installing))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            let failed = AiPermissionsStatusDto {
                last_error: Some("pip install failed: no matching distribution".into()),
                ..status(AiPermissionsState::Failed)
            };
            let frame = Event::default()
                .event("ai_permissions_updated")
                .data(serde_json::to_string(&failed).unwrap());
            Sse::new(stream::once(async move { Ok(frame) }))
        }
        let app = Router::new()
            .route(
                "/v1/permissions/ai",
                put(put_installing).get(get_installing),
            )
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Enable { wait: true }),
            Format::Json,
        )
        .await
        .unwrap_err();
        server.abort();

        assert_eq!(
            err.to_string(),
            "pip install failed: no matching distribution"
        );
    }

    /// The install can finish between the `PUT`'s own `installing` answer
    /// and this connection opening — the daemon's stream carries no replay,
    /// so an event published before a subscriber connects reaches nobody.
    /// The status read right after subscribing is what catches it: this
    /// stream never sends a single frame, and `--wait` still returns.
    #[tokio::test]
    async fn enable_wait_refetches_status_for_an_install_that_already_settled() {
        async fn put_installing() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Installing))
        }
        async fn get_ready() -> Json<AiPermissionsStatusDto> {
            Json(status(AiPermissionsState::Ready))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            Sse::new(stream::pending())
        }
        let app = Router::new()
            .route("/v1/permissions/ai", put(put_installing).get(get_ready))
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        run(
            &client,
            PermissionsCommand::Ai(AiPermissionsCommand::Enable { wait: true }),
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();
    }
}
