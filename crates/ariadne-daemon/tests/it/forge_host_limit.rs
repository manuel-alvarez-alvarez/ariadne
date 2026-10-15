//! At most two repository fetches run at once against one forge host,
//! whatever started them — startup, the timer, or an explicit wake — and a
//! different host never waits on another's cap (026 rule 6). A queued
//! fetch runs once a slot frees, whether the fetch ahead of it finished,
//! failed, or was cancelled by disabling its integration mid-fetch.
use crate::common::forge::{StubForgeCli, answer, stub_forge_cli};
use crate::common::{Harness, QUIET, TIMEOUT, eventually, harness, post_json, put_json, sh};
use axum::http::StatusCode;
use serde_json::{Value, json};

/// A repository checked out, its remote on `host`, registered with the
/// forge switched on: one more worker competing for `host`'s own cap.
async fn repo_on(h: &Harness, host: &str, owner: &str, name: &str) -> String {
    let path = h.git_repo(name);
    sh(
        &path,
        &format!("git remote add origin https://{host}/{owner}/{name}.git"),
    );
    let repo: Value = h
        .json(
            post_json(
                "/v1/repositories",
                json!({"path": path, "forge": {"enabled": true}}),
            ),
            StatusCode::CREATED,
        )
        .await;
    repo["id"].as_str().unwrap().into()
}

/// What every repository's enable probes, and its fetch's own final call:
/// the open issues, empty, so the fetch raises no news of its own.
fn issues(owner: &str, name: &str) -> Value {
    answer(
        &[
            "api",
            &format!("repos/{owner}/{name}/issues?state=open&per_page=100"),
        ],
        0,
        "[]",
    )
}

fn login_probe() -> Vec<Value> {
    vec![
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
    ]
}

/// A `pr list` entry for one repository's own `--repo <host>/<owner>/<name>`,
/// answering an empty list, held at `gate` where one is given.
fn list_entry(host: &str, owner: &str, name: &str, gate: Option<&std::path::Path>) -> Value {
    let mut entry = answer(
        &["pr", "list", "--repo", &format!("{host}/{owner}/{name}")],
        0,
        "[]",
    );
    if let Some(gate) = gate {
        entry["wait_for"] = json!(gate);
    }
    entry
}

/// Whether a `pr list` call for exactly this repository has been invoked.
fn listed(stub: &StubForgeCli, host: &str, owner: &str, name: &str) -> bool {
    let slug = format!("{host}/{owner}/{name}");
    stub.invocations().iter().any(|call| {
        call.args.starts_with(&["pr".into(), "list".into()]) && call.args.contains(&slug)
    })
}

/// At most two of three repositories on one host reach their `pr list` call
/// at once; the third waits, and runs once one of the first two finishes.
#[tokio::test]
async fn at_most_two_fetches_run_at_once_against_one_forge_host_and_a_third_waits_for_capacity() {
    let stub = stub_forge_cli(json!(login_probe()));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-host");
    let mut entries = login_probe();
    entries.push(list_entry("github.com", "acme", "one", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "two", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "three", Some(&gate)));
    entries.push(issues("acme", "one"));
    entries.push(issues("acme", "two"));
    entries.push(issues("acme", "three"));
    stub.reprogram(json!(entries));

    repo_on(&h, "github.com", "acme", "one").await;
    repo_on(&h, "github.com", "acme", "two").await;
    repo_on(&h, "github.com", "acme", "three").await;

    let started = |stub: &StubForgeCli| {
        [
            listed(stub, "github.com", "acme", "one"),
            listed(stub, "github.com", "acme", "two"),
            listed(stub, "github.com", "acme", "three"),
        ]
        .into_iter()
        .filter(|&l| l)
        .count()
    };
    eventually(
        TIMEOUT,
        "two of the three reach the host's cap",
        async || started(&stub) == 2,
    )
    .await;
    tokio::time::sleep(QUIET).await;
    assert_eq!(started(&stub), 2, "a third fetch must wait for capacity");

    std::fs::write(&gate, "").unwrap();
    eventually(
        TIMEOUT,
        "the third fetch, once capacity frees",
        async || started(&stub) == 3,
    )
    .await;
}

/// A second forge host's own fetch never waits on the first host's cap,
/// while it stands at its own.
#[tokio::test]
async fn a_different_forge_host_has_its_own_independent_capacity() {
    let stub = stub_forge_cli(json!(login_probe()));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-host");
    let mut entries = login_probe();
    entries.push(list_entry("github.com", "acme", "one", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "two", Some(&gate)));
    entries.push(list_entry("github.other.test", "acme", "solo", None));
    entries.push(issues("acme", "one"));
    entries.push(issues("acme", "two"));
    entries.push(issues("acme", "solo"));
    stub.reprogram(json!(entries));

    repo_on(&h, "github.com", "acme", "one").await;
    repo_on(&h, "github.com", "acme", "two").await;
    eventually(TIMEOUT, "the busy host's own two fetches", async || {
        listed(&stub, "github.com", "acme", "one") && listed(&stub, "github.com", "acme", "two")
    })
    .await;

    repo_on(&h, "github.other.test", "acme", "solo").await;
    eventually(
        TIMEOUT,
        "the other host's fetch, unimpeded by the first host's full cap",
        async || listed(&stub, "github.other.test", "acme", "solo"),
    )
    .await;

    std::fs::write(&gate, "").unwrap();
}

