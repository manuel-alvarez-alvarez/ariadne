//! `ariadne repo ...`

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::repositories::{
    CreateRepositoryRequest, ForgeDto, ForgeTunnelDto, ForgeUpdate, RepositoryDto,
    UpdateRepositoryRequest,
};
use ariadne_client::Client;
use ariadne_core::{Landing, PermissionMode};
use clap::{Args, ValueEnum};
use serde_json::json;

use super::resolve::{self, Kind};
use super::{Subject, confirm, parse_effort_or_default, parse_model};
use crate::cli::values::Spelling;
use crate::output::{
    Column, Format, Kv, UNCAPPED, age, col, empty_state, moment, ok_id_line, print, print_kv,
    print_list, view,
};

/// Columns of `repo ls`. The path is what a repository is, so it stays
/// whatever the terminal's width; the description is the first thing to go.
const LS: &[Column] = &[
    col("id", UNCAPPED).id(),
    col("title", 48).title(),
    col("age", UNCAPPED).rank(4),
    col("branch", 24).rank(3),
    col("permissions", UNCAPPED).rank(2),
    col("landing", UNCAPPED).rank(1),
    col("forge", 40).rank(2),
    col("description", 40).rank(0),
];

/// The forge integration's switch, as `--forge` spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Switch {
    On,
    Off,
}

/// The forge integration of a repository (025): the switch, and the pin of
/// the session that reviews a request asking for the user's review (029).
#[derive(Args, Debug, Default)]
pub(crate) struct ForgeArgs {
    /// Work with the forge the repository's remote is on: on needs `gh` or
    /// `glab` signed in to its host
    #[arg(long, value_enum)]
    forge: Option<Switch>,
    /// What the session that reviews a request runs on: AGENT:MODEL, or ""
    /// for no such session
    #[arg(long, value_name = "MODEL", value_parser = parse_pin_model, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::models))]
    review_model: Option<String>,
    /// The effort that model is run at, or "default"
    #[arg(long, value_name = "EFFORT", value_parser = parse_effort_or_default, add = clap_complete::engine::ArgValueCandidates::new(crate::complete::efforts))]
    review_effort: Option<String>,
}

impl ForgeArgs {
    /// The request's `forge`, or None where no flag of it was given.
    fn update(self) -> Option<ForgeUpdate> {
        let update = ForgeUpdate {
            enabled: self.forge.map(|switch| switch == Switch::On),
            review_model: self.review_model,
            review_effort: self.review_effort,
        };
        let given = update.enabled.is_some()
            || update.review_model.is_some()
            || update.review_effort.is_some();
        given.then_some(update)
    }
}

/// A role's model: `AGENT:MODEL`, or empty to pin none.
fn parse_pin_model(s: &str) -> Result<String, String> {
    match s.is_empty() {
        true => Ok(String::new()),
        false => parse_model(s),
    }
}

