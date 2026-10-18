//! Integration tests for the AI permission settings, the Python check and the install
//! behind `/v1/permissions/ai` (022).
//!
//! Nothing here downloads anything. The whole install is one shell script the
//! daemon runs in place of the venv, package and weights, and the interpreter is a script
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
use ariadne_daemon::ai_permissions::hardware::HardwareOverride;
use ariadne_daemon::bus::EventBus;
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::Store;

use common::{
    Harness, HarnessBuilder, QUIET, TIMEOUT, delete, eventually, harness, next_event, post,
    post_json, put_json, shared_script,
};

/// The release of 4b on `mlx`, the choice [`with_ai_permissions`]'s Mac settles on.
const PIN: &str = "kev@f1535963 jaredpalmer/kev-4b@139fdd94 on mlx";
const RUN: &str = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101";
const KEV_COMMIT: &str = "f1535963cea021439370c23127bc970b6788e730";

/// The default threshold pair of `4b`, which the benchmark selected on kev-4b
/// (022, rule 24). `0.8b` and `27b` take it too.
const PAIR_4B: (f64, f64) = (0.0201, 0.6321);
/// The default threshold pair of `9b`: the 4b pair, since the benchmark's
/// kev-9b pair allows nothing.
const PAIR_9B: (f64, f64) = (0.0201, 0.6321);

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
         printf '%s\\n%s\\n%s\\n%s\\n%s\\n' \
           \"$AI_PERMISSIONS_HOME\" \"$AI_PERMISSIONS_RUN\" \"$AI_PERMISSIONS_KEV_COMMIT\" \
           \"$AI_PERMISSIONS_FLAVOUR\" \"$AI_PERMISSIONS_DEVICE\" > \"$1\"\n\
         if [ -n \"$2\" ]; then\n\
           while [ ! -f \"$2\" ]; do sleep 0.05; done\n\
         fi\n\
         if [ \"$3\" != \"0\" ]; then\n\
           echo 'the package could not be installed' >&2\n\
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

/// What the installer recorded: its home, run, Kev commit, flavour and
/// device, in that order.
fn recorded(record: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(record)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

// -- the harness -------------------------------------------------------------

/// A daemon whose Python is new enough, on a 64 GB Apple Silicon Mac, which
/// settles on 4b on `mlx`. The install itself is the caller's to configure.
fn with_ai_permissions(python: &str) -> HarnessBuilder {
    harness()
        .python_bin(python_printing(python))
        .ai_permissions_hardware(machine("macos", "aarch64", 64, None))
}

fn machine(os: &str, arch: &str, memory_gb: u64, vram_gb: Option<u64>) -> HardwareOverride {
    HardwareOverride {
        os: os.into(),
        arch: arch.into(),
        memory_gb,
        gpu_name: vram_gb.map(|_| "test-gpu".to_string()),
        vram_gb,
    }
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
/// interpreter it probed: off, the thresholds the schema carries, and the
/// default flavour.
#[tokio::test]
async fn the_settings_start_at_the_defaults_with_the_interpreter_probed() {
    let python = python_printing("3.13.1");
    let h = with_ai_permissions("3.13.1").await;

    let status = status(&h).await;
    assert!(!status.enabled);
    assert_eq!(status.flavour.as_str(), "4b");
    assert_eq!((status.allow_threshold, status.deny_threshold), PAIR_4B);
    assert!(status.thresholds_default);
    assert_eq!(status.state, AiPermissionsState::Disabled);
    assert_eq!(status.installed_release, None);
    assert_eq!(status.latest_release, None);
    assert!(!status.weights_present);
    assert_eq!(status.endpoint, None);
    assert_eq!(status.last_refresh_at, None);
    assert_eq!(status.last_error, None);

    assert_eq!(status.python.path.as_deref(), Some(python.as_str()));
    assert_eq!(status.python.version.as_deref(), Some("3.13.1"));
    assert!(status.python.ok);
}

/// The model installs PyTorch into a virtual environment of the daemon's Python,
/// and needs Python 3.12 or 3.13. Turning it on against Python 3.14 is refused
/// before anything is downloaded — and the refusal leaves the settings
/// exactly as they were, so nothing has to be undone.
#[tokio::test]
async fn turning_the_model_on_without_a_new_enough_python_is_refused_and_changes_nothing() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.14.0")
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;

    let refused: ErrorBody = h
        .json(update(json!({"enabled": true})), StatusCode::CONFLICT)
        .await;
    assert_eq!(refused.error.code, "python_unavailable");
    assert!(refused.error.message.contains("3.14.0"), "{refused:?}");
    assert!(
        refused.error.message.contains("3.12 or 3.13"),
        "{refused:?}"
    );

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
/// installer saw is the model home, run and package commit, and the status
/// records the pin it installed.
#[tokio::test]
async fn turning_the_model_on_starts_the_install_and_reports_it_ready() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
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
    assert_eq!(ready.installed_release.as_deref(), Some(PIN));
    assert_eq!(ready.latest_release.as_deref(), Some(PIN));
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
            RUN.to_string(),
            KEV_COMMIT.to_string(),
            "4b".to_string(),
            "mlx".to_string(),
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
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
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
        Some("the package could not be installed"),
        "the installer's own stderr is what the user is left to act on"
    );
    assert!(!failed.weights_present);
    assert_eq!(failed.installed_release, None);
}

