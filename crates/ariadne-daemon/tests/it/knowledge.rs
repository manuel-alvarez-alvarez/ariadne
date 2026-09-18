//! The knowledge base in the daemon: what is indexed when, and what the
//! endpoints answer. `git` is real here, as it is for the branch watch: the
//! commits are made where an author would make them.

use crate::common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};

use ariadne_api::SESSION_HEADER;
use ariadne_api::knowledge::{
    KnowledgeHitDto, KnowledgeImpactDto, KnowledgeIndexedDto, KnowledgeOutlineEntryDto,
    KnowledgeState, KnowledgeStatusDto, KnowledgeSymbolDto,
};
use ariadne_api::stream::DomainEvent;
use ariadne_core::{Actor, SessionStatus, TaskStatus};
use ariadne_daemon::bus::BusEvent;
use ariadne_daemon::knowledge::Knowledge;
use ariadne_knowledge::State;
use tokio::sync::broadcast::Receiver;

use common::{Harness, QUIET, TIMEOUT, eventually, get, harness, next_event, post, sh, test_pin};

/// A git repository on `main` holding one Rust file that defines `add`.
fn code_repo(h: &Harness, name: &str) -> PathBuf {
    let repo = h.git_repo(name);
    std::fs::write(
        repo.join("lib.rs"),
        "/// Adds.\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .unwrap();
    commit(&repo, "code");
    repo
}

/// Commit everything in `dir`, and answer the sha.
fn commit(dir: &Path, message: &str) -> String {
    sh(
        dir,
        &format!(
            "git add . && git -c user.email=t@t -c user.name=t commit -qm {message} && \
             git rev-parse HEAD"
        ),
    )
}

/// The next `knowledge_indexed` of `repository` at `git_ref`, at `commit`
/// where one is named.
async fn indexed(
    rx: &mut Receiver<BusEvent>,
    repository: &str,
    git_ref: &str,
    commit: Option<&str>,
) -> KnowledgeIndexedDto {
    let event = next_event(rx, |e| {
        matches!(&e.event, DomainEvent::KnowledgeIndexed(k)
            if k.repository_id == repository
                && k.git_ref == git_ref
                && commit.is_none_or(|commit| k.commit == commit))
    })
    .await;
    let DomainEvent::KnowledgeIndexed(indexed) = event.event else {
        unreachable!("matched knowledge_indexed")
    };
    indexed
}

fn get_as(uri: &str, session_id: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(SESSION_HEADER, session_id)
        .body(Body::empty())
        .unwrap()
}

fn search_uri(q: &str, repository: &str, git_ref: Option<&str>) -> String {
    match git_ref {
        Some(git_ref) => {
            format!("/v1/knowledge/search?q={q}&repository={repository}&git_ref={git_ref}")
        }
        None => format!("/v1/knowledge/search?q={q}&repository={repository}"),
    }
}

/// Commit until the branch watch answers, so the commit the test is about
/// is one the watch is armed for (see `task_branches::commit_until_seen`).
async fn commit_until_followed(worktree: &Path, rx: &mut Receiver<BusEvent>) {
    for n in 0..40 {
        std::fs::write(worktree.join(format!("armed{n}.txt")), "armed").unwrap();
        let head = commit(worktree, &format!("armed{n}"));
        let seen = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                if let DomainEvent::TaskBranchUpdated(dto) =
                    rx.recv().await.expect("event bus closed").event
                {
                    return dto.head;
                }
            }
        })
        .await;
        if let Ok(seen) = seen {
            assert_eq!(seen, head, "the watch reported a head nobody committed");
            return;
        }
    }
    panic!("the branch watch never reported a commit");
}