#[derive(Subcommand)]
pub(crate) enum RepoCommand {
    /// Register a repository
    Add {
        /// Absolute path of the checkout
        path: String,
        /// Base branch tasks branch off (default: the repo's current branch)
        #[arg(long)]
        branch: Option<String>,
        /// What this repository is, in a line
        #[arg(long)]
        description: Option<String>,
        /// How its agents' ACP permission requests are answered: auto
        /// approves, ask waits for a console answer, learn remembers
        /// approvals, ai lets the AI permission model decide (default: auto)
        #[arg(long, value_parser = Spelling::<PermissionMode>::new())]
        permission_mode: Option<PermissionMode>,
        /// Landing new goals use: none, merge, pull-request, or feature-branch (default: merge)
        #[arg(long, value_parser = Spelling::<Landing>::new())]
        default_landing: Option<Landing>,
        #[command(flatten)]
        forge: ForgeArgs,
    },
    /// List repositories
    Ls,
    /// Show a repository
    Inspect {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
    },
    /// Update a repository
    Update {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
        /// New absolute path of the checkout
        #[arg(long)]
        path: Option<String>,
        /// New base branch
        #[arg(long)]
        branch: Option<String>,
        /// New description, or "" to clear it
        #[arg(long)]
        description: Option<String>,
        /// New permission mode: auto, ask, learn, or ai (the AI permission model decides)
        #[arg(long, value_parser = Spelling::<PermissionMode>::new())]
        permission_mode: Option<PermissionMode>,
        /// New default landing: none, merge, pull-request, or feature-branch
        #[arg(long, value_parser = Spelling::<Landing>::new())]
        default_landing: Option<Landing>,
        #[command(flatten)]
        forge: ForgeArgs,
    },
    /// Delete a repository
    Rm {
        /// Repository id
        #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::complete::repo_ids))]
        id: String,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}

pub(crate) async fn run(client: &Client, cmd: RepoCommand, format: Format) -> Result<()> {
    match cmd {
        RepoCommand::Add {
            path,
            branch,
            description,
            permission_mode,
            default_landing,
            forge,
        } => {
            let repo: RepositoryDto = client
                .post_json(
                    "/v1/repositories",
                    &CreateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        permission_mode,
                        default_landing,
                        forge: forge.update(),
                    },
                )
                .await?;
            print(format, &repo, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "created", &repo.id)
                )
            })?;
        }
        RepoCommand::Ls => {
            let repos: Vec<RepositoryDto> = client.get_json("/v1/repositories").await?;
            let now = chrono::Utc::now();
            print_list(
                format,
                &repos,
                LS,
                |r| ls_row(r, now),
                empty_state("No repositories yet.", Some("ariadne repo add <path>")),
            )?;
        }
        RepoCommand::Inspect { id } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            // An older daemon has no tunnel: the line then reads as a dash.
            let tunnel: Option<ForgeTunnelDto> = match r.forge {
                Some(_) => client.get_json("/v1/forge/tunnel").await.ok(),
                None => None,
            };
            print(format, &r, || print_kv(&inspect_rows(&r, tunnel.as_ref())))?;
        }
        RepoCommand::Update {
            id,
            path,
            branch,
            description,
            permission_mode,
            default_landing,
            forge,
        } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client
                .put_json(
                    &repo_path(&id),
                    &UpdateRepositoryRequest {
                        path,
                        base_branch: branch,
                        description,
                        permission_mode,
                        default_landing,
                        forge: forge.update(),
                    },
                )
                .await?;
            print(format, &r, || {
                println!(
                    "{}",
                    ok_id_line(view().color, view().quiet, "updated", &r.id)
                )
            })?;
        }
        RepoCommand::Rm { id, yes } => {
            let id = resolve::id(client, Kind::Repo, &id).await?;
            let r: RepositoryDto = client.get_json(&repo_path(&id)).await?;
            let subject = Subject::new("repository", &r.path, &r.id);
            confirm("delete", &subject, &rm_question(&r, &subject), yes)?;
            client
                .send_no_content::<()>(http::Method::DELETE, &repo_path(&id), None)
                .await?;
            // The repository is gone, so there is no DTO left to print: what
            // the caller asked about, and that it happened.
            print(format, &json!({"repository": id, "deleted": true}), || {
                println!("{}", ok_id_line(view().color, view().quiet, "deleted", &id))
            })?;
        }
    }
    Ok(())
}

/// One row of `repo ls`.
fn ls_row(r: &RepositoryDto, now: chrono::DateTime<chrono::Utc>) -> Vec<String> {
    vec![
        r.id.clone(),
        r.path.clone(),
        age(&r.created_at, now),
        r.base_branch.clone(),
        r.permission_mode.as_str().into(),
        r.default_landing.as_str().into(),
        r.forge.as_ref().map_or_else(|| "-".into(), forge_label),
        r.description.clone().unwrap_or_else(|| "-".into()),
    ]
}

/// The forge in a few words: `github acme/widgets on`.
fn forge_label(forge: &ForgeDto) -> String {
    let switch = match forge.enabled {
        true => "on",
        false => "off",
    };
    format!(
        "{} {}/{} {switch}",
        forge.kind.as_str(),
        forge.owner,
        forge.name
    )
}

/// A role's pin: the model, and the effort where one was pinned.
fn role_pin(model: Option<&str>, effort: Option<&str>) -> String {
    match (model, effort) {
        (None, _) => "-".into(),
        (Some(model), None) => model.into(),
        (Some(model), Some(effort)) => format!("{model} @ {effort}"),
    }
}