/// A fetch that fails releases its host's capacity at once: a third
/// repository queued behind two full slots runs the moment one of them
/// fails, without waiting for the other, still-blocked one.
#[tokio::test]
async fn a_failed_fetch_releases_its_hosts_capacity_for_a_queued_one() {
    let stub = stub_forge_cli(json!(login_probe()));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-host");
    let mut entries = login_probe();
    // More specific than the generic entry below, so this one answers `b`
    // alone: an immediate refusal, no `wait_for`, freeing its slot at once.
    let failing =
        json!({"args": ["pr", "list", "--repo", "github.com/acme/b"], "exit": 1, "stderr": "boom"});
    entries.push(failing);
    entries.push(list_entry("github.com", "acme", "a", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "c", Some(&gate)));
    entries.push(issues("acme", "a"));
    entries.push(issues("acme", "b"));
    entries.push(issues("acme", "c"));
    stub.reprogram(json!(entries));

    repo_on(&h, "github.com", "acme", "a").await;
    repo_on(&h, "github.com", "acme", "b").await;
    eventually(
        TIMEOUT,
        "a's blocked fetch and b's failed one",
        async || {
            listed(&stub, "github.com", "acme", "a") && listed(&stub, "github.com", "acme", "b")
        },
    )
    .await;

    // `b`'s failure already freed its slot; `c` waits on it alone, not on
    // `a`'s own, still-blocked one.
    repo_on(&h, "github.com", "acme", "c").await;
    eventually(
        TIMEOUT,
        "c's fetch, once b's failure freed its slot",
        async || listed(&stub, "github.com", "acme", "c"),
    )
    .await;

    std::fs::write(&gate, "").unwrap();
}

/// A fetch cancelled by disabling its integration releases its host's
/// capacity the same way a finished or failed one does.
#[tokio::test]
async fn a_cancelled_fetch_releases_its_hosts_capacity_for_a_queued_one() {
    let stub = stub_forge_cli(json!(login_probe()));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-host");
    let mut entries = login_probe();
    entries.push(list_entry("github.com", "acme", "a", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "b", Some(&gate)));
    entries.push(list_entry("github.com", "acme", "c", Some(&gate)));
    entries.push(issues("acme", "a"));
    entries.push(issues("acme", "b"));
    entries.push(issues("acme", "c"));
    stub.reprogram(json!(entries));

    repo_on(&h, "github.com", "acme", "a").await;
    let b = repo_on(&h, "github.com", "acme", "b").await;
    eventually(TIMEOUT, "a and b fill the host's cap", async || {
        listed(&stub, "github.com", "acme", "a") && listed(&stub, "github.com", "acme", "b")
    })
    .await;

    repo_on(&h, "github.com", "acme", "c").await;
    tokio::time::sleep(QUIET).await;
    assert!(
        !listed(&stub, "github.com", "acme", "c"),
        "c must wait while a and b hold the only two slots"
    );

    // Disabling b cancels its worker mid-fetch (026), which must drop its
    // permit rather than hold the slot for a worker that no longer runs.
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{b}"),
                json!({"forge": {"enabled": false}}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(
        TIMEOUT,
        "c's fetch, once disabling b freed its slot",
        async || listed(&stub, "github.com", "acme", "c"),
    )
    .await;

    std::fs::write(&gate, "").unwrap();
}

/// A repository's own forge host can change after its worker already
/// started — a remote edit, re-enabled under the new identity in the same
/// request (027 rule 9) — and the fetch that follows must wait on the new
/// host's own capacity rather than slip through on whatever host its
/// worker happened to start on.
#[tokio::test]
async fn a_repository_moved_to_a_full_host_waits_for_that_hosts_capacity() {
    let stub = stub_forge_cli(json!(login_probe()));
    let h = harness().forge_cli(&stub).await;
    let gate = h.dir.path().join("release-host");
    let mut entries = login_probe();
    entries.push(list_entry("github.com", "acme", "z", None));
    entries.push(list_entry("github.other.test", "acme", "x", Some(&gate)));
    entries.push(list_entry("github.other.test", "acme", "y", Some(&gate)));
    entries.push(list_entry("github.other.test", "acme", "z", Some(&gate)));
    entries.push(issues("acme", "z"));
    entries.push(issues("acme", "x"));
    entries.push(issues("acme", "y"));
    stub.reprogram(json!(entries));

    let z = repo_on(&h, "github.com", "acme", "z").await;
    eventually(
        TIMEOUT,
        "z's first fetch, still on its original host",
        async || listed(&stub, "github.com", "acme", "z"),
    )
    .await;

    repo_on(&h, "github.other.test", "acme", "x").await;
    repo_on(&h, "github.other.test", "acme", "y").await;
    eventually(
        TIMEOUT,
        "x and y fill the destination host's own cap",
        async || {
            listed(&stub, "github.other.test", "acme", "x")
                && listed(&stub, "github.other.test", "acme", "y")
        },
    )
    .await;

    let repo = h.store.get_repository(&z).await.unwrap();
    sh(
        std::path::Path::new(&repo.path),
        "git remote set-url origin https://github.other.test/acme/z.git",
    );
    let _: Value = h
        .json(
            put_json(
                &format!("/v1/repositories/{z}"),
                json!({"forge": {"enabled": true}}),
            ),
            StatusCode::OK,
        )
        .await;

    tokio::time::sleep(QUIET).await;
    assert!(
        !listed(&stub, "github.other.test", "acme", "z"),
        "z must wait for its new host's own capacity, not slip through on its old host"
    );

    std::fs::write(&gate, "").unwrap();
    eventually(
        TIMEOUT,
        "z's fetch, once its new host's capacity frees",
        async || listed(&stub, "github.other.test", "acme", "z"),
    )
    .await;
}
