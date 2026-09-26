//! Integration tests for the AI permission settings, the Python check and the install
//! behind `/v1/permissions/ai` (022).
//!
//! Nothing here downloads anything. The release document is served by the
//! test, the whole install is one shell script the daemon runs in place of
//! the venv, the wheel and the checkpoints, and the interpreter is a script
//! that prints a version. What is proved is the daemon's own rules: what it
//! refuses, what it starts, what it writes down and what it says on the
//! stream.

use crate::common;

use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;

use ariadne_api::error::ErrorBody;
use ariadne_api::permissions::{AiPermissionsState, AiPermissionsStatusDto};
use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_core::PermissionMode;
use ariadne_daemon::ai_permissions::AiPermissions;
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::Store;

use common::{
    Harness, HarnessBuilder, TIMEOUT, delete, eventually, harness, next_event, post, post_json,
    put_json, shared_script,
};

/// The release the served document names, and the wheel on it.
const TAG: &str = "v0.1.4";
const WHEEL: &str = "model-0.1.4-py3-none-any.whl";

// -- the stubs ---------------------------------------------------------------

/// An interpreter that prints `version` and nothing else, the way `python3
/// --version` does.
fn python_printing(version: &str) -> String {
    shared_script(&format!("#!/bin/sh\necho 'Python {version}'\n"))
        .display()
        .to_string()
}

/// The script that stands in for the whole install.
///
/// `$1` is where it writes the environment the daemon handed it, one value a
/// line. `$2`, when it is not empty, is a file it waits for before it ends —
/// so a test can hold an install open without waiting on a clock. `$3` is the
/// status it exits with; anything but `0` prints a line on stderr first,
/// which is what `last_error` ends up carrying.
fn installer(record: &std::path::Path, hold: &str, status: &str) -> Vec<String> {
    let script = shared_script(
        "#!/bin/sh\n\
         printf '%s\\n%s\\n%s\\n%s\\n' \
           \"$LAYA_HOME\" \"$LAYA_CHECKPOINTS\" \"$LAYA_WHEEL_URL\" \"$LAYA_RELEASE\" > \"$1\"\n\
         if [ -n \"$2\" ]; then\n\
           while [ ! -f \"$2\" ]; do sleep 0.05; done\n\
         fi\n\
         if [ \"$3\" != \"0\" ]; then\n\
           echo 'the wheel could not be installed' >&2\n\
         fi\n\
         exit \"$3\"\n",
    );
    vec![
        script.display().to_string(),
        record.display().to_string(),
        hold.to_string(),
        status.to_string(),
    ]
}