/// Registering a repository indexes its base branch, and the status, the
/// search and the outline read it back.
#[tokio::test]
async fn registering_a_repository_indexes_its_base_branch() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;

    let event = indexed(&mut rx, &repo.id, "main", None).await;
    assert_eq!(event.commit, sh(&path, "git rev-parse main"));
    assert_eq!(event.files, 1, "{event:?}");
    assert_eq!(event.symbols, 1);

    let status: KnowledgeStatusDto = h
        .get(&format!("/v1/repositories/{}/knowledge", repo.id))
        .await;
    assert_eq!(status.repository_id, repo.id);
    assert_eq!(status.state, KnowledgeState::Idle);
    assert_eq!(status.refs.len(), 1);
    assert_eq!(status.refs[0].git_ref, "main");
    assert_eq!(status.refs[0].commit, event.commit);
    assert_eq!(status.files, 1);
    assert_eq!(status.symbols, 1);
    assert_eq!(status.languages.len(), 1);
    assert_eq!(status.languages[0].language, "rust");
    assert_eq!(status.error, None);

    let hits: Vec<KnowledgeHitDto> = h.get(&search_uri("add", &repo.id, None)).await;
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].repository_id, repo.id);
    assert_eq!(hits[0].path, "lib.rs");
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].kind, "function");
    assert_eq!(hits[0].name, "add");
    assert_eq!(hits[0].signature, "pub fn add(a: i32, b: i32) -> i32");

    let outline: Vec<KnowledgeOutlineEntryDto> = h
        .get(&format!(
            "/v1/knowledge/outline?repository={}&path=lib.rs",
            repo.id
        ))
        .await;
    assert_eq!(outline.len(), 1, "{outline:?}");
    assert_eq!(outline[0].name, "add");
    assert_eq!((outline[0].start_line, outline[0].end_line), (2, 4));

    // A file that is not there is said to be not there.
    h.error(
        get(&format!(
            "/v1/knowledge/outline?repository={}&path=nothing.rs",
            repo.id
        )),
        StatusCode::NOT_FOUND,
    )
    .await;
}

/// A task branch head move indexes the new commit: `search_code` with the
/// task branch ref finds a symbol added on that branch, the base ref does
/// not, and the author's own default is its branch.
#[tokio::test]
async fn a_task_branch_head_move_indexes_the_new_commit() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    code_repo(&h, "repo");
    let cast = h.active_cast().await;
    indexed(&mut rx, &cast.repo.id, "main", None).await;

    let author = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the author's first turn to end", || async {
        h.session_status(&author).await == SessionStatus::Idle
    })
    .await;
    let task = h.store.get_task(&cast.task.id).await.unwrap();
    let worktree = PathBuf::from(task.worktree_path.clone().expect("a worktree"));
    commit_until_followed(&worktree, &mut rx).await;

    std::fs::write(
        worktree.join("extra.rs"),
        "pub fn only_on_the_branch() {}\n",
    )
    .unwrap();
    let head = commit(&worktree, "branch");
    let event = indexed(&mut rx, &cast.repo.id, &task.branch, Some(&head)).await;
    assert_eq!(event.files, 2, "{event:?}");

    let on_branch: Vec<KnowledgeHitDto> = h
        .get(&search_uri(
            "only_on_the_branch",
            &cast.repo.id,
            Some(&task.branch),
        ))
        .await;
    assert_eq!(on_branch.len(), 1, "{on_branch:?}");
    assert_eq!(on_branch[0].path, "extra.rs");
    let on_base: Vec<KnowledgeHitDto> = h
        .get(&search_uri(
            "only_on_the_branch",
            &cast.repo.id,
            Some("main"),
        ))
        .await;
    assert!(on_base.is_empty(), "{on_base:?}");

    let own: Vec<KnowledgeHitDto> = h
        .json(
            get_as("/v1/knowledge/search?q=only_on_the_branch", &author.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(own.len(), 1, "the author reads its own branch: {own:?}");
    let own_outline: Vec<KnowledgeOutlineEntryDto> = h
        .json(
            get_as(
                &format!(
                    "/v1/knowledge/outline?repository={}&path=extra.rs",
                    cast.repo.id
                ),
                &author.id,
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(own_outline.len(), 1, "{own_outline:?}");
}

/// A landing indexes the base branch again, at the commit it landed, and
/// the task's branch leaves the index: the base branch has all of it.
#[tokio::test]
async fn a_landing_indexes_the_base_branch_again() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let cast = h.active_cast().await;
    indexed(&mut rx, &cast.repo.id, "main", None).await;

    // The task's branch, indexed as a head move would have it.
    sh(
        &path,
        &format!(
            "git checkout -q -b {} && printf 'pub fn on_the_branch() {{}}\\n' > branch.rs && \
             git add . && git -c user.email=t@t -c user.name=t commit -qm branch && \
             git checkout -q main",
            cast.task.branch
        ),
    );
    h.state.knowledge.index(&cast.repo.id, &cast.task.branch);
    indexed(&mut rx, &cast.repo.id, &cast.task.branch, None).await;

    std::fs::write(path.join("landed.rs"), "pub fn landed_symbol() {}\n").unwrap();
    let merge = commit(&path, "landed");
    h.advance(&cast.task, TaskStatus::UnderReview).await;
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Approved,
            Actor::Daemon,
            None,
            None,
        )
        .await
        .unwrap();
    h.store
        .transition_task(
            &cast.task.id,
            TaskStatus::Finished,
            Actor::Author,
            None,
            Some(&merge),
        )
        .await
        .unwrap();

    let event = indexed(&mut rx, &cast.repo.id, "main", Some(&merge)).await;
    assert_eq!(event.files, 2, "{event:?}");
    let hits: Vec<KnowledgeHitDto> = h
        .get(&search_uri("landed_symbol", &cast.repo.id, None))
        .await;
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].path, "landed.rs");

    // The branch's rows went before the base branch was read again.
    let status: KnowledgeStatusDto = h
        .get(&format!("/v1/repositories/{}/knowledge", cast.repo.id))
        .await;
    let refs: Vec<&str> = status.refs.iter().map(|r| r.git_ref.as_str()).collect();
    assert_eq!(refs, ["main"], "{status:?}");
}