/// What `repo inspect` prints, the forge block included.
fn inspect_rows(r: &RepositoryDto, tunnel: Option<&ForgeTunnelDto>) -> Vec<(&'static str, Kv)> {
    let mut rows = vec![
        ("id", Kv::id(r.id.clone())),
        ("path", r.path.clone().into()),
        ("branch", r.base_branch.clone().into()),
        ("permissions", r.permission_mode.as_str().into()),
        ("default landing", r.default_landing.as_str().into()),
        (
            "description",
            r.description.clone().unwrap_or_else(|| "-".into()).into(),
        ),
    ];
    match &r.forge {
        None => rows.push(("forge", "-".into())),
        Some(forge) => rows.extend([
            ("forge", forge_label(forge).into()),
            ("webhook state", forge.webhook.state.clone().into()),
            (
                "webhook URL",
                forge
                    .webhook
                    .url
                    .clone()
                    .unwrap_or_else(|| "-".into())
                    .into(),
            ),
            (
                "webhook error",
                forge
                    .webhook
                    .error
                    .clone()
                    .unwrap_or_else(|| "-".into())
                    .into(),
            ),
            (
                "webhook last delivery",
                forge
                    .webhook
                    .last_delivery_at
                    .clone()
                    .unwrap_or_else(|| "-".into())
                    .into(),
            ),
            (
                "fetch error",
                forge
                    .webhook
                    .fetch_error
                    .clone()
                    .unwrap_or_else(|| "-".into())
                    .into(),
            ),
            (
                "tunnel",
                tunnel
                    .map_or_else(|| "-".into(), super::forge::label)
                    .into(),
            ),
            (
                "forge remote",
                format!("{} ({})", forge.remote, forge.host).into(),
            ),
            (
                "forge login",
                forge.login.clone().unwrap_or_else(|| "-".into()).into(),
            ),
            (
                "review",
                role_pin(
                    forge.review_model.as_deref(),
                    forge.review_effort.as_deref(),
                )
                .into(),
            ),
        ]),
    }
    rows.extend([
        ("created", Kv::meta(moment(&r.created_at))),
        ("updated", Kv::meta(moment(&r.updated_at))),
    ]);
    rows
}

fn repo_path(id: &str) -> String {
    format!("/v1/repositories/{id}")
}

