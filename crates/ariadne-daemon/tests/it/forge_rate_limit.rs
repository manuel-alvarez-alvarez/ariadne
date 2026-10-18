//! A poll worker backs off, pauses, and folds wakes at the forge boundary
//! (026 rule 6).
use crate::common::forge::stub_forge_cli;
use crate::common::{QUIET, RUNS_OUT, TIMEOUT, eventually, harness, next_event, with_forge};
use ariadne_api::stream::DomainEvent;
use ariadne_core::ForgeKind;
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::forge::poll::{FORGE_RATE_LIMITED, ForgePoll, Mode};
use ariadne_daemon::timeouts::Timeouts;
use ariadne_store::{NewRepository, SetForgeIntegration, Store};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast::Receiver, oneshot};

async fn enabled_repository(h: &crate::common::Harness) -> ariadne_store::Repository {
    let path = h.git_repo("repo");
    let repo = h.repository(&path).await;
    with_forge(h, &repo).await;
    repo
}

fn called(stub: &crate::common::forge::StubForgeCli, prefix: &[&str]) -> bool {
    stub.invocations().iter().any(|call| {
        call.args.len() >= prefix.len()
            && call
                .args
                .iter()
                .map(String::as_str)
                .zip(prefix)
                .all(|(actual, expected)| actual == *expected)
    })
}

async fn next_fetch_error(events: &mut Receiver<BusEvent>, repository_id: &str, text: &str) {
    next_event(events, |event| {
        matches!(
            &event.event,
            DomainEvent::RepositoryUpdated(repository)
                if repository.id == repository_id
                    && repository
                        .forge
                        .as_ref()
                        .and_then(|forge| forge.webhook.fetch_error.as_deref())
                        .is_some_and(|error| error.contains(text))
        )
    })
    .await;
}

async fn next_fetch_error_clear(events: &mut Receiver<BusEvent>, repository_id: &str) {
    next_event(events, |event| {
        matches!(
            &event.event,
            DomainEvent::RepositoryUpdated(repository)
                if repository.id == repository_id
                    && repository
                        .forge
                        .as_ref()
                        .is_some_and(|forge| forge.webhook.fetch_error.is_none())
        )
    })
    .await;
}