/// At start, an in-flight task branch that is gone — deleted by hand while
/// the daemon was down — is dropped rather than failing its repository.
#[tokio::test]
async fn a_start_reads_a_task_branch_that_is_gone_without_failing_the_repository() {
    let h = harness().await;
    code_repo(&h, "repo");
    let cast = h.active_cast().await;
    let worktree = h.at("wt").display().to_string();
    h.store
        .set_task_worktree(&cast.task.id, Some(&worktree))
        .await
        .unwrap();
    h.advance(&cast.task, TaskStatus::InProgress).await;

    let mut rx = h.bus.subscribe();
    let knowledge = Knowledge::start(true, h.at("knowledge.db"), h.store.clone(), h.bus.clone())
        .await
        .unwrap();
    indexed(&mut rx, &cast.repo.id, "main", None).await;
    // The lenient run of the missing branch follows the base branch's run
    // at once, and reports nothing.
    tokio::time::sleep(QUIET).await;
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(event.event, DomainEvent::KnowledgeFailed(_)),
            "a missing task branch failed the repository: {:?}",
            event.event
        );
    }
    let status = knowledge
        .store()
        .expect("enabled")
        .status(&cast.repo.id)
        .await
        .unwrap();
    assert_eq!(status.state, State::Idle, "{status:?}");
    assert_eq!(status.error, None);
    let refs: Vec<&str> = status.refs.iter().map(|r| r.git_ref.as_str()).collect();
    assert_eq!(refs, ["main"]);
}

/// A reindex drops the repository's rows and reads it again: it answers
/// 202 at once, and the index comes back whole.
#[tokio::test]
async fn a_reindex_drops_the_rows_and_indexes_again() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;
    let first = indexed(&mut rx, &repo.id, "main", None).await;

    let accepted: KnowledgeStatusDto = h
        .json(
            post(&format!("/v1/repositories/{}/knowledge/reindex", repo.id)),
            StatusCode::ACCEPTED,
        )
        .await;
    assert_eq!(accepted.state, KnowledgeState::Indexing);

    let again = indexed(&mut rx, &repo.id, "main", Some(&first.commit)).await;
    assert_eq!(again.files, 1);
    eventually(TIMEOUT, "the repository to read idle again", || async {
        let status: KnowledgeStatusDto = h
            .get(&format!("/v1/repositories/{}/knowledge", repo.id))
            .await;
        status.state == KnowledgeState::Idle && status.files == 1
    })
    .await;
    let hits: Vec<KnowledgeHitDto> = h.get(&search_uri("add", &repo.id, None)).await;
    assert_eq!(hits.len(), 1, "{hits:?}");
}