/// The settings a user chooses are kept, refused where they are not a
/// threshold pair, and read back by the daemon that comes up next: an
/// install that ran for minutes must not be forgotten by a restart.
#[tokio::test]
async fn the_settings_are_validated_and_survive_a_daemon_restart() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;

    let chosen: AiPermissionsStatusDto = h
        .json(
            update(json!({"allow_threshold": 0.2, "deny_threshold": 0.8})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(chosen.allow_threshold, 0.2);
    assert_eq!(chosen.deny_threshold, 0.8);

    for bad in [
        json!({"allow_threshold": 1.5}),
        json!({"deny_threshold": 1.5}),
        json!({"allow_threshold": 0.8}),
    ] {
        let refused: ErrorBody = h
            .json(update(bad.clone()), StatusCode::UNPROCESSABLE_ENTITY)
            .await;
        assert_eq!(refused.error.code, "invalid_request", "{bad}");
    }
    let unchanged = status(&h).await;
    assert_eq!(unchanged.allow_threshold, 0.2, "a refusal wrote nothing");
    assert_eq!(unchanged.deny_threshold, 0.8, "a refusal wrote nothing");

    // The daemon that comes up next reads the same settings.
    let restarted = Store::open(h.dir.path().join("test.db")).await.unwrap();
    let ai_permissions = AiPermissions::new(
        restarted,
        EventBus::default(),
        &h.launcher.cfg,
        Timeouts::default(),
    );
    let kept = ai_permissions.status().await;
    assert_eq!(kept.allow_threshold, 0.2);
    assert_eq!(kept.deny_threshold, 0.8);
}

fn pair(status: &AiPermissionsStatusDto) -> (f64, f64) {
    (status.allow_threshold, status.deny_threshold)
}

/// A fresh daemon switched to `9b` reports the default pair of `9b`, not the
/// pair the row was seeded with.
#[tokio::test]
async fn a_fresh_daemon_on_9b_reports_the_9b_default_pair() {
    let h = with_ai_permissions("3.13.1").await;

    let on_9b: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "9b"})), StatusCode::OK)
        .await;

    assert_eq!(on_9b.flavour.as_str(), "9b");
    assert_eq!(pair(&on_9b), PAIR_9B);
    assert!(on_9b.thresholds_default);
}

/// A pair nobody set by hand follows each flavour change to that flavour's
/// default. A threshold sent marks the pair hand-set, and a later flavour
/// change keeps it.
#[tokio::test]
async fn a_hand_set_pair_survives_a_flavour_change_and_a_default_pair_follows_it() {
    let h = with_ai_permissions("3.13.1").await;
    let on_9b: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "9b"})), StatusCode::OK)
        .await;
    assert_eq!(pair(&on_9b), PAIR_9B);

    let on_4b: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "4b"})), StatusCode::OK)
        .await;
    assert_eq!(pair(&on_4b), PAIR_4B, "a default pair follows the flavour");
    assert!(on_4b.thresholds_default);

    let hand_set: AiPermissionsStatusDto = h
        .json(update(json!({"allow_threshold": 0.2})), StatusCode::OK)
        .await;
    assert_eq!(pair(&hand_set), (0.2, PAIR_4B.1));
    assert!(!hand_set.thresholds_default);

    let switched: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "9b"})), StatusCode::OK)
        .await;
    assert_eq!(switched.flavour.as_str(), "9b");
    assert_eq!(pair(&switched), (0.2, PAIR_4B.1), "a hand-set pair stays");
    assert!(!switched.thresholds_default);
}