/// Listen for the short interval during which a completed fetch would be
/// wrong. This is a `QUIET` listen, rather than a clock the test sleeps
/// through: a changed count fails immediately.
async fn quiet_fetches(
    stub: &crate::common::forge::StubForgeCli,
    expected: usize,
    within: Duration,
) {
    assert!(
        tokio::time::timeout(within, async {
            loop {
                assert_eq!(stub.fetches(), expected, "a fetch finished too early");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_err(),
        "a fetch finished during the quiet listen"
    );
}

async fn wait_for_fetches(stub: &crate::common::forge::StubForgeCli, expected: usize) {
    eventually(TIMEOUT, "the expected forge fetch", async || {
        stub.fetches() == expected
    })
    .await;
}

/// Start one poll over a stored, live hook. It chooses `WakeOnly` before its
/// worker exists, which proves the rate-limit path there without racing a
/// runtime mode change against the first fetch.
async fn wake_only_poll(
    h: &crate::common::Harness,
    timeouts: Timeouts,
) -> (Store, EventBus, ForgePoll, String) {
    let store = Store::open(&h.dir.path().join("wake-only.db"))
        .await
        .unwrap();
    let repository = store
        .create_repository_with_forge(
            NewRepository {
                default_workflow: None,
                path: "/test/widgets".into(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
                supervise_posts: None,
            },
            Some(SetForgeIntegration {
                repository_id: String::new(),
                kind: ForgeKind::Github,
                host: "github.com".into(),
                owner: "acme".into(),
                name: "widgets".into(),
                remote: "origin".into(),
                enabled: true,
                login: Some("me".into()),
            }),
        )
        .await
        .unwrap();
    let mut integration = repository.forge.unwrap();
    integration.webhook_id = Some(17);
    integration.webhook_secret = Some("ab".repeat(32));
    integration.webhook_url = Some(format!(
        "https://hooks.example/webhooks/github/{}",
        repository.id
    ));
    integration.webhook_state = "live".into();
    store.set_webhook(&integration).await.unwrap();

    let bus = ariadne_daemon::bus::start(store.clone());
    let mut cfg = (*h.launcher.cfg).clone();
    cfg.webhook_public_url = Some("https://hooks.example".into());
    let poll = ariadne_daemon::forge::poll::start(
        store.clone(),
        Arc::new(cfg),
        &bus,
        ariadne_daemon::forge::live::LivePulls::default(),
        timeouts,
    );
    (store, bus, poll, repository.id)
}

/// A `pr list` rate-limit refusal pauses the worker. Wakes inside the pause
/// are held until its end, when exactly one successful fetch clears the
/// stored, user-visible error. A rate limit does not spend another forge
/// call on the reactive signed-out check.
#[tokio::test]
async fn a_rate_limited_timer_fetch_pauses_and_fetches_once_after_wakes_inside_it() {
    let stub = stub_forge_cli(json!([
        {"args": ["api", "rate_limit"], "exit": 1, "stderr": "gh: Not Found (HTTP 404)"},
        {"args": ["pr", "list"], "exit": 1,
         "stderr": "gh: API rate limit exceeded for user ID 1. (HTTP 403)"},
    ]));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_poll: RUNS_OUT * 3,
            forge_rate_limit_pause: RUNS_OUT * 3,
            forge_wake_debounce: Duration::from_millis(1),
            ..Default::default()
        })
        .await;
    let mut events = h.bus.subscribe();
    let repo = enabled_repository(&h).await;

    next_fetch_error(&mut events, &repo.id, FORGE_RATE_LIMITED).await;
    assert!(called(&stub, &["api", "rate_limit"]));
    assert!(
        !called(&stub, &["api", "user"]),
        "a rate limit is already classified; do not check whether the CLI is signed out"
    );

    // Any early fetch would now succeed, so the quiet listen proves the
    // wakes stayed inside the pause instead of becoming their own fetches.
    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    h.state.forge_poll.wake(&repo.id);
    h.state.forge_poll.wake(&repo.id);
    quiet_fetches(&stub, 0, QUIET).await;

    wait_for_fetches(&stub, 1).await;
    next_fetch_error_clear(&mut events, &repo.id).await;
    quiet_fetches(&stub, 1, QUIET).await;
}

/// A stored live hook starts in WakeOnly mode. Its rate-limit pause holds
/// wakes exactly as the timer worker does, and recovery clears the visible
/// error without trying a sign-out confirmation.
#[tokio::test]
async fn a_rate_limited_wake_only_fetch_pauses_and_fetches_once_after_wakes_inside_it() {
    let stub = stub_forge_cli(json!([
        {"args": ["api", "rate_limit"], "exit": 1, "stderr": "gh: Not Found (HTTP 404)"},
        {"args": ["pr", "list"], "exit": 1,
         "stderr": "gh: API rate limit exceeded for user ID 1. (HTTP 403)"},
        {"args": ["api", "repos/acme/widgets/hooks/17"], "exit": 0, "stdout": "{}"},
    ]));
    let h = harness().forge_cli(&stub).await;
    let (_store, bus, poll, id) = wake_only_poll(
        &h,
        Timeouts {
            forge_poll: RUNS_OUT * 3,
            forge_rate_limit_pause: RUNS_OUT * 3,
            forge_wake_debounce: Duration::from_millis(1),
            ..Default::default()
        },
    )
    .await;
    let mut events = bus.subscribe();

    next_fetch_error(&mut events, &id, FORGE_RATE_LIMITED).await;
    assert!(called(&stub, &["api", "rate_limit"]));
    assert!(
        !called(&stub, &["api", "user"]),
        "a rate limit is already classified; do not check whether the CLI is signed out"
    );

    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    poll.wake(&id);
    poll.wake(&id);
    quiet_fetches(&stub, 0, QUIET).await;

    wait_for_fetches(&stub, 1).await;
    next_fetch_error_clear(&mut events, &id).await;
    quiet_fetches(&stub, 1, QUIET).await;
}

/// A successful GitHub rate-limit read records the reset of the only
/// exhausted resource, not the later reset of another resource with calls
/// left.
#[tokio::test]
async fn a_rate_limited_fetch_reads_the_only_spent_limit_reset() {
    let core = 4_102_444_800_i64;
    let graphql = core + 1800;
    let stub = stub_forge_cli(json!([
        {"args": ["api", "rate_limit"], "exit": 0,
         "stdout": json!({"resources": {
             "core": {"remaining": 0, "reset": core},
             "graphql": {"remaining": 4_000, "reset": graphql}
         }}).to_string()},
        {"args": ["pr", "list"], "exit": 1,
         "stderr": "gh: API rate limit exceeded for user ID 1. (HTTP 403)"},
    ]));
    let h = harness().forge_cli(&stub).await;
    let mut events = h.bus.subscribe();
    let repo = enabled_repository(&h).await;
    let expected = format!(
        "{FORGE_RATE_LIMITED} {}",
        chrono::DateTime::from_timestamp(core, 0)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );

    next_fetch_error(&mut events, &repo.id, &expected).await;
    assert!(called(&stub, &["api", "rate_limit"]));
    quiet_fetches(&stub, 0, QUIET).await;
}

/// A rate-limit reset which has passed still pauses for one ordinary poll
/// interval, so an ahead local clock cannot make the worker loop at once.
#[tokio::test]
async fn a_past_rate_limit_reset_waits_for_the_poll_interval() {
    let poll = RUNS_OUT * 3;
    let stub = stub_forge_cli(json!([
        {"args": ["api", "rate_limit"], "exit": 0,
         "stdout": json!({"resources": {
             "core": {"remaining": 0, "reset": 1},
             "graphql": {"remaining": 4_000, "reset": 4_102_444_800_i64}
         }}).to_string()},
        {"args": ["pr", "list"], "exit": 1,
         "stderr": "gh: API rate limit exceeded for user ID 1. (HTTP 403)"},
    ]));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_poll: poll,
            ..Default::default()
        })
        .await;
    let mut events = h.bus.subscribe();
    let repo = enabled_repository(&h).await;

    next_fetch_error(&mut events, &repo.id, FORGE_RATE_LIMITED).await;
    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    quiet_fetches(&stub, 0, QUIET).await;
    wait_for_fetches(&stub, 1).await;
    next_fetch_error_clear(&mut events, &repo.id).await;
}