/// What `repo rm` asks before it deletes: the checkout on disk is untouched,
/// so the question names the registration it is about to drop.
fn rm_question(r: &RepositoryDto, subject: &Subject) -> String {
    format!(
        "Delete the repository {} on {}?",
        subject.named(),
        r.base_branch
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::commands::fixtures;

    #[test]
    fn the_repository_subject_column_is_title() {
        let table = crate::output::render_table(
            LS,
            &[vec![String::new(); LS.len()]],
            &crate::output::View::plain(),
        )
        .expect("table");
        let header = table.lines().next().expect("header");
        assert!(header.contains("TITLE"), "{table}");
        assert!(!header.contains("PATH"), "{table}");
    }

    fn forge(enabled: bool, login: Option<&str>) -> ForgeDto {
        ForgeDto {
            webhook: ariadne_api::repositories::WebhookDto {
                state: "live".into(),
                url: Some("https://hooks.example/webhooks/github/repo".into()),
                error: None,
                last_delivery_at: Some("2026-10-07T10:00:00Z".into()),
                fetch_error: Some("gh: HTTP 502".into()),
            },
            kind: ariadne_core::ForgeKind::Github,
            host: "github.com".into(),
            owner: "acme".into(),
            name: "widgets".into(),
            remote: "origin".into(),
            enabled,
            login: login.map(Into::into),
            review_model: Some("stub:test-model".into()),
            review_effort: Some("high".into()),
        }
    }

    /// `--forge` and the pin flags reach the request; none of them leaves the
    /// forge out of it, and an empty model clears a role's pin.
    #[test]
    fn repo_add_and_update_take_the_forge_flags() {
        #[derive(clap::Parser)]
        struct Repo {
            #[command(subcommand)]
            command: RepoCommand,
        }
        let forge_of = |argv: &[&str]| {
            let parsed = <Repo as clap::Parser>::try_parse_from(argv).expect("parses");
            match parsed.command {
                RepoCommand::Add { forge, .. } | RepoCommand::Update { forge, .. } => {
                    forge.update()
                }
                _ => unreachable!("add or update"),
            }
        };
        let update = forge_of(&[
            "repo",
            "update",
            "01R",
            "--forge",
            "on",
            "--review-model",
            "stub:test-model",
            "--review-effort",
            "high",
        ])
        .expect("the forge flags were given");
        assert_eq!(update.enabled, Some(true));
        assert_eq!(update.review_model.as_deref(), Some("stub:test-model"));
        assert_eq!(update.review_effort.as_deref(), Some("high"));
        let cleared = forge_of(&["repo", "update", "01R", "--review-model", ""]).expect("given");
        assert_eq!(cleared.review_model.as_deref(), Some(""));
        assert!(
            <Repo as clap::Parser>::try_parse_from([
                "repo",
                "update",
                "01R",
                "--babysit-model",
                "x"
            ])
            .is_err(),
            "a request of the user's own runs on its task's author, not on a pin"
        );

        let off = forge_of(&["repo", "add", "/r", "--forge", "off"]).expect("given");
        assert_eq!(off.enabled, Some(false));
        assert!(forge_of(&["repo", "add", "/r"]).is_none());
        assert!(
            <Repo as clap::Parser>::try_parse_from(["repo", "add", "/r", "--review-model", "x"])
                .is_err(),
            "a model names its agent"
        );
    }

    #[test]
    fn repo_ls_shows_the_forge_column() {
        let now = chrono::Utc::now();
        let rows = [
            ls_row(
                &RepositoryDto {
                    forge: Some(forge(true, Some("octocat"))),
                    ..fixtures::repository("01A", "/repos/widgets", "main")
                },
                now,
            ),
            ls_row(&fixtures::repository("01B", "/repos/local", "main"), now),
        ];
        let table =
            crate::output::render_table(LS, &rows, &crate::output::View::plain()).expect("table");
        let mut lines = table.lines();
        assert!(lines.next().unwrap().contains("FORGE"), "{table}");
        assert!(
            lines.next().unwrap().contains("github acme/widgets on"),
            "{table}"
        );
        assert!(!table.contains("github acme/widgets off"), "{table}");
    }

    #[test]
    fn repo_inspect_prints_the_forge_block_with_the_login() {
        let r = RepositoryDto {
            forge: Some(forge(true, Some("octocat"))),
            ..fixtures::repository("01A", "/repos/widgets", "main")
        };
        let tunnel = ForgeTunnelDto {
            enabled: true,
            state: ariadne_api::repositories::TunnelState::Down,
            url: None,
            listen: Some("127.0.0.1:49152".into()),
            since: "2026-10-08T10:00:00.000Z".into(),
            error: Some("connection refused".into()),
        };
        let block = crate::output::kv_block(
            &inspect_rows(&r, Some(&tunnel)),
            &crate::output::View::plain(),
        );
        assert!(
            block
                .lines()
                .any(|l| l.starts_with("tunnel") && l.trim_end().ends_with("down")),
            "the tunnel line follows the webhook block: {block}"
        );
        assert!(block.contains("github acme/widgets on"), "{block}");
        assert!(block.contains("origin (github.com)"), "{block}");
        assert!(block.contains("octocat"), "{block}");
        assert!(
            block.contains("webhook state") && block.contains("live"),
            "{block}"
        );
        assert!(
            block.contains("https://hooks.example/webhooks/github/repo"),
            "{block}"
        );
        assert!(block.contains("webhook error"), "{block}");
        assert!(
            block
                .lines()
                .any(|l| l.starts_with("fetch error") && l.contains("gh: HTTP 502")),
            "{block}"
        );
        assert!(block.contains("2026-10-07T10:00:00Z"), "{block}");
        assert!(
            block
                .lines()
                .any(|l| l.starts_with("review") && l.contains("stub:test-model @ high")),
            "{block}"
        );
        assert!(!block.contains("babysit"), "{block}");

        let none = fixtures::repository("01B", "/repos/local", "main");
        let block =
            crate::output::kv_block(&inspect_rows(&none, None), &crate::output::View::plain());
        assert!(
            block
                .lines()
                .any(|l| l.starts_with("forge") && l.trim_end().ends_with('-'))
        );
    }
}