/// `default_thresholds` drops a hand-set pair and goes back to the default
/// of the chosen flavour. Sent with a threshold, it is refused and writes
/// nothing.
#[tokio::test]
async fn default_thresholds_returns_a_hand_set_pair_to_the_flavour_default() {
    let h = with_ai_permissions("3.13.1").await;
    let _: AiPermissionsStatusDto = h
        .json(
            update(json!({"flavour": "9b", "allow_threshold": 0.2, "deny_threshold": 0.8})),
            StatusCode::OK,
        )
        .await;

    let refused: ErrorBody = h
        .json(
            update(json!({"default_thresholds": true, "allow_threshold": 0.3})),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
    assert_eq!(refused.error.code, "invalid_request");
    assert_eq!(
        pair(&status(&h).await),
        (0.2, 0.8),
        "a refusal wrote nothing"
    );

    let reset: AiPermissionsStatusDto = h
        .json(update(json!({"default_thresholds": true})), StatusCode::OK)
        .await;
    assert_eq!(pair(&reset), PAIR_9B);
    assert!(reset.thresholds_default);

    let on_4b: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "4b"})), StatusCode::OK)
        .await;
    assert_eq!(
        pair(&on_4b),
        PAIR_4B,
        "the reset pair follows the flavour again"
    );
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
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.13.1")
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
        Some(RUN)
    );
}

/// Turning the model off keeps every file: turning it back on is the release check
/// and nothing else, and the weights that took an hour stay where they are.
#[tokio::test]
async fn turning_the_model_off_keeps_the_files_it_installed() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
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
    assert_eq!(off.installed_release.as_deref(), Some(PIN));
    assert!(off.weights_present, "the files are still on disk");
}

/// Turning the model off while an install runs does not stop the install, and the
/// install's end does not turn the model back on: what it put on disk is written
/// down, and the state stays `disabled`.
#[tokio::test]
async fn turning_the_model_off_during_an_install_keeps_it_off_when_the_install_ends() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.13.1")
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
    assert_eq!(ended.installed_release.as_deref(), Some(PIN));
    assert!(ended.weights_present, "what it put on disk is written down");
    assert_eq!(status(&h).await.state, AiPermissionsState::Disabled);
}

/// Turning the model off and on again while its install runs waits for that
/// install: the answer says `installing` again, and the install's end is
/// what the model is on.
#[tokio::test]
async fn turning_the_model_back_on_during_its_install_reports_that_install() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.13.1")
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
    assert_eq!(ready.installed_release.as_deref(), Some(PIN));
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
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");
    let h = with_ai_permissions("3.13.1")
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

/// The `ai` mode asks the model, so a repository cannot be put into it while
/// there is no model to ask. It is refused where it is set rather than an
/// hour into an agent's work, and allowed once the model is on.
#[tokio::test]
async fn a_repository_takes_the_ai_mode_only_once_the_model_is_on() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
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
    let record = tempfile::NamedTempFile::new().unwrap();
    let hold = tempfile::tempdir().unwrap();
    let hold = hold.path().join("let-go");

    // With nothing configured, the endpoint is whatever the server reported.
    // The test reports it by hand, so the installer holds and the install
    // never gets ready: a ready install starts the real server, which has no
    // Python here, exits, and takes the reported endpoint away with it.
    let h = with_ai_permissions("3.13.1")
        .ai_permissions_installer(installer(record.path(), &hold.display().to_string(), "0"))
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
            update(json!({"enabled": true, "allow_threshold": 0.2,
                          "deny_threshold": 0.8})),
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
    assert_eq!(live.allow_threshold, 0.2);
    assert_eq!(live.deny_threshold, 0.8);
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"default_thresholds": true})), StatusCode::OK)
        .await;
    let live = h.state.ai_permissions.live().await.expect("still served");
    assert_eq!(
        (live.allow_threshold, live.deny_threshold),
        PAIR_4B,
        "a decision holds the flavour default once the pair is reset"
    );
    assert_eq!(
        status(&h).await.endpoint.as_deref(),
        Some("http://127.0.0.1:9001"),
        "and the status carries it"
    );
    h.state.ai_permissions.set_endpoint(None);
    assert_eq!(h.state.ai_permissions.live().await, None);

    // A configured endpoint is the answer whatever a server says.
    let pinned = with_ai_permissions("3.13.1")
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