/// In timer mode a plain failure waits twice as long before trying again,
/// stops growing at its cap, resets after success, and a wake overrides the
/// pending timer. Error updates are the observable boundary for failed
/// fetches; successful rounds are counted by the forge stub itself.
#[tokio::test]
async fn a_failed_fetch_in_timer_mode_backs_off_and_a_wake_still_fetches_at_once() {
    // These are production timeout fields reduced only for this timer
    // behavior test. The proportional margins below still distinguish each
    // scheduling decision from the next one. A listen that waits for a fetch
    // to complete also waits out the stub's own calls, which under load take
    // a good part of an interval: so the cap is four intervals, and each such
    // listen ends at least an interval before the wrong decision would fetch.
    let poll = RUNS_OUT / 2;
    let cap = poll * 4;
    let stub = stub_forge_cli(json!([
        {"args": ["api", "user"], "exit": 0, "stdout": "me"},
        {"args": ["pr", "list"], "exit": 1, "stderr": "gh: first failure (HTTP 500)"},
    ]));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_poll: poll,
            forge_poll_backoff_cap: cap,
            forge_wake_debounce: Duration::ZERO,
            ..Default::default()
        })
        .await;
    let mut events = h.bus.subscribe();
    let repo = enabled_repository(&h).await;
    next_fetch_error(&mut events, &repo.id, "first failure").await;
    assert!(called(&stub, &["api", "user"]));

    // The normal timer would run after `poll`; halfway between that and the
    // doubled wait, its distinct error must not have been stored yet.
    stub.reprogram(json!([
        {"args": ["api", "user"], "exit": 0, "stdout": "me"},
        {"args": ["pr", "list"], "exit": 1, "stderr": "gh: second failure (HTTP 500)"},
    ]));
    assert!(
        tokio::time::timeout(
            poll + poll / 2,
            next_fetch_error(&mut events, &repo.id, "second failure")
        )
        .await
        .is_err(),
        "a failed timer fetch must wait twice as long"
    );
    next_fetch_error(&mut events, &repo.id, "second failure").await;

    // The second failure reaches the cap. If it doubled past it, its success
    // would miss this listen, which ends three intervals before that.
    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    tokio::time::timeout(cap + poll, wait_for_fetches(&stub, 1))
        .await
        .expect("the capped timer fetch must complete");
    next_fetch_error_clear(&mut events, &repo.id).await;

    // The successful fetch resets the timer to its ordinary interval.
    // Leaving the forge successful makes the next completed fetch prove it
    // did not keep the cap-sized backoff: kept, it would start only at the
    // cap, an interval and a half after this listen ends.
    tokio::time::timeout(poll * 2 + poll / 2, wait_for_fetches(&stub, 2))
        .await
        .expect("the first success must reset the timer backoff");

    // A wake during the next timer wait ignores that wait and fetches now.
    stub.reprogram(json!([
        {"args": ["api", "user"], "exit": 0, "stdout": "me"},
        {"args": ["pr", "list"], "exit": 1, "stderr": "gh: wake failure (HTTP 500)"},
    ]));
    h.state.forge_poll.wake(&repo.id);
    next_fetch_error(&mut events, &repo.id, "wake failure").await;
    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    h.state.forge_poll.wake(&repo.id);
    // Manager commands preserve order. Switching to WakeOnly after the
    // wake removes the timer as a possible source of the next fetch, so the
    // completed success below can only be the queued wake bypassing backoff.
    h.state.forge_poll.set_mode(&repo.id, Mode::WakeOnly);
    wait_for_fetches(&stub, 3).await;
    next_fetch_error_clear(&mut events, &repo.id).await;
}

