//! Integration tests for the forge integration of a repository (025).
//!
//! The daemon reads the forge off the checkout's remote at registration, on
//! every edit and once at start, and the user enables the integration with a
//! pin for each of its two roles. `gh` and `glab` are the stub of
//! [`common::forge`], so nothing here needs either CLI installed.

use crate::common;

use std::path::Path;

use axum::http::StatusCode;
use serde_json::{Value, json};

use ariadne_api::repositories::RepositoryDto;
use ariadne_api::stream::DomainEvent;
use ariadne_core::ForgeKind;

use common::forge::{Invocation, answer, stub_forge_cli};
use common::{Harness, TIMEOUT, eventually, harness, next_event, post_json, put_json, sh};

/// A git checkout under the harness with `url` as its `origin`.
fn checkout_with_origin(h: &Harness, name: &str, url: &str) -> String {
    let repo = h.git_repo(name);
    sh(&repo, &format!("git remote add origin {url}"));
    repo.display().to_string()
}

async fn register(h: &Harness, path: &str, branch: &str) -> RepositoryDto {
    h.json(
        post_json(
            "/v1/repositories",
            json!({"path": path, "base_branch": branch}),
        ),
        StatusCode::CREATED,
    )
    .await
}

fn edit(id: &str, body: Value) -> axum::http::Request<axum::body::Body> {
    put_json(&format!("/v1/repositories/{id}"), body)
}

/// `gh` signed in to github.com as `octocat`.
fn signed_in() -> Value {
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "octocat\n"),
    ])
}

#[tokio::test]
async fn a_github_origin_registers_with_owner_and_name_from_every_url_form() {
    let h = harness().await;
    for (i, url) in [
        "git@github.com:Acme/Widgets.git",
        "git@github.com:acme/widgets",
        "https://github.com/acme/widgets.git",
        "https://github.com/acme/widgets",
        "https://github.com/acme/widgets/",
    ]
    .into_iter()
    .enumerate()
    {
        let path = checkout_with_origin(&h, &format!("repo-{i}"), url);
        let repo = register(&h, &path, "main").await;
        let forge = repo
            .forge
            .unwrap_or_else(|| panic!("no forge read off {url}"));
        assert_eq!(forge.kind, ForgeKind::Github, "{url}");
        assert_eq!(forge.host, "github.com", "{url}");
        assert_eq!(forge.owner, "acme", "{url}");
        assert_eq!(forge.name, "widgets", "{url}");
        assert_eq!(forge.remote, "origin", "{url}");
        assert!(!forge.enabled, "detected, not enabled: {url}");
        assert_eq!(forge.login, None, "{url}");
    }
}