/// The wire contract four other tasks build on: the four paths, the request
/// and reply schemas, the flavour and device shapes and the event kind.
#[tokio::test]
async fn the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document() {
    let h = harness().await;
    let doc: serde_json::Value = h.get("/api-docs/openapi.json").await;

    assert!(doc["paths"]["/v1/permissions/ai"]["get"].is_object());
    assert!(doc["paths"]["/v1/permissions/ai"]["put"].is_object());
    assert!(doc["paths"]["/v1/permissions/ai/refresh"]["post"].is_object());
    assert!(doc["paths"]["/v1/permissions/ai/test"]["post"].is_object());

    let schemas = &doc["components"]["schemas"];
    for name in [
        "AiPermissionsStatusDto",
        "UpdateAiPermissionsRequest",
        "AiPermissionsState",
        "PythonDto",
        "TestAiPermissionRequest",
        "TestAiPermissionResponse",
        "Flavour",
        "Device",
        "HardwareDto",
        "FlavourOptionsDto",
        "DeviceOptionDto",
    ] {
        assert!(schemas[name].is_object(), "{name} is not in the document");
    }
    for field in [
        "flavour",
        "device",
        "hardware",
        "flavours",
        "thresholds_default",
    ] {
        assert!(
            schemas["AiPermissionsStatusDto"]["properties"][field].is_object(),
            "{field} is absent from the status schema"
        );
    }
    for field in ["flavour", "device", "default_thresholds"] {
        assert!(
            schemas["UpdateAiPermissionsRequest"]["properties"][field].is_object(),
            "{field} is absent from the update schema"
        );
    }
    for field in ["allow_threshold", "deny_threshold"] {
        assert!(
            schemas["AiPermissionsStatusDto"]["properties"][field].is_object(),
            "{field} is absent from the status schema"
        );
        assert!(
            schemas["UpdateAiPermissionsRequest"]["properties"][field].is_object(),
            "{field} is absent from the update schema"
        );
    }
    for field in ["workspace", "locations"] {
        assert!(
            schemas["TestAiPermissionRequest"]["properties"][field].is_object(),
            "{field} is absent from the test request"
        );
    }
    for field in ["operation", "risk_tags", "cap", "probabilities"] {
        assert!(
            schemas["TestAiPermissionResponse"]["properties"][field].is_object(),
            "{field} is absent from the test response"
        );
    }
    assert_eq!(
        schemas["TestAiPermissionResponse"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "ai_error",
            "allow_threshold",
            "cap",
            "danger",
            "deny_threshold",
            "label",
            "operation",
            "probabilities",
            "risk_tags"
        ]
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
    let python = python_printing("3.14.0");
    let h = with_ai_permissions("3.14.0").await;

    let report: ariadne_api::doctor::DaemonReportDto = h.get("/v1/doctor").await;
    assert_eq!(report.python.path.as_deref(), Some(python.as_str()));
    assert_eq!(report.python.version.as_deref(), Some("3.14.0"));
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

/// Each choice installs its own flavour on its own device: the installer
/// gets the flavour, the device and the flavour's `--run`, and the release
/// names all three. Every choice after the first is a switch of a model that
/// is on, so it starts its install at once.
#[tokio::test]
async fn the_install_gets_the_run_flavour_and_device_of_each_choice() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = harness()
        .python_bin(python_printing("3.13.1"))
        .ai_permissions_hardware(machine("linux", "x86_64", 256, Some(256)))
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;
    let choices = [
        (
            "0.8b",
            "cpu",
            "jaredpalmer/kev-0.8b@9a45d25eb2ab761841196625383fa1dff0e56c1e",
            "kev@f1535963 jaredpalmer/kev-0.8b@9a45d25e on cpu",
        ),
        (
            "4b",
            "cuda",
            "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101",
            "kev@f1535963 jaredpalmer/kev-4b@139fdd94 on cuda",
        ),
        (
            "9b",
            "cpu",
            "jaredpalmer/kev-9b@2629c06a5aeb0feb3b9783bafed17ed8f39ecf5c",
            "kev@f1535963 jaredpalmer/kev-9b@2629c06a on cpu",
        ),
        (
            "27b",
            "cuda",
            "jaredpalmer/kev-27b@01b81998019be550f0ae858727df49bac9511195",
            "kev@f1535963 jaredpalmer/kev-27b@01b81998 on cuda",
        ),
    ];
    for (index, (flavour, device, run, release)) in choices.into_iter().enumerate() {
        let body = match index {
            0 => json!({"enabled": true, "flavour": flavour, "device": device}),
            _ => json!({"flavour": flavour, "device": device}),
        };
        let started: AiPermissionsStatusDto = h.json(update(body), StatusCode::OK).await;
        assert_eq!(started.state, AiPermissionsState::Installing, "{flavour}");
        let ready = settles_on(&h, AiPermissionsState::Ready).await;
        assert_eq!(ready.installed_release.as_deref(), Some(release));
        assert_eq!(
            recorded(record.path())[1..],
            [
                run.to_string(),
                KEV_COMMIT.to_string(),
                flavour.to_string(),
                device.to_string(),
            ]
        );
    }
}

/// A switch while the model is off is only stored: nothing installs until
/// the model is turned on, and then the stored choice is what installs.
#[tokio::test]
async fn a_switch_while_off_is_only_stored_and_the_next_turn_on_installs_it() {
    let record = tempfile::NamedTempFile::new().unwrap();
    let h = with_ai_permissions("3.13.1")
        .ai_permissions_installer(installer(record.path(), "", "0"))
        .await;
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    settles_on(&h, AiPermissionsState::Ready).await;
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": false})), StatusCode::OK)
        .await;
    std::fs::write(record.path(), "").unwrap();

    let stored: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "0.8b"})), StatusCode::OK)
        .await;
    assert_eq!(stored.state, AiPermissionsState::Disabled);
    assert_eq!(stored.flavour.as_str(), "0.8b");
    tokio::time::sleep(QUIET).await;
    assert!(recorded(record.path()).is_empty(), "nothing installed");
    assert_eq!(status(&h).await.installed_release.as_deref(), Some(PIN));

    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    let ready = settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(
        ready.installed_release.as_deref(),
        Some("kev@f1535963 jaredpalmer/kev-0.8b@9a45d25e on mlx")
    );
    assert_eq!(recorded(record.path())[3..], ["0.8b", "mlx"]);
}