/// What the installer recorded: `LAYA_HOME`, `LAYA_CHECKPOINTS`,
/// `LAYA_WHEEL_URL` and `LAYA_RELEASE`, in that order.
fn recorded(record: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(record)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A release document served over loopback, with the tarball ahead of the
/// wheel so the pick is a pick and not the first asset.
struct ReleaseServer {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl ReleaseServer {
    async fn start() -> Self {
        let document = json!({
            "tag_name": TAG,
            "assets": [
                {"browser_download_url": "https://example.test/model-0.1.4.tar.gz"},
                {"browser_download_url": format!("https://example.test/{WHEEL}")},
            ],
        });
        let app = axum::Router::new().route(
            "/release.json",
            axum::routing::get(move || {
                let document = document.clone();
                async move { axum::Json(document) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/release.json", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, task }
    }
}

impl Drop for ReleaseServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// -- the harness -------------------------------------------------------------

/// A daemon whose Python is new enough and whose release document is the
/// served one. The install itself is the caller's to configure.
fn with_ai_permissions(python: &str, release: &ReleaseServer) -> HarnessBuilder {
    harness()
        .python_bin(python_printing(python))
        .ai_permissions_release_url(release.url.clone())
}

async fn status(h: &Harness) -> AiPermissionsStatusDto {
    h.get("/v1/permissions/ai").await
}

fn update(body: serde_json::Value) -> axum::http::Request<Body> {
    put_json("/v1/permissions/ai", body)
}

/// Wait for the install to settle on `state`, which is where it stops.
async fn settles_on(h: &Harness, state: AiPermissionsState) -> AiPermissionsStatusDto {
    eventually(TIMEOUT, "the install to settle", || async {
        status(h).await.state == state
    })
    .await;
    status(h).await
}

// -- the tests ---------------------------------------------------------------

/// A daemon nobody has configured answers with the defaults, and with the
/// interpreter it probed: off, the threshold the schema
/// carries, and no daily refresh.
#[tokio::test]
async fn the_settings_start_at_the_defaults_with_the_interpreter_probed() {
    let release = ReleaseServer::start().await;
    let python = python_printing("3.12.1");
    let h = with_ai_permissions("3.12.1", &release).await;

    let status = status(&h).await;
    assert!(!status.enabled);
    assert_eq!(status.threshold, 0.7);
    assert_eq!(status.schedule, None);
    assert_eq!(status.state, AiPermissionsState::Disabled);
    assert_eq!(status.installed_release, None);
    assert_eq!(status.latest_release, None);
    assert!(!status.weights_present);
    assert_eq!(status.endpoint, None);
    assert_eq!(status.last_refresh_at, None);
    assert_eq!(status.last_error, None);

    assert_eq!(status.python.path.as_deref(), Some(python.as_str()));
    assert_eq!(status.python.version.as_deref(), Some("3.12.1"));
    assert!(status.python.ok);
}

/// The model installs PyTorch into a virtual environment of the daemon's Python,
/// and needs 3.10 for it. Turning it on against an older one is refused
/// before anything is downloaded — and the refusal leaves the settings
/// exactly as they were, so nothing has to be undone.
#[tokio::test]
async fn turning_the_model_on_without_a_new_enough_python_is_refused_and_changes_nothing() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.9.18", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;

    let refused: ErrorBody = h
        .json(update(json!({"enabled": true})), StatusCode::CONFLICT)
        .await;
    assert_eq!(refused.error.code, "python_unavailable");
    assert!(refused.error.message.contains("3.9.18"), "{refused:?}");

    let after = status(&h).await;
    assert!(!after.enabled, "the refusal wrote nothing");
    assert_eq!(after.state, AiPermissionsState::Disabled);
    assert!(!after.python.ok);
    assert!(
        recorded(record.path()).is_empty(),
        "and nothing was installed"
    );
}

/// Turning the model on answers at once with `installing` — the install takes
/// minutes and gigabytes — and says on the stream when it is ready. What the
/// installer saw is the checkpoints the settings name and the wheel of the
/// release document, and what is written down afterwards is the tag it
/// installed.
#[tokio::test]
async fn turning_the_model_on_starts_the_install_and_reports_it_ready() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;
    let mut events = h.bus.subscribe();

    let started: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    assert!(started.enabled);
    assert_eq!(started.state, AiPermissionsState::Installing);
    assert_eq!(started.installed_release, None);

    let announced = next_event(
        &mut events,
        |e| matches!(&e.event, DomainEvent::AiPermissionsUpdated(l) if l.state == AiPermissionsState::Installing),
    )
    .await;
    let DomainEvent::AiPermissionsUpdated(installing) = announced.event else {
        unreachable!("the predicate matched an ai_permissions_updated")
    };
    assert!(installing.enabled);

    let ready = settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(ready.installed_release.as_deref(), Some(TAG));
    assert_eq!(ready.latest_release.as_deref(), Some(TAG));
    assert!(ready.weights_present);
    assert!(ready.last_refresh_at.is_some());
    assert_eq!(ready.last_error, None);

    assert_eq!(
        recorded(record.path()),
        [
            h.launcher
                .cfg
                .root
                .join("ai-permissions")
                .display()
                .to_string(),
            "typed-decisions".to_string(),
            format!("https://example.test/{WHEEL}"),
            TAG.to_string(),
        ]
    );

    // The end of the install reaches the stream too.
    next_event(
        &mut events,
        |e| matches!(&e.event, DomainEvent::AiPermissionsUpdated(l) if l.state == AiPermissionsState::Ready),
    )
    .await;
}

/// An install that fails says why and leaves the model on: the settings are the
/// user's choice, and a download that broke is not them changing their mind.
/// The earlier install, where there is one, is left on disk.
#[tokio::test]
async fn an_install_that_fails_keeps_the_model_on_and_says_why() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "7"))
        .await;

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    let failed = settles_on(&h, AiPermissionsState::Failed).await;
    assert!(
        failed.enabled,
        "a failed install is not the model turned off"
    );
    assert_eq!(
        failed.last_error.as_deref(),
        Some("the wheel could not be installed"),
        "the installer's own stderr is what the user is left to act on"
    );
    assert!(!failed.weights_present);
    assert_eq!(failed.installed_release, None);
}

/// The settings a user chooses are kept, refused where they are not a
/// threshold or a time, and read back by the daemon that comes up next: an
/// install that ran for minutes must not be forgotten by a restart.
#[tokio::test]
async fn the_settings_are_validated_and_survive_a_daemon_restart() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;

    let chosen: AiPermissionsStatusDto = h
        .json(
            update(json!({"threshold": 0.6, "schedule": "03:30"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(chosen.threshold, 0.6);
    assert_eq!(chosen.schedule.as_deref(), Some("03:30"));

    for bad in [json!({"threshold": 1.5}), json!({"schedule": "25:00"})] {
        let refused: ErrorBody = h
            .json(update(bad.clone()), StatusCode::UNPROCESSABLE_ENTITY)
            .await;
        assert_eq!(refused.error.code, "invalid_request", "{bad}");
    }
    let unchanged = status(&h).await;
    assert_eq!(unchanged.threshold, 0.6, "a refusal wrote nothing");
    assert_eq!(unchanged.schedule.as_deref(), Some("03:30"));

    // The daemon that comes up next reads the same settings.
    let restarted = Store::open(h.dir.path().join("test.db")).await.unwrap();
    let ai_permissions = AiPermissions::new(
        restarted,
        EventBus::default(),
        &h.launcher.cfg,
        Timeouts::default(),
    )
    .unwrap();
    let kept = ai_permissions.status().await;
    assert_eq!(kept.threshold, 0.6);
    assert_eq!(kept.schedule.as_deref(), Some("03:30"));

    // An absent schedule keeps it; a null turns it off.
    let untouched: AiPermissionsStatusDto = h
        .json(update(json!({"threshold": 0.7})), StatusCode::OK)
        .await;
    assert_eq!(untouched.schedule.as_deref(), Some("03:30"));
    let off: AiPermissionsStatusDto = h
        .json(update(json!({"schedule": null})), StatusCode::OK)
        .await;
    assert_eq!(off.schedule, None);
}

/// The old name of the settings is gone from the wire: nothing answers at the
/// route it had.
#[tokio::test]
async fn the_old_route_answers_404() {
    let h = harness().await;
    let old = axum::http::Request::get("/v1/permissions/laya")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.response(old).await.status(), StatusCode::NOT_FOUND);
}

/// A refresh runs the install again on the settings as they stand now. It is
/// refused while the AI permission model is off — there is nothing to refresh — and while an
/// install is running, because one runs at a time.
#[tokio::test]
async fn refresh_is_refused_while_the_model_is_off_or_busy_and_reruns_the_install() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
        .await;

    let off: ErrorBody = h
        .json(post("/v1/permissions/ai/refresh"), StatusCode::CONFLICT)
        .await;
    assert_eq!(off.error.code, "ai_disabled");

    // The installer holds until the file appears, so the daemon is busy for
    // as long as the test needs it to be, and not a moment by the clock.
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    let busy: ErrorBody = h
        .json(post("/v1/permissions/ai/refresh"), StatusCode::CONFLICT)
        .await;
    assert_eq!(busy.error.code, "ai_busy");

    std::fs::write(&hold, "go").unwrap();
    settles_on(&h, AiPermissionsState::Ready).await;
    std::fs::remove_file(&hold).unwrap();

    // A refresh of a ready model runs the installer again on the built-in checkpoint.
    std::fs::write(record.path(), "").unwrap();
    let again: AiPermissionsStatusDto = h
        .json(post("/v1/permissions/ai/refresh"), StatusCode::ACCEPTED)
        .await;
    assert_eq!(again.state, AiPermissionsState::Installing);
    std::fs::write(&hold, "go").unwrap();
    settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(
        recorded(record.path()).get(1).map(String::as_str),
        Some("typed-decisions")
    );
}

/// Turning the model off keeps every file: turning it back on is the release check
/// and nothing else, and the weights that took an hour stay where they are.
#[tokio::test]
async fn turning_the_model_off_keeps_the_files_it_installed() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    settles_on(&h, AiPermissionsState::Ready).await;

    let off: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    assert!(!off.enabled);
    assert_eq!(off.state, AiPermissionsState::Disabled);
    assert_eq!(off.installed_release.as_deref(), Some(TAG));
    assert!(off.weights_present, "the files are still on disk");
}

/// Turning the model off while an install runs does not stop the install, and the
/// install's end does not turn the model back on: what it put on disk is written
/// down, and the state stays `disabled`.
#[tokio::test]
async fn turning_the_model_off_during_an_install_keeps_it_off_when_the_install_ends() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
        .await;
    let mut events = h.bus.subscribe();

    let started: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    assert_eq!(started.state, AiPermissionsState::Installing);
    let off: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    assert_eq!(off.state, AiPermissionsState::Disabled);

    // The install ends: its last write is the one that names the release.
    std::fs::write(&hold, "go").unwrap();
    let ended = next_event(
        &mut events,
        |e| matches!(&e.event, DomainEvent::AiPermissionsUpdated(l) if l.installed_release.is_some()),
    )
    .await;
    let DomainEvent::AiPermissionsUpdated(ended) = ended.event else {
        unreachable!("the predicate matched an ai_permissions_updated")
    };
    assert!(!ended.enabled);
    assert_eq!(
        ended.state,
        AiPermissionsState::Disabled,
        "the install did not turn it on"
    );
    assert_eq!(ended.installed_release.as_deref(), Some(TAG));
    assert!(ended.weights_present, "what it put on disk is written down");
    assert_eq!(status(&h).await.state, AiPermissionsState::Disabled);
}