#[tokio::test]
async fn a_checkout_with_no_remote_has_no_forge() {
    let h = harness().await;
    let path = h.git_repo("repo").display().to_string();
    let repo = register(&h, &path, "main").await;
    assert!(repo.forge.is_none());
    let read: RepositoryDto = h.get(&format!("/v1/repositories/{}", repo.id)).await;
    assert!(read.forge.is_none());

    // And a forge cannot be enabled where there is none.
    let refused = h
        .error(
            edit(&repo.id, json!({"forge": {"enabled": true}})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "forge_unavailable");
}

#[tokio::test]
async fn the_only_remote_stands_in_for_a_missing_origin() {
    let h = harness().await;
    let repo = h.git_repo("repo");
    sh(&repo, "git remote add upstream git@gitlab.com:team/app.git");
    let registered = register(&h, &repo.display().to_string(), "main").await;
    let forge = registered.forge.expect("the one remote is read");
    assert_eq!(forge.kind, ForgeKind::Gitlab);
    assert_eq!(forge.remote, "upstream");
    assert_eq!((forge.owner.as_str(), forge.name.as_str()), ("team", "app"));

    // Two remotes and no `origin` name no forge.
    sh(&repo, "git remote add fork git@gitlab.com:me/app.git");
    let edited: RepositoryDto = h
        .json(edit(&registered.id, json!({})), StatusCode::OK)
        .await;
    assert!(edited.forge.is_none());
}

#[tokio::test]
async fn enabling_needs_the_cli_signed_in_and_stores_its_login() {
    let cli = stub_forge_cli(json!([{
        "args": ["auth", "status"], "exit": 1, "stdout": "",
        "stderr": "You are not logged into any GitHub hosts. Run gh auth login.",
    }]));
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/widgets.git");
    let repo = register(&h, &path, "main").await;

    let refused = h
        .error(
            edit(&repo.id, json!({"forge": {"enabled": true}})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "forge_unauthenticated");
    assert!(
        refused
            .error
            .message
            .contains("You are not logged into any GitHub hosts"),
        "the probe's own words: {}",
        refused.error.message
    );
    let read: RepositoryDto = h.get(&format!("/v1/repositories/{}", repo.id)).await;
    assert!(!read.forge.unwrap().enabled, "a refusal writes nothing");

    cli.reprogram(signed_in());
    let mut rx = h.bus.subscribe();
    let enabled: RepositoryDto = h
        .json(
            edit(&repo.id, json!({"forge": {"enabled": true}})),
            StatusCode::OK,
        )
        .await;
    let forge = enabled.forge.unwrap();
    assert!(forge.enabled);
    assert_eq!(forge.login.as_deref(), Some("octocat"));
    assert!(
        cli.invocations().contains(&Invocation {
            program: "gh".into(),
            args: ["auth", "status", "--hostname", "github.com"]
                .map(String::from)
                .to_vec(),
        })
    );

    // Events come in commit order, so the marker edit's event proves that no
    // second enabled one came before it.
    let _: RepositoryDto = h
        .json(
            edit(&repo.id, json!({"description": "marker"})),
            StatusCode::OK,
        )
        .await;
    let mut announced = Vec::new();
    loop {
        let event = next_event(&mut rx, |e| e.event.kind() == "repository_updated").await;
        let DomainEvent::RepositoryUpdated(dto) = event.event else {
            unreachable!("matched on kind above");
        };
        if dto.description.as_deref() == Some("marker") {
            break;
        }
        if let Some(forge) = dto.forge.filter(|f| f.enabled) {
            announced.push(forge.login);
        }
    }
    assert_eq!(announced, [Some("octocat".to_string())]);
}

#[tokio::test]
async fn a_pin_must_be_a_catalog_model_and_a_role_may_have_none() {
    let h = harness().await;
    let path = checkout_with_origin(&h, "repo", "https://github.com/acme/widgets");
    let repo = register(&h, &path, "main").await;

    let refused = h
        .error(
            edit(
                &repo.id,
                json!({"forge": {"babysit_model": "nosuch:model"}}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        refused.error.message.contains("nosuch"),
        "{}",
        refused.error.message
    );

    let pinned: RepositoryDto = h
        .json(
            edit(
                &repo.id,
                json!({"forge": {"babysit_model": "stub:test-model"}}),
            ),
            StatusCode::OK,
        )
        .await;
    let forge = pinned.forge.unwrap();
    assert_eq!(forge.babysit_model.as_deref(), Some("stub:test-model"));
    assert_eq!(forge.review_model, None, "a role with no pin is allowed");
    assert_eq!(forge.review_effort, None);
}

#[tokio::test]
async fn one_forge_repository_is_enabled_on_one_repository_row_at_a_time() {
    let cli = stub_forge_cli(signed_in());
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/widgets.git");
    let first = register(&h, &path, "main").await;
    let second = register(&h, &path, "next").await;

    let _: RepositoryDto = h
        .json(
            edit(&first.id, json!({"forge": {"enabled": true}})),
            StatusCode::OK,
        )
        .await;
    let refused = h
        .error(
            edit(&second.id, json!({"forge": {"enabled": true}})),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refused.error.message.contains(&first.id),
        "names the row that holds it: {}",
        refused.error.message
    );

    let disabled: RepositoryDto = h
        .json(
            edit(&first.id, json!({"forge": {"enabled": false}})),
            StatusCode::OK,
        )
        .await;
    assert!(!disabled.forge.unwrap().enabled);
    let enabled: RepositoryDto = h
        .json(
            edit(&second.id, json!({"forge": {"enabled": true}})),
            StatusCode::OK,
        )
        .await;
    assert!(enabled.forge.unwrap().enabled);
    let enabled_rows = h.store.enabled_forge_integrations().await.unwrap();
    assert_eq!(enabled_rows.len(), 1);
    assert_eq!(enabled_rows[0].repository_id, second.id);
}

#[tokio::test]
async fn disabling_keeps_the_pins() {
    let cli = stub_forge_cli(signed_in());
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/widgets.git");
    let repo = register(&h, &path, "main").await;
    let _: RepositoryDto = h
        .json(
            edit(
                &repo.id,
                json!({"forge": {"enabled": true, "review_model": "stub:test-model"}}),
            ),
            StatusCode::OK,
        )
        .await;
    let disabled: RepositoryDto = h
        .json(
            edit(&repo.id, json!({"forge": {"enabled": false}})),
            StatusCode::OK,
        )
        .await;
    let forge = disabled.forge.unwrap();
    assert!(!forge.enabled);
    assert_eq!(forge.review_model.as_deref(), Some("stub:test-model"));
}

#[tokio::test]
async fn a_changed_remote_is_detected_on_the_next_edit_and_at_daemon_start() {
    let h = harness().await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/one.git");
    let repo = register(&h, &path, "main").await;
    let set_url = |url: &str| {
        sh(
            Path::new(&path),
            &format!("git remote set-url origin {url}"),
        )
    };

    set_url("git@github.com:acme/two.git");
    let edited: RepositoryDto = h
        .json(
            edit(&repo.id, json!({"description": "edited"})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(edited.forge.unwrap().name, "two");

    // The sweep the daemon runs once at start.
    set_url("https://gitlab.com/acme/three.git");
    ariadne_daemon::forge::detect_all(&h.store, &h.launcher.cfg).await;
    let read: RepositoryDto = h.get(&format!("/v1/repositories/{}", repo.id)).await;
    let forge = read.forge.unwrap();
    assert_eq!(
        (forge.kind, forge.name.as_str()),
        (ForgeKind::Gitlab, "three")
    );

    sh(Path::new(&path), "git remote remove origin");
    ariadne_daemon::forge::detect_all(&h.store, &h.launcher.cfg).await;
    let read: RepositoryDto = h.get(&format!("/v1/repositories/{}", repo.id)).await;
    assert!(
        read.forge.is_none(),
        "a remote that went away leaves no forge"
    );
}

#[tokio::test]
async fn another_host_is_the_forge_whose_cli_is_signed_in_to_it() {
    let cli = stub_forge_cli(json!([
        {"program": "gh", "args": ["auth", "status"], "exit": 1, "stdout": ""},
        {"program": "glab", "args": ["auth", "status"], "exit": 0, "stdout": ""},
        {"program": "glab", "args": ["api", "user"], "exit": 0,
         "stdout": "{\"id\": 7, \"username\": \"tanuki\"}"},
    ]));
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@git.corp.example:team/app.git");
    let repo = register(&h, &path, "main").await;
    let forge = repo.forge.clone().expect("glab is signed in to the host");
    assert_eq!(forge.kind, ForgeKind::Gitlab);
    assert_eq!(forge.host, "git.corp.example");

    let enabled: RepositoryDto = h
        .json(
            edit(&repo.id, json!({"forge": {"enabled": true}})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(enabled.forge.unwrap().login.as_deref(), Some("tanuki"));

    // A host neither CLI is signed in to has no forge.
    cli.reprogram(json!([]));
    let path = checkout_with_origin(&h, "other", "git@git.unknown.example:team/app.git");
    assert!(register(&h, &path, "main").await.forge.is_none());
}

/// `gh` signed in, its `auth status` held until `gate` exists.
fn signed_in_behind(gate: &Path) -> Value {
    json!([
        {"args": ["auth", "status"], "exit": 0, "stdout": "",
         "wait_for": gate.display().to_string()},
        answer(&["api", "user"], 0, "octocat\n"),
    ])
}

/// Wait until `n` enables are inside `gh auth status`, past every check the
/// daemon makes before it writes, and then let them all go.
async fn release_when_probing(cli: &common::forge::StubForgeCli, gate: &Path, n: usize) {
    eventually(TIMEOUT, "both enables to probe gh", || async {
        cli.invocations()
            .iter()
            .filter(|call| {
                call.args
                    .starts_with(&["auth".to_string(), "status".to_string()])
            })
            .count()
            >= n
    })
    .await;
    std::fs::write(gate, "").unwrap();
}

/// Two enables of one forge repository on two rows, at once: both pass the
/// check before the probe, and the store's transaction lets one through. The
/// other writes nothing, the edit it carried included.
#[tokio::test]
async fn concurrent_enables_on_two_rows_leave_the_loser_unchanged() {
    let gate_dir = tempfile::tempdir().unwrap();
    let gate = gate_dir.path().join("go");
    let cli = stub_forge_cli(signed_in_behind(&gate));
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/widgets.git");
    let first = register(&h, &path, "main").await;
    let second = register(&h, &path, "next").await;

    let enable = |id: &str, description: &str| {
        h.send(edit(
            id,
            json!({"description": description, "forge": {"enabled": true}}),
        ))
    };
    let ((a, _), (b, _), ()) = tokio::join!(
        enable(&first.id, "first won"),
        enable(&second.id, "second won"),
        release_when_probing(&cli, &gate, 2),
    );
    let mut statuses = [a, b];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);

    let rows = h.store.enabled_forge_integrations().await.unwrap();
    assert_eq!(rows.len(), 1);
    let (winner, loser) = match rows[0].repository_id == first.id {
        true => (&first, &second),
        false => (&second, &first),
    };
    let won: RepositoryDto = h.get(&format!("/v1/repositories/{}", winner.id)).await;
    assert!(won.description.unwrap().ends_with("won"));
    let lost: RepositoryDto = h.get(&format!("/v1/repositories/{}", loser.id)).await;
    assert_eq!(lost.description, None, "the refused edit wrote nothing");
    assert_eq!(lost.updated_at, loser.updated_at);
    assert!(!lost.forge.unwrap().enabled);
}

/// Two registrations of one checkout, each enabling the forge, at once: the
/// refused one leaves no repository row behind.
#[tokio::test]
async fn concurrent_registrations_that_enable_leave_no_repository_for_the_loser() {
    let gate_dir = tempfile::tempdir().unwrap();
    let gate = gate_dir.path().join("go");
    let cli = stub_forge_cli(signed_in_behind(&gate));
    let h = harness().forge_cli(&cli).await;
    let path = checkout_with_origin(&h, "repo", "git@github.com:acme/widgets.git");

    let register_enabled = |branch: &str| {
        h.send(post_json(
            "/v1/repositories",
            json!({"path": path, "base_branch": branch, "forge": {"enabled": true}}),
        ))
    };
    let ((a, _), (b, _), ()) = tokio::join!(
        register_enabled("main"),
        register_enabled("next"),
        release_when_probing(&cli, &gate, 2),
    );
    let mut statuses = [a, b];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);

    let repositories = h.store.list_repositories().await.unwrap();
    assert_eq!(
        repositories.len(),
        1,
        "the refused registration left no row"
    );
    assert!(repositories[0].forge.as_ref().unwrap().enabled);
}