/// A session searches the repositories of its goal by default, and every
/// registered repository on `all=true`; a user searches every one.
#[tokio::test]
async fn a_session_searches_its_goals_repositories_and_all_on_request() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let one = h.repository(&code_repo(&h, "one")).await;
    let two = h.repository(&code_repo(&h, "two")).await;
    indexed(&mut rx, &one.id, "main", None).await;
    indexed(&mut rx, &two.id, "main", None).await;
    let goal = h.goal_on(&one, test_pin()).await;
    let session = h.orchestrator_session(&goal).await;

    let repositories = |hits: Vec<KnowledgeHitDto>| -> Vec<String> {
        let mut ids: Vec<String> = hits.into_iter().map(|hit| hit.repository_id).collect();
        ids.sort();
        ids
    };
    let own: Vec<KnowledgeHitDto> = h
        .json(
            get_as("/v1/knowledge/search?q=add", &session.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(repositories(own), std::slice::from_ref(&one.id));

    let all: Vec<KnowledgeHitDto> = h
        .json(
            get_as("/v1/knowledge/search?q=add&all=true", &session.id),
            StatusCode::OK,
        )
        .await;
    let mut both = vec![one.id.clone(), two.id.clone()];
    both.sort();
    assert_eq!(repositories(all), both);

    let user: Vec<KnowledgeHitDto> = h.get("/v1/knowledge/search?q=add").await;
    assert_eq!(repositories(user), both);
}

/// With the knowledge base off, the status says `disabled`, nothing is
/// indexed, and a search is refused.
#[tokio::test]
async fn with_the_knowledge_base_off_the_status_says_disabled_and_nothing_is_indexed() {
    let h = harness().await;
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;

    let status: KnowledgeStatusDto = h
        .get(&format!("/v1/repositories/{}/knowledge", repo.id))
        .await;
    assert_eq!(status.state, KnowledgeState::Disabled);
    assert!(status.refs.is_empty());
    assert_eq!(status.files, 0);
    assert!(
        !h.launcher.cfg.knowledge_db_path().exists(),
        "no store is written"
    );

    let refused = h
        .error(
            get(&search_uri("add", &repo.id, None)),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refused.error.message.contains("knowledge_enabled"),
        "{refused:?}"
    );
    h.error(
        post(&format!("/v1/repositories/{}/knowledge/reindex", repo.id)),
        StatusCode::CONFLICT,
    )
    .await;
}

/// A repository that is not a git repository fails its index run, and the
/// status says why.
#[tokio::test]
async fn a_repository_git_cannot_read_reads_as_failed() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = h.repository(&h.at("plain")).await;
    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::KnowledgeFailed(k) if k.repository_id == repo.id),
    )
    .await;
    let DomainEvent::KnowledgeFailed(failed) = event.event else {
        unreachable!("matched knowledge_failed")
    };
    assert!(failed.error.contains("main"), "{}", failed.error);
    eventually(TIMEOUT, "the status to read failed", || async {
        let status: KnowledgeStatusDto = h
            .get(&format!("/v1/repositories/{}/knowledge", repo.id))
            .await;
        status.state == KnowledgeState::Failed && status.error.is_some()
    })
    .await;
}

#[tokio::test]
async fn every_knowledge_endpoint_is_in_the_openapi_document() {
    let h = harness().await;
    let document: serde_json::Value = h.get("/api-docs/openapi.json").await;
    for path in [
        "/v1/repositories/{id}/knowledge",
        "/v1/repositories/{id}/knowledge/reindex",
        "/v1/knowledge/search",
        "/v1/knowledge/outline",
        "/v1/knowledge/symbol",
        "/v1/knowledge/impact",
    ] {
        assert!(document["paths"].get(path).is_some(), "no {path}");
    }
}