/// A switch that installs well deletes the Hugging Face cache folders of
/// every other flavour. One that fails keeps them, so the weights that
/// worked are still there; a refresh then repairs the stored choice.
#[tokio::test]
async fn a_successful_switch_deletes_the_other_flavours_weights_and_a_failed_one_keeps_them() {
    let outcome = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outcome.path(), "0").unwrap();
    let h = with_ai_permissions("3.13.1")
        .ai_permissions_installer(vec![
            "/bin/sh".into(),
            "-c".into(),
            "exit \"$(cat \"$1\")\"".into(),
            "installer".into(),
            outcome.path().display().to_string(),
        ])
        .await;
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"enabled": true})), StatusCode::OK)
        .await;
    settles_on(&h, AiPermissionsState::Ready).await;
    let hub = h.launcher.cfg.root.join("ai-permissions/hf/hub");
    let kept = [
        "models--jaredpalmer--kev-0.8b",
        "models--Qwen--Qwen3.5-0.8B-Base",
    ];
    let others = [
        "models--jaredpalmer--kev-4b",
        "models--Qwen--Qwen3.5-4B-Base",
        "models--jaredpalmer--kev-9b",
    ];
    for folder in kept.iter().chain(&others) {
        std::fs::create_dir_all(hub.join(folder).join("snapshots")).unwrap();
    }

    std::fs::write(outcome.path(), "1").unwrap();
    let _: AiPermissionsStatusDto = h
        .json(update(json!({"flavour": "0.8b"})), StatusCode::OK)
        .await;
    settles_on(&h, AiPermissionsState::Failed).await;
    for folder in kept.iter().chain(&others) {
        assert!(hub.join(folder).exists(), "a failed switch keeps {folder}");
    }

    std::fs::write(outcome.path(), "0").unwrap();
    let _: AiPermissionsStatusDto = h
        .json(post("/v1/permissions/ai/refresh"), StatusCode::ACCEPTED)
        .await;
    let ready = settles_on(&h, AiPermissionsState::Ready).await;
    assert_eq!(
        ready.installed_release.as_deref(),
        Some("kev@f1535963 jaredpalmer/kev-0.8b@9a45d25e on mlx"),
        "the refresh repaired the stored choice"
    );
    for folder in kept {
        assert!(
            hub.join(folder).exists(),
            "the chosen flavour keeps {folder}"
        );
    }
    for folder in others {
        assert!(!hub.join(folder).exists(), "{folder} is deleted");
    }
}