/// Turning the model off and on again while its install runs waits for that
/// install: the answer says `installing` again, and the install's end is
/// what the model is on.
#[tokio::test]
async fn turning_the_model_back_on_during_its_install_reports_that_install() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
        .await;

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    let again: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    assert!(again.enabled);
    assert_eq!(again.state, AiPermissionsState::Installing);

    std::fs::write(&hold, "go").unwrap();
    let ready = settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(ready.installed_release.as_deref(), Some(TAG));
}

/// The race of turning the model on while its install runs, taken in its bad
/// order: `update` has written `enabled = true` and found the install busy, a
/// turn-off lands, and only then does `update` write `installing`. That last
/// write must not move a model that is off, or nothing ever moves it back:
/// the install's end leaves a disabled row alone.
///
/// The test takes the three steps in that order itself, so the interleaving
/// is the same on every run.
#[tokio::test]
async fn a_turn_off_that_lands_before_the_rejoin_keeps_the_model_off() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
        .await;
    let mut events = h.bus.subscribe();

    let started: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    assert_eq!(started.state, AiPermissionsState::Installing);

    // The turn-off lands between `update`'s two writes …
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    // … and `update`'s busy-path write comes after it.
    let rejoined = h.state.ai_permissions.rejoin_install().await;
    assert!(!rejoined.enabled);
    assert_eq!(
        rejoined.state,
        AiPermissionsState::Disabled,
        "a model that is off is not put back to installing"
    );

    std::fs::write(&hold, "go").unwrap();
    let ended = next_event(
        &mut events,
        |e| matches!(&e.event, DomainEvent::AiPermissionsUpdated(l) if l.installed_release.is_some()),
    )
    .await;
    let DomainEvent::AiPermissionsUpdated(ended) = ended.event else {
        unreachable!("the predicate matched an ai_permissions_updated")
    };
    assert_eq!(
        ended.state,
        AiPermissionsState::Disabled,
        "and the install's end leaves it off, not stuck at installing"
    );
    assert_eq!(status(&h).await.state, AiPermissionsState::Disabled);
}

