//! `ariadne permissions ...` — the `ai` permission mode's model, Laya (022).

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::permissions::{
    LayaCheckpoints, LayaState, LayaStatusDto, PythonDto, UpdateLayaRequest,
};
use ariadne_client::Client;

use crate::output::{Format, Kv, age, dash, print, print_kv, style, view, yes_no};

/// Which checkpoints `--checkpoints` names, in the spelling clap takes: the
/// wire spelling `ariadne_api::permissions::LayaCheckpoints` already carries
/// is not a `clap::ValueEnum`, so this is the CLI's own name for the same
/// two values.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum Checkpoints {
    English,
    All,
}

impl Checkpoints {
    fn as_api(self) -> LayaCheckpoints {
        match self {
            Checkpoints::English => LayaCheckpoints::English,
            Checkpoints::All => LayaCheckpoints::All,
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum PermissionsCommand {
    /// Show the Laya settings and where the install has got to
    Show,
    /// Turn Laya on and start the install
    Enable {
        /// Wait until the install leaves "installing"
        #[arg(long)]
        wait: bool,
    },
    /// Turn Laya off; the install stays on disk
    Disable,
    /// Run the install again, on the settings as they stand
    Refresh {
        /// Wait until the install leaves "installing"
        #[arg(long)]
        wait: bool,
    },
    /// Change the checkpoints, the threshold or the daily refresh
    #[command(group = clap::ArgGroup::new("laya-set")
        .args(["checkpoints", "threshold", "schedule", "no_schedule"])
        .required(true)
        .multiple(true))]
    Set {
        /// Which checkpoints to install: english (843 MB) or all (2.4 GB)
        #[arg(long, value_enum)]
        checkpoints: Option<Checkpoints>,
        /// How sure Laya has to be before its answer is taken, 0 to 1
        #[arg(long, value_parser = parse_threshold)]
        threshold: Option<f64>,
        /// When the daily refresh runs, HH:MM in 24-hour local time
        #[arg(long, value_parser = parse_schedule, conflicts_with = "no_schedule")]
        schedule: Option<String>,
        /// Turn the daily refresh off
        #[arg(long, conflicts_with = "schedule")]
        no_schedule: bool,
    },
}

pub(crate) async fn run(client: &Client, cmd: PermissionsCommand, format: Format) -> Result<()> {
    match cmd {
        PermissionsCommand::Show => show(client, format).await,
        PermissionsCommand::Enable { wait } => {
            settle(
                client,
                format,
                wait,
                client
                    .update_laya(&UpdateLayaRequest {
                        enabled: Some(true),
                        ..Default::default()
                    })
                    .await?,
            )
            .await
        }
        PermissionsCommand::Disable => {
            let status = client
                .update_laya(&UpdateLayaRequest {
                    enabled: Some(false),
                    ..Default::default()
                })
                .await?;
            print_result(format, &status)
        }
        PermissionsCommand::Refresh { wait } => {
            settle(client, format, wait, client.refresh_laya().await?).await
        }
        PermissionsCommand::Set {
            checkpoints,
            threshold,
            schedule,
            no_schedule,
        } => {
            let status = client
                .update_laya(&UpdateLayaRequest {
                    enabled: None,
                    checkpoints: checkpoints.map(Checkpoints::as_api),
                    threshold,
                    schedule: schedule_field(schedule, no_schedule),
                })
                .await?;
            print_result(format, &status)
        }
    }
}

/// The wire shape of a partial `schedule`: absent when neither flag was
/// given, `Some(Some(_))` for `--schedule`, `Some(None)` for `--no-schedule`
/// — clap's own `ArgGroup` has already refused both together.
fn schedule_field(schedule: Option<String>, no_schedule: bool) -> Option<Option<String>> {
    match (schedule, no_schedule) {
        (Some(hhmm), _) => Some(Some(hhmm)),
        (None, true) => Some(None),
        (None, false) => None,
    }
}

async fn show(client: &Client, format: Format) -> Result<()> {
    let status = client.laya_status().await?;
    print(format, &status, || print_kv(&fields(&status)))
}

/// `--wait` on `enable` and `refresh`: neither answers with anything but
/// `installing` once it has actually started an install, so there is nothing
/// to wait for when the settings did not change.
async fn settle(client: &Client, format: Format, wait: bool, status: LayaStatusDto) -> Result<()> {
    if !wait {
        return print_result(format, &status);
    }
    let status = wait_for_settled(client, status).await?;
    if status.state == LayaState::Failed {
        anyhow::bail!(
            status
                .last_error
                .clone()
                .unwrap_or_else(|| "the install failed".to_string())
        );
    }
    print_result(format, &status)
}

/// Block until `state` leaves `installing`, by following `laya_updated` on
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
async fn wait_for_settled(client: &Client, initial: LayaStatusDto) -> Result<LayaStatusDto> {
    if initial.state != LayaState::Installing {
        return Ok(initial);
    }
    let mut stream = client.stream("/v1/events/stream").await?;
    let status = client.laya_status().await?;
    if status.state != LayaState::Installing {
        return Ok(status);
    }
    while let Some(frame) = stream.next().await {
        let frame = frame?;
        if frame.event == "laya_updated" {
            let status: LayaStatusDto = serde_json::from_str(&frame.data)?;
            if status.state != LayaState::Installing {
                return Ok(status);
            }
        }
    }
    let status = client.laya_status().await?;
    if status.state == LayaState::Installing {
        anyhow::bail!("the connection ended before the install finished");
    }
    Ok(status)
}

fn print_result(format: Format, status: &LayaStatusDto) -> Result<()> {
    print(format, status, || println!("{}", one_line(status)))
}

/// The one line a mutation ends on: `laya is now <state>`, coloured and
/// glyphed exactly as `ariadne events` paints the same word in a stream.
fn one_line(status: &LayaStatusDto) -> String {
    let color = view().color;
    let word = status.state.as_str();
    let (sty, glyph) = style::status(word);
    let painted = match (color, glyph) {
        (true, Some(glyph)) => format!("{glyph} {word}"),
        _ => word.to_string(),
    };
    format!("laya is now {}", style::paint(color, sty, &painted))
}

/// `show`'s key/value block: every field of the status, in the order the
/// ticket lists them.
fn fields(status: &LayaStatusDto) -> Vec<(&'static str, Kv)> {
    vec![
        ("enabled", yes_no(status.enabled, "no").into()),
        ("state", Kv::status(status.state.as_str())),
        ("python", python_field(&status.python).into()),
        (
            "checkpoints",
            status.checkpoints.as_str().to_string().into(),
        ),
        ("threshold", status.threshold.to_string().into()),
        (
            "schedule",
            status
                .schedule
                .clone()
                .unwrap_or_else(|| "off".into())
                .into(),
        ),
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

fn last_refresh(at: Option<&str>) -> Kv {
    match at {
        Some(rfc3339) => age(rfc3339, chrono::Utc::now()).into(),
        None => "never".into(),
    }
}

/// `--threshold`, refused locally before anything is sent: the daemon would
/// refuse the same value with a 422, but a round trip is not needed to know
/// 0 to 1 from a typo.
fn parse_threshold(s: &str) -> Result<f64, String> {
    let value: f64 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if !(0.0..=1.0).contains(&value) {
        return Err(format!("threshold must be between 0 and 1, not {value}"));
    }
    Ok(value)
}

/// `--schedule`, refused locally before anything is sent: two digits, a
/// colon, two digits — the same shape the daemon checks, kept in step with
/// `http/permissions.rs::is_clock_time` by hand since the two sides of the
/// wire do not share code.
fn parse_schedule(s: &str) -> Result<String, String> {
    match is_clock_time(s) {
        true => Ok(s.to_string()),
        false => Err(format!("schedule must be HH:MM in 24-hour time, not `{s}`")),
    }
}

fn is_clock_time(schedule: &str) -> bool {
    let Some((hours, minutes)) = schedule.split_once(':') else {
        return false;
    };
    let two_digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
    two_digits(hours)
        && two_digits(minutes)
        && hours.parse::<u32>().is_ok_and(|h| h < 24)
        && minutes.parse::<u32>().is_ok_and(|m| m < 60)
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

    fn status(state: LayaState) -> LayaStatusDto {
        LayaStatusDto {
            enabled: true,
            checkpoints: LayaCheckpoints::English,
            threshold: 0.8,
            schedule: None,
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
    fn a_schedule_is_two_digits_a_colon_and_two_digits() {
        assert_eq!(parse_schedule("03:30"), Ok("03:30".to_string()));
        assert!(parse_schedule("25:00").unwrap_err().contains("HH:MM"));
    }

    /// Every field of the status shows up in the block, in words a person
    /// reads rather than the wire's own spelling: python names its path and
    /// version together, an unset schedule reads "off".
    #[test]
    fn show_renders_every_field() {
        let text = crate::output::kv_block(
            &fields(&status(LayaState::Ready)),
            &crate::output::View::plain(),
        );
        for expected in [
            "yes",
            "ready",
            "3.12.1 at /usr/bin/python3",
            "english",
            "0.8",
            "off",
            "v0.1.4",
            "http://127.0.0.1:8900",
            "last refresh",
        ] {
            assert!(
                text.contains(expected),
                "{expected:?} missing from:\n{text}"
            );
        }

        let missing_python = LayaStatusDto {
            python: PythonDto {
                path: None,
                version: None,
                ok: false,
            },
            last_refresh_at: None,
            ..status(LayaState::Disabled)
        };
        let text = crate::output::kv_block(&fields(&missing_python), &crate::output::View::plain());
        assert!(text.contains("not found"), "{text}");
        assert!(text.contains("never"), "{text}");
    }

    /// One route, capturing every body it is sent — what every body-shape
    /// test below reads back.
    async fn capturing_put() -> (
        Client,
        tokio::task::JoinHandle<()>,
        Arc<Mutex<Vec<serde_json::Value>>>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        // The raw body, not `UpdateLayaRequest` round-tripped back through
        // its own `Serialize`: that would fill in the very fields — absent
        // on the wire — this test exists to catch.
        async fn put_handler(
            State(seen): State<Arc<Mutex<Vec<serde_json::Value>>>>,
            Json(req): Json<serde_json::Value>,
        ) -> Json<LayaStatusDto> {
            seen.lock().unwrap().push(req);
            Json(status(LayaState::Ready))
        }
        let app = Router::new()
            .route("/v1/permissions/laya", put(put_handler))
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

    /// `enable` sends `{"enabled": true}` and nothing else: no `checkpoints`
    /// or `threshold` key at all, `null` or otherwise.
    #[tokio::test]
    async fn enable_sends_enabled_true_and_nothing_else() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Enable { wait: false },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(seen.lock().unwrap()[0], json!({"enabled": true}));
    }

    /// `set --no-schedule` sends `{"schedule": null}` alone.
    #[tokio::test]
    async fn set_no_schedule_sends_a_null_schedule() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Set {
                checkpoints: None,
                threshold: None,
                schedule: None,
                no_schedule: true,
            },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(seen.lock().unwrap()[0], json!({"schedule": null}));
    }

    /// `set --threshold 0.6` sends `threshold` alone.
    #[tokio::test]
    async fn set_threshold_sends_only_threshold() {
        let (client, server, seen) = capturing_put().await;

        run(
            &client,
            PermissionsCommand::Set {
                checkpoints: None,
                threshold: Some(0.6),
                schedule: None,
                no_schedule: false,
            },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();

        assert_eq!(seen.lock().unwrap()[0], json!({"threshold": 0.6}));
    }

    /// The daemon's message survives whole, and `laya_disabled` picks up the
    /// hint that names the command which fixes it.
    #[tokio::test]
    async fn refresh_keeps_the_daemons_message_and_adds_the_hint_on_laya_disabled() {
        async fn refused() -> (StatusCode, Json<ErrorBody>) {
            (
                StatusCode::CONFLICT,
                Json(ErrorBody::new(
                    "laya_disabled",
                    "Laya is off; turn it on before refreshing it",
                )),
            )
        }
        let app = Router::new().route("/v1/permissions/laya/refresh", post(refused));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Refresh { wait: false },
            Format::Table,
        )
        .await
        .unwrap_err();
        server.abort();

        let client_error = err.downcast_ref::<ClientError>().unwrap();
        assert_eq!(
            client_error.human(),
            "Laya is off; turn it on before refreshing it"
        );
        assert_eq!(
            crate::error::human_line(&err),
            "Laya is off; turn it on before refreshing it (run ariadne permissions enable)"
        );
    }

    /// `enable --wait` blocks on the event stream until `laya_updated`
    /// leaves `installing`, and returns the settled status.
    #[tokio::test]
    async fn enable_wait_returns_once_the_stream_answers_ready() {
        async fn put_installing() -> Json<LayaStatusDto> {
            Json(status(LayaState::Installing))
        }
        async fn get_installing() -> Json<LayaStatusDto> {
            Json(status(LayaState::Installing))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            let ready = Event::default()
                .event("laya_updated")
                .data(serde_json::to_string(&status(LayaState::Ready)).unwrap());
            Sse::new(stream::once(async move { Ok(ready) }))
        }
        let app = Router::new()
            .route(
                "/v1/permissions/laya",
                put(put_installing).get(get_installing),
            )
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        run(
            &client,
            PermissionsCommand::Enable { wait: true },
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
        async fn put_installing() -> Json<LayaStatusDto> {
            Json(status(LayaState::Installing))
        }
        async fn get_installing() -> Json<LayaStatusDto> {
            Json(status(LayaState::Installing))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            let failed = LayaStatusDto {
                last_error: Some("pip install failed: no matching distribution".into()),
                ..status(LayaState::Failed)
            };
            let frame = Event::default()
                .event("laya_updated")
                .data(serde_json::to_string(&failed).unwrap());
            Sse::new(stream::once(async move { Ok(frame) }))
        }
        let app = Router::new()
            .route(
                "/v1/permissions/laya",
                put(put_installing).get(get_installing),
            )
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        let err = run(
            &client,
            PermissionsCommand::Enable { wait: true },
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
        async fn put_installing() -> Json<LayaStatusDto> {
            Json(status(LayaState::Installing))
        }
        async fn get_ready() -> Json<LayaStatusDto> {
            Json(status(LayaState::Ready))
        }
        async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
            Sse::new(stream::pending())
        }
        let app = Router::new()
            .route("/v1/permissions/laya", put(put_installing).get(get_ready))
            .route("/v1/events/stream", get(events));
        let (client, server) = serve(app).await;

        run(
            &client,
            PermissionsCommand::Enable { wait: true },
            Format::Json,
        )
        .await
        .unwrap();
        server.abort();
    }
}