/// `symbol` and `impact` read the graph the index derived: the definition
/// with its callers and its tests, and what a change to it reaches.
#[tokio::test]
async fn the_symbol_and_impact_endpoints_answer_from_the_derived_graph() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = h.git_repo("graph");
    for (path, text) in [
        ("inner/m.rs", "/// Adds.\npub fn b() {}\n"),
        (
            "a.rs",
            "use crate::inner::m::b;\n\npub fn a() {\n    b();\n}\n",
        ),
        (
            "proof.rs",
            "use crate::a::a;\n\n#[test]\nfn a_proof() {\n    a();\n}\n",
        ),
    ] {
        let file = repo.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    commit(&repo, "graph");
    let repository = h.repository(&repo).await;
    indexed(&mut rx, &repository.id, "main", None).await;

    // The outline of a definition, and then its context.
    let outlined: Vec<KnowledgeSymbolDto> = h
        .get(&format!(
            "/v1/knowledge/symbol?name=b&repository={}",
            repository.id
        ))
        .await;
    assert_eq!(outlined.len(), 1, "{outlined:?}");
    assert_eq!(outlined[0].path, "inner/m.rs");
    assert_eq!((outlined[0].start_line, outlined[0].end_line), (2, 2));
    assert_eq!(outlined[0].signature, "pub fn b()");
    assert_eq!(outlined[0].doc.as_deref(), Some("Adds."));
    assert!(outlined[0].source.is_none(), "outline holds no text");
    assert!(outlined[0].context.is_none());

    let sourced: Vec<KnowledgeSymbolDto> = h
        .get(&format!(
            "/v1/knowledge/symbol?name=b&repository={}&detail=source",
            repository.id
        ))
        .await;
    assert_eq!(sourced[0].source.as_deref(), Some("pub fn b() {}"));

    let context: Vec<KnowledgeSymbolDto> = h
        .get(&format!(
            "/v1/knowledge/symbol?name=b&repository={}&detail=context",
            repository.id
        ))
        .await;
    let context = context[0].context.as_ref().expect("a context");
    assert_eq!(
        context
            .callers
            .iter()
            .map(|c| (c.name.as_str(), c.confidence.as_str()))
            .collect::<Vec<_>>(),
        [("a", "exact")]
    );
    assert_eq!(
        context
            .tests
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["a_proof"],
        "a test two edges away"
    );

    let impact: Vec<KnowledgeImpactDto> = h
        .get(&format!(
            "/v1/knowledge/impact?repository={}&symbol=b&depth=2",
            repository.id
        ))
        .await;
    assert_eq!(impact.len(), 1, "{impact:?}");
    assert_eq!(impact[0].symbol.name, "b");
    assert_eq!(
        impact[0]
            .callers
            .iter()
            .map(|c| (c.depth, c.name.as_str()))
            .collect::<Vec<_>>(),
        [(1, "a"), (2, "a_proof")]
    );
    assert!(impact[0].stopped.is_empty());

    // A diff with no ref is read at its own head: the lines of a hunk are the
    // head's, so the definitions they touch have to be the head's too. The
    // branch adds a definition above `b` and moves `b` down, so what the base
    // branch would answer and what the branch answers differ.
    sh(
        &repo,
        "git checkout -q -b feat-graph && \
         printf '/// Adds.\\npub fn helper() {}\\n\\n/// Adds.\\npub fn b() {\\n    helper();\\n}\\n' \
           > inner/m.rs",
    );
    commit(&repo, "move-b");
    sh(&repo, "git checkout -q main");
    h.state.knowledge.index(&repository.id, "feat-graph");
    indexed(&mut rx, &repository.id, "feat-graph", None).await;

    let changed: Vec<KnowledgeImpactDto> = h
        .get(&format!(
            "/v1/knowledge/impact?repository={}&diff=main..feat-graph",
            repository.id
        ))
        .await;
    assert_eq!(
        changed
            .iter()
            .map(|impact| (impact.symbol.name.as_str(), impact.symbol.line))
            .collect::<Vec<_>>(),
        [("helper", 2), ("b", 5)],
        "the branch's definitions, at the branch's lines"
    );
    let callers = |name: &str| -> Vec<String> {
        changed
            .iter()
            .find(|impact| impact.symbol.name == name)
            .expect(name)
            .callers
            .iter()
            .map(|caller| format!("{} {}", caller.depth, caller.name))
            .collect()
    };
    assert_eq!(callers("b"), ["1 a", "2 a_proof"]);
    assert_eq!(callers("helper"), ["1 b", "2 a"]);

    // One of `symbol` and `diff`, never both and never neither.
    for query in ["", "&symbol=b&diff=main..main"] {
        let refused = h
            .error(
                get(&format!(
                    "/v1/knowledge/impact?repository={}{query}",
                    repository.id
                )),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(
            refused.error.message.contains("symbol or diff"),
            "{refused:?}"
        );
    }
}