/// A local time on a January day, when no daylight-saving change is near.
fn local(time: &str) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    let naive = chrono::NaiveDate::from_ymd_opt(2026, 1, 10)
        .unwrap()
        .and_time(chrono::NaiveTime::parse_from_str(time, "%H:%M:%S").unwrap());
    chrono::Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap()
}

/// The daily refresh runs the install once, on the tick that passes its
/// minute, on the settings as they stand. It runs nothing on the next tick,
/// nothing while the AI permission model is off, and nothing without a schedule. The test says
/// what time it is, so nothing here waits on a clock.
#[tokio::test]
async fn the_daily_refresh_runs_the_install_once_at_its_minute() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    // The installer waits while this file is missing: it is there, so the
    // first install ends at once, and the refresh removes it to hold its own
    // install open while the test reads `installing`.
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("hold");
    std::fs::write(&hold, "").unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
        .await;
    let ai_permissions = &h.state.ai_permissions;
    let (before, over, after) = (local("03:29:40"), local("03:30:10"), local("03:30:40"));

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    settles_on(&h, AiPermissionsState::Ready).await;
    assert!(
        !ai_permissions.run_schedule(before, over).await,
        "no schedule, no refresh"
    );

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"schedule": "03:30"})), StatusCode::OK)
        .await;
    std::fs::write(record.path(), "").unwrap();
    assert!(!ai_permissions.run_schedule(local("03:29:00"), before).await);
    assert!(
        recorded(record.path()).is_empty(),
        "a tick before the minute runs nothing"
    );

    std::fs::remove_file(&hold).unwrap();
    assert!(ai_permissions.run_schedule(before, over).await);
    assert_eq!(status(&h).await.state, AiPermissionsState::Installing);
    std::fs::write(&hold, "").unwrap();
    settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(
        recorded(record.path()).get(1).map(String::as_str),
        Some("typed-decisions"),
        "the refresh installs the built-in checkpoint"
    );

    std::fs::write(record.path(), "").unwrap();
    assert!(
        !ai_permissions.run_schedule(over, after).await,
        "the tick after the minute runs nothing"
    );

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    assert!(
        !ai_permissions.run_schedule(before, over).await,
        "a model that is off is not refreshed"
    );
    assert!(recorded(record.path()).is_empty());
}