/// Each fetch of a repository's requests is timed by its repository and its
/// outcome (035): a failure, then the wake's success.
#[tokio::test]
async fn each_fetch_is_timed_by_its_repository_and_outcome() {
    let stub = stub_forge_cli(json!([
        {"args": ["api", "user"], "exit": 0, "stdout": "me"},
        {"args": ["pr", "list"], "exit": 1, "stderr": "gh: failure (HTTP 500)"},
    ]));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_wake_debounce: Duration::ZERO,
            ..Default::default()
        })
        .await;
    let mut events = h.bus.subscribe();
    let repo = enabled_repository(&h).await;
    next_fetch_error(&mut events, &repo.id, "failure").await;
    stub.reprogram(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    h.state.forge_poll.wake(&repo.id);
    next_fetch_error_clear(&mut events, &repo.id).await;
    for outcome in ["failed", "ok"] {
        eventually(TIMEOUT, "the timed fetch", async || {
            h.metric(
                "ariadne.forge.poll.duration_count",
                &[("repo", "repo"), ("outcome", outcome)],
            )
            .await
                == 1.0
        })
        .await;
    }
}

/// Three wakes during the configured quiet period become one fetch.
#[tokio::test]
async fn three_wakes_inside_the_debounce_window_cause_one_fetch() {
    let stub = stub_forge_cli(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_wake_debounce: QUIET * 2,
            ..Default::default()
        })
        .await;
    let repo = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&repo.id, Mode::WakeOnly);
    wait_for_fetches(&stub, 1).await;

    for _ in 0..3 {
        h.state.forge_poll.wake(&repo.id);
    }
    quiet_fetches(&stub, 1, QUIET).await;
    wait_for_fetches(&stub, 2).await;
    quiet_fetches(&stub, 2, QUIET).await;
}

/// Continued webhook-like wakes do not move the debounce deadline: one
/// fetch starts while deliveries still arrive, rather than only after they
/// stop. The driver is stopped with the test before it can leave work behind.
#[tokio::test]
async fn continued_wakes_do_not_extend_the_debounce_window() {
    let stub = stub_forge_cli(json!([
        {"args": ["pr", "list"], "exit": 0, "stdout": "[]"},
        {"args": ["api", "repos/acme/widgets/issues?state=open&per_page=100"],
         "exit": 0, "stdout": "[]"},
    ]));
    let window = RUNS_OUT * 4;
    let h = harness()
        .forge_cli(&stub)
        .timeouts(Timeouts {
            forge_wake_debounce: window,
            ..Default::default()
        })
        .await;
    let repo = enabled_repository(&h).await;
    h.state.forge_poll.set_mode(&repo.id, Mode::WakeOnly);
    wait_for_fetches(&stub, 1).await;

    h.state.forge_poll.wake(&repo.id);
    let poll = h.state.forge_poll.clone();
    let id = repo.id.clone();
    let (sent_continued, continued) = oneshot::channel();
    let deliveries = tokio::spawn(async move {
        let mut every = tokio::time::interval(window / 4);
        every.tick().await;
        every.tick().await;
        poll.wake(&id);
        every.tick().await;
        poll.wake(&id);
        let _ = sent_continued.send(());
        loop {
            every.tick().await;
            poll.wake(&id);
        }
    });
    continued
        .await
        .expect("the test's continued delivery driver stopped");
    let fetched = tokio::time::timeout(window + QUIET, wait_for_fetches(&stub, 2)).await;
    deliveries.abort();
    let _ = deliveries.await;
    fetched.expect("continued wakes must not postpone the first debounce deadline");
}