/// The `ai` mode asks the model, so a repository cannot be put into it while
/// there is no model to ask. It is refused where it is set rather than an
/// hour into an agent's work, and allowed once the model is on.
#[tokio::test]
async fn a_repository_takes_the_ai_mode_only_once_the_model_is_on() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;
    let repo = h.git_repo("repo");
    let body = json!({"path": repo.display().to_string(), "permission_mode": "ai"});

    let refused: ErrorBody = h
        .json(
            post_json("/v1/repositories", body.clone()),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "ai_disabled");

    // A repository already registered cannot be edited into it either.
    let registered: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": repo.display().to_string()}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let edit = format!("/v1/repositories/{}", registered.id);
    let refused: ErrorBody = h
        .json(
            put_json(&edit, json!({"permission_mode": "ai"})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "ai_disabled");

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;

    let edited: RepositoryDto = h
        .json(
            put_json(&edit, json!({"permission_mode": "ai"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.permission_mode, PermissionMode::Ai);

    // And a fresh registration takes it as readily.
    let _: RepositoryDto = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": repo.display().to_string(), "base_branch": "next",
                       "permission_mode": "ai"}),
            ),
            StatusCode::CREATED,
        )
        .await;
    let (status, _) = h.send(delete(&edit)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// The endpoint is the one handle the model server and the decision are built
/// on (022, Server and Decisions): the configured one wins over whatever a
/// server reports, and nothing is live while the AI permission model is off.
#[tokio::test]
async fn the_endpoint_is_the_configured_one_and_live_needs_the_model_on() {
    let release = ReleaseServer::start().await;
    let record = tempfile::NamedTempFile::new().unwrap();

    // With nothing configured, the endpoint is whatever the server reported.
    let h = with_ai_permissions("3.12.1", &release)
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;
    assert_eq!(h.state.ai_permissions.endpoint(), None);
    assert_eq!(h.state.ai_permissions.live().await, None);
    h.state
        .ai_permissions
        .set_endpoint(Some("http://127.0.0.1:9001".into()));
    assert_eq!(
        h.state.ai_permissions.endpoint().as_deref(),
        Some("http://127.0.0.1:9001")
    );
    assert_eq!(
        h.state.ai_permissions.live().await,
        None,
        "a server is not enough while the AI permission model is off"
    );

    let _: AiPermissionsStatusDto = h
        .json(
            update(json!({"enabled": true, "threshold": 0.6})),
            StatusCode::OK,
        )
        .await;
    let live = h
        .state
        .ai_permissions
        .live()
        .await
        .expect("the model is on and served");
    assert_eq!(live.endpoint, "http://127.0.0.1:9001");
    assert_eq!(live.threshold, 0.6);
    assert_eq!(
        status(&h).await.endpoint.as_deref(),
        Some("http://127.0.0.1:9001"),
        "and the status carries it"
    );
    h.state.ai_permissions.set_endpoint(None);
    assert_eq!(h.state.ai_permissions.live().await, None);

    // A configured endpoint is the answer whatever a server says.
    let pinned = with_ai_permissions("3.12.1", &release)
        .ai_permissions_endpoint("http://127.0.0.1:9999")
        .await;
    assert_eq!(
        pinned.state.ai_permissions.endpoint().as_deref(),
        Some("http://127.0.0.1:9999")
    );
    pinned
        .state
        .ai_permissions
        .set_endpoint(Some("http://127.0.0.1:1".into()));
    assert_eq!(
        pinned.state.ai_permissions.endpoint().as_deref(),
        Some("http://127.0.0.1:9999")
    );
}

/// The wire contract four other tasks build on: the three paths, the request
/// and reply schemas, the nullable schedule and the event kind.
#[tokio::test]
async fn the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;

    assert!(doc["paths"]["/v1/permissions/ai"]["get"].is_object());
    assert!(doc["paths"]["/v1/permissions/ai"]["put"].is_object());
    assert!(doc["paths"]["/v1/permissions/ai/refresh"]["post"].is_object());

    let schemas = &doc["components"]["schemas"];
    for name in [
        "AiPermissionsStatusDto",
        "UpdateAiPermissionsRequest",
        "AiPermissionsState",
        "PythonDto",
    ] {
        assert!(schemas[name].is_object(), "{name} is not in the document");
    }
    // A schedule that can be turned off has to say so on the wire.
    let schedule = &schemas["UpdateAiPermissionsRequest"]["properties"]["schedule"];
    assert!(
        schedule.to_string().contains("null"),
        "the schedule is not nullable: {schedule}"
    );

    // The doctor's report carries the interpreter the AI permission model installs into.
    assert!(schemas["DaemonReportDto"]["properties"]["python"].is_object());

    let event = doc["components"]["schemas"]["DomainEvent"].to_string();
    assert!(event.contains("ai_permissions_updated"), "{event}");
}

/// The doctor reports the interpreter the daemon would install into, whatever
/// the machine running the tests has: the question it answers is not whether
/// python3 is there but whether it is new enough.
#[tokio::test]
async fn the_doctor_reports_the_interpreter_the_model_needs() {
    let release = ReleaseServer::start().await;
    let python = python_printing("3.9.18");
    let h = with_ai_permissions("3.9.18", &release).await;

    let report: ariadne_api::doctor::DaemonReportDto = h.get("/v1/doctor").await;
    assert_eq!(report.python.path.as_deref(), Some(python.as_str()));
    assert_eq!(report.python.version.as_deref(), Some("3.9.18"));
    assert!(
        !report.python.ok,
        "3.9 is not one the AI permission model installs into"
    );
    assert!(
        report.tools.iter().all(|tool| tool.name != "python3"),
        "the interpreter is reported on its own, not among the tools"
    );

    // And a daemon whose `python_bin` names nothing runnable says so.
    let missing = harness().python_bin("/nonexistent/python3").await;
    let report: ariadne_api::doctor::DaemonReportDto = missing.get("/v1/doctor").await;
    assert_eq!(report.python.path, None);
    assert_eq!(report.python.version, None);
    assert!(!report.python.ok);
}
