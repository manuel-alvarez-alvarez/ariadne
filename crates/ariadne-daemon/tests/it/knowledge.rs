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
    KnowledgeFailureDto, KnowledgeGraphDto, KnowledgeHitDto, KnowledgeImpactDto,
    KnowledgeIndexedDto, KnowledgeInteractionGroupDto, KnowledgeMapDto, KnowledgeOutlineEntryDto,
    KnowledgePathDto, KnowledgeState, KnowledgeStatusDto, KnowledgeSymbolDto,
};
use ariadne_api::stream::DomainEvent;
use ariadne_core::{Actor, Seat, SessionStatus, TaskStatus};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::knowledge::Knowledge;
use ariadne_knowledge::State;
use ariadne_store::{NewTask, NewTaskAgent, Repository, author_branch};
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
    assert_eq!(status.failures, []);

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

    // And the map of the one file, under the budget it was asked for.
    let map: KnowledgeMapDto = h
        .get(&format!(
            "/v1/knowledge/map?repository={}&budget=200",
            repo.id
        ))
        .await;
    assert_eq!(map.repository_id, repo.id);
    assert_eq!(map.git_ref, "main");
    assert_eq!(map.files, 1);
    assert!(map.tokens <= 200, "{map:?}");
    assert_eq!(
        map.text,
        "lib.rs\n  2-4 function add pub fn add(a: i32, b: i32) -> i32\n"
    );

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

/// The file graph route returns its public shape for one repository and
/// returns not found for an unknown repository.
#[tokio::test]
async fn the_file_graph_route_returns_its_contract_and_rejects_an_unknown_repository() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;
    indexed(&mut rx, &repo.id, "main", None).await;

    let graph: KnowledgeGraphDto = h
        .get(&format!(
            "/v1/knowledge/graph?repository={}&limit=1",
            repo.id
        ))
        .await;
    assert_eq!(graph.repository_id, repo.id);
    assert_eq!(graph.git_ref, "main");
    assert_eq!(graph.total_nodes, 1);
    assert!(!graph.truncated);
    assert_eq!(graph.nodes.len(), 1);
    assert_eq!(graph.nodes[0].path, "lib.rs");
    assert_eq!(graph.nodes[0].language, "rust");
    assert_eq!(graph.nodes[0].symbols, 1);
    assert!(graph.edges.is_empty());

    h.error(
        get("/v1/knowledge/graph?repository=missing"),
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

/// An author reads its own branch, while a reviewer reads the first author
/// branch unless it names another author branch.
#[tokio::test]
async fn authors_and_reviewers_read_their_correct_task_branch() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = code_repo(&h, "task-branches");
    let repository = h.repository(&repo).await;
    indexed(&mut rx, &repository.id, "main", None).await;
    let goal = h.goal_on(&repository, test_pin()).await;
    let author = || NewTaskAgent::new(Seat::Author, ["coding"], test_pin());
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repository.id.clone(),
            title: "two authors".into(),
            description: "read branches".into(),
            agents: [
                author(),
                author(),
                NewTaskAgent::new(Seat::Reviewer, ["code-review"], test_pin()),
            ]
            .into(),
            depends_on: vec![],
            landing: None,
            permission_mode: None,
        })
        .await
        .unwrap();
    let authors = h.store.list_task_authors(&task.id).await.unwrap();
    let reviewer = h
        .store
        .list_task_reviewers(&task.id)
        .await
        .unwrap()
        .remove(0);
    let second_branch = author_branch(&task.branch, authors[1].ordinal);

    sh(&repo, &format!("git checkout -q -b {}", task.branch));
    std::fs::write(repo.join("first.rs"), "pub fn first_author() {}\n").unwrap();
    commit(&repo, "first-author");
    sh(&repo, &format!("git checkout -q -b {second_branch}"));
    std::fs::write(repo.join("second.rs"), "pub fn second_author() {}\n").unwrap();
    commit(&repo, "second-author");
    sh(&repo, "git checkout -q main");
    h.state.knowledge.index(&repository.id, &task.branch);
    indexed(&mut rx, &repository.id, &task.branch, None).await;
    h.state.knowledge.index(&repository.id, &second_branch);
    indexed(&mut rx, &repository.id, &second_branch, None).await;

    let author_two = h
        .session(&goal, Some(&task), Seat::Author, &authors[1].id)
        .await;
    let own: Vec<KnowledgeHitDto> = h
        .json(
            get_as("/v1/knowledge/search?q=second_author", &author_two.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        own.len(),
        1,
        "the second author reads its own branch: {own:?}"
    );

    let reviewer = h
        .session(&goal, Some(&task), Seat::Reviewer, &reviewer.id)
        .await;
    let first: Vec<KnowledgeHitDto> = h
        .json(
            get_as("/v1/knowledge/search?q=first_author", &reviewer.id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        first.len(),
        1,
        "the reviewer reads the first branch: {first:?}"
    );
    let named: Vec<KnowledgeHitDto> = h
        .json(
            get_as(
                &search_uri("second_author", &repository.id, Some(&second_branch)),
                &reviewer.id,
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(
        named.len(),
        1,
        "the reviewer reads the named branch: {named:?}"
    );
}

/// A diff needs definitions from its head, so an unindexed head is refused.
#[tokio::test]
async fn an_unindexed_diff_head_is_refused_by_name() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = code_repo(&h, "unindexed-diff");
    let repository = h.repository(&repo).await;
    indexed(&mut rx, &repository.id, "main", None).await;
    sh(&repo, "git checkout -q -b unindexed-head");
    std::fs::write(repo.join("new.rs"), "pub fn only_in_the_head() {}\n").unwrap();
    commit(&repo, "unindexed-head");
    sh(&repo, "git checkout -q main");

    let refused = h
        .error(
            get(&format!(
                "/v1/knowledge/impact?repository={}&diff=main..unindexed-head",
                repository.id
            )),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "knowledge_not_ready");
    assert!(
        refused.error.message.contains("unindexed-head"),
        "{refused:?}"
    );
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
    let knowledge = Knowledge::start(
        true,
        ariadne_knowledge::default_workers(),
        h.at("knowledge.db"),
        h.store.clone(),
        h.bus.clone(),
    )
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
    assert_eq!(status.failures, []);
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
    assert_eq!(failed.git_ref, "main");
    assert!(failed.error.contains("main"), "{}", failed.error);
    eventually(TIMEOUT, "the status to read failed", || async {
        let status: KnowledgeStatusDto = h
            .get(&format!("/v1/repositories/{}/knowledge", repo.id))
            .await;
        status.state == KnowledgeState::Failed
            && status.failures
                == [KnowledgeFailureDto {
                    git_ref: "main".into(),
                    error: failed.error.clone(),
                }]
    })
    .await;
}

/// The eight reads of one repository at one ref, each with the arguments it
/// needs to get as far as the index.
fn read_uris(repository: &str, git_ref: &str) -> Vec<String> {
    [
        "search?q=add",
        "outline?path=lib.rs",
        "symbol?name=add",
        "impact?symbol=add",
        "path?from=add&to=add",
        "map?",
        "graph?",
        "interactions?",
    ]
    .iter()
    .map(|read| format!("/v1/knowledge/{read}&repository={repository}&git_ref={git_ref}"))
    .collect()
}

/// A ref that cannot answer refuses every read, and the refusal says which
/// of the three cases it is, and names the repository and the ref: an empty
/// answer would read as a fact about the code.
#[tokio::test]
async fn a_read_of_a_ref_that_is_not_ready_is_refused_with_the_text_of_its_case() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;
    indexed(&mut rx, &repo.id, "main", None).await;
    let store = h.state.knowledge.store().expect("enabled");

    // Never indexed: no run of `feature` has started.
    for uri in read_uris(&repo.id, "feature") {
        let refused = h.error(get(&uri), StatusCode::CONFLICT).await;
        assert_eq!(refused.error.code, "knowledge_not_ready", "{uri}");
        assert_eq!(
            refused.error.message,
            format!(
                "the index of feature in repository {} ({}) is not ready: \
                 no index run of this ref has started",
                path.display(),
                repo.id
            ),
            "{uri}"
        );
    }

    // In its first index: a run has started, and the ref has no rows yet.
    store
        .set_ref_state(&repo.id, "feature", State::Indexing, None)
        .await
        .unwrap();
    for uri in read_uris(&repo.id, "feature") {
        let refused = h.error(get(&uri), StatusCode::CONFLICT).await;
        assert_eq!(refused.error.code, "knowledge_not_ready", "{uri}");
        assert_eq!(
            refused.error.message,
            format!(
                "the index of feature in repository {} ({}) is not ready: \
                 the first index run of this ref is in progress, try again later",
                path.display(),
                repo.id
            ),
            "{uri}"
        );
    }

    // Failed: a directory git cannot read.
    let plain = h.repository(&h.at("plain")).await;
    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::KnowledgeFailed(k) if k.repository_id == plain.id),
    )
    .await;
    let DomainEvent::KnowledgeFailed(failed) = event.event else {
        unreachable!("matched knowledge_failed")
    };
    for uri in read_uris(&plain.id, "main") {
        let refused = h.error(get(&uri), StatusCode::CONFLICT).await;
        assert_eq!(refused.error.code, "knowledge_not_ready", "{uri}");
        assert_eq!(
            refused.error.message,
            format!(
                "the index of main in repository {} ({}) is not ready: \
                 the last index run of this ref failed: {}",
                plain.path, plain.id, failed.error
            ),
            "{uri}"
        );
    }
}

/// A ref that has an index answers while a run updates it.
#[tokio::test]
async fn a_read_during_an_update_of_a_good_index_answers() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = h.repository(&code_repo(&h, "repo")).await;
    indexed(&mut rx, &repo.id, "main", None).await;

    h.state
        .knowledge
        .store()
        .expect("enabled")
        .set_ref_state(&repo.id, "main", State::Indexing, None)
        .await
        .unwrap();
    let status: KnowledgeStatusDto = h
        .get(&format!("/v1/repositories/{}/knowledge", repo.id))
        .await;
    assert_eq!(status.state, KnowledgeState::Indexing);

    let hits: Vec<KnowledgeHitDto> = h.get(&search_uri("add", &repo.id, None)).await;
    assert_eq!(hits.len(), 1, "{hits:?}");
    for uri in read_uris(&repo.id, "main") {
        let (answered, body) = h.send(get(&uri)).await;
        assert_eq!(
            answered,
            StatusCode::OK,
            "{uri}: {}",
            String::from_utf8_lossy(&body)
        );
    }
}

/// A run is of one ref, and so is its failure: a good run of another branch
/// leaves the base branch failed, named, and refusing its reads.
#[tokio::test]
async fn a_base_branch_failure_survives_a_good_task_branch_run() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;
    indexed(&mut rx, &repo.id, "main", None).await;

    // The base branch goes, and its next run fails.
    sh(&path, "git checkout -q -b task && git branch -q -D main");
    h.state.knowledge.index(&repo.id, "main");
    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::KnowledgeFailed(k) if k.repository_id == repo.id),
    )
    .await;
    let DomainEvent::KnowledgeFailed(failed) = event.event else {
        unreachable!("matched knowledge_failed")
    };
    assert_eq!(failed.git_ref, "main");

    h.state.knowledge.index(&repo.id, "task");
    indexed(&mut rx, &repo.id, "task", None).await;

    let status: KnowledgeStatusDto = h
        .get(&format!("/v1/repositories/{}/knowledge", repo.id))
        .await;
    assert_eq!(status.state, KnowledgeState::Failed, "{status:?}");
    assert_eq!(
        status.failures,
        [KnowledgeFailureDto {
            git_ref: "main".into(),
            error: failed.error,
        }]
    );
    let refused = h
        .error(
            get(&search_uri("add", &repo.id, Some("main"))),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "knowledge_not_ready");
    let hits: Vec<KnowledgeHitDto> = h.get(&search_uri("add", &repo.id, Some("task"))).await;
    assert_eq!(hits.len(), 1, "the good ref answers: {hits:?}");
}

/// A connection of the test's own to the knowledge store, to break it with.
async fn knowledge_db(path: &Path) -> sqlx::SqlitePool {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(path))
        .await
        .unwrap()
}

/// An in-flight task of `cast` on a branch that holds `branch.rs`, as a
/// start finds it.
async fn in_flight_branch(h: &Harness, repo: &Path, cast: &common::Cast) {
    let worktree = h.at("wt").display().to_string();
    h.store
        .set_task_worktree(&cast.task.id, Some(&worktree))
        .await
        .unwrap();
    h.advance(&cast.task, TaskStatus::InProgress).await;
    commit_on(
        repo,
        &cast.task.branch,
        "branch.rs",
        "pub fn on_the_branch() {}",
    );
}

/// Commit one file on `branch` of `repo`, cut from `main` where it is new,
/// and answer the sha. The checkout goes back to `main`.
fn commit_on(repo: &Path, branch: &str, file: &str, text: &str) -> String {
    sh(
        repo,
        &format!(
            "(git checkout -q {branch} 2>/dev/null || git checkout -q -b {branch}) && \
             printf '{text}\\n' > {file} && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm {file} && \
             git rev-parse HEAD && git checkout -q main"
        ),
    )
}

/// A lenient run drops a branch only where git cannot resolve it. A store
/// that fails is no deleted branch: the rows stay, and the ref reads as
/// failed.
#[tokio::test]
async fn a_lenient_run_with_a_store_error_keeps_the_rows() {
    let h = harness().await;
    let path = code_repo(&h, "repo");
    let cast = h.active_cast().await;
    in_flight_branch(&h, &path, &cast).await;
    let branch = cast.task.branch.clone();

    // The branch as an earlier daemon indexed it, and then a store that
    // refuses every new symbol, under a branch that moved.
    let db = h.at("knowledge.db");
    let earlier = ariadne_knowledge::KnowledgeStore::open(&db).await.unwrap();
    earlier.index(&cast.repo.id, &path, &branch).await.unwrap();
    drop(earlier);
    let pool = knowledge_db(&db).await;
    sqlx::raw_sql(
        "CREATE TRIGGER busy BEFORE INSERT ON symbols
         BEGIN SELECT RAISE(ABORT, 'the store is busy'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    commit_on(&path, &branch, "moved.rs", "pub fn moved() {}");

    let mut rx = h.bus.subscribe();
    let knowledge = Knowledge::start(
        true,
        ariadne_knowledge::default_workers(),
        db,
        h.store.clone(),
        h.bus.clone(),
    )
    .await
    .unwrap();
    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::KnowledgeFailed(k) if k.repository_id == cast.repo.id),
    )
    .await;
    let DomainEvent::KnowledgeFailed(failed) = event.event else {
        unreachable!("matched knowledge_failed")
    };
    assert_eq!(failed.git_ref, branch);
    assert!(failed.error.contains("the store is busy"), "{failed:?}");

    let status = knowledge
        .store()
        .expect("enabled")
        .status(&cast.repo.id)
        .await
        .unwrap();
    let kept = status
        .refs
        .iter()
        .find(|r| r.git_ref == branch)
        .unwrap_or_else(|| panic!("the rows of the branch went: {status:?}"));
    assert_eq!(kept.files, 2, "lib.rs and branch.rs: {status:?}");
    let failures: Vec<&str> = status.failures.iter().map(|f| f.git_ref.as_str()).collect();
    assert_eq!(failures, [branch.as_str()]);
}

/// A follower that fell behind the event stream lost branch moves it cannot
/// get back, so it queues what a start queues: every base branch and every
/// in-flight task branch.
#[tokio::test]
async fn a_lag_queues_the_branches_again() {
    let h = harness().await;
    let path = code_repo(&h, "repo");
    let cast = h.active_cast().await;
    in_flight_branch(&h, &path, &cast).await;
    let branch = cast.task.branch.clone();

    // A bus of the knowledge base's own: nothing but this test publishes on
    // it, and four events fill it.
    let bus = EventBus::with_capacity(4);
    let mut rx = bus.subscribe();
    let _knowledge = Knowledge::start(
        true,
        ariadne_knowledge::default_workers(),
        h.at("knowledge.db"),
        h.store.clone(),
        bus.clone(),
    )
    .await
    .unwrap();
    indexed(&mut rx, &cast.repo.id, "main", None).await;
    indexed(&mut rx, &cast.repo.id, &branch, None).await;

    // Both branches move with no event, and the follower then misses more
    // events than the bus holds. Nothing awaits in between, so the follower
    // reads none of them before they are gone.
    let base_head = commit_on(&path, "main", "later.rs", "pub fn later() {}");
    let branch_head = commit_on(&path, &branch, "moved.rs", "pub fn moved() {}");
    for _ in 0..16 {
        bus.publish(BusEvent {
            event: DomainEvent::KnowledgeIndexed(KnowledgeIndexedDto {
                repository_id: "no-repository".into(),
                git_ref: "main".into(),
                commit: String::new(),
                files: 0,
                symbols: 0,
            }),
            goal_id: None,
            task_id: None,
            recorded: None,
        });
    }
    let mut rx = bus.subscribe();

    indexed(&mut rx, &cast.repo.id, "main", Some(&base_head)).await;
    indexed(&mut rx, &cast.repo.id, &branch, Some(&branch_head)).await;
}

/// A diff range of the wrong form, or with an end that names no commit, is a
/// wrong request. A git that fails on a right range is the daemon's failure.
#[tokio::test]
async fn a_git_failure_on_a_right_diff_range_answers_5xx_and_a_wrong_range_4xx() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let path = code_repo(&h, "repo");
    let repo = h.repository(&path).await;
    indexed(&mut rx, &repo.id, "main", None).await;
    let impact = |range: &str| {
        get(&format!(
            "/v1/knowledge/impact?repository={}&git_ref=main&diff={range}",
            repo.id
        ))
    };

    for wrong in [
        "main",
        "main..no-such-branch",
        "main..--output=x",
        "main%20..%20next",
        "main...next",
        "main....next",
    ] {
        let refused = h.error(impact(wrong), StatusCode::BAD_REQUEST).await;
        assert_eq!(refused.error.code, "invalid_request", "{wrong}");
    }
    let answered: Vec<KnowledgeImpactDto> = h.json(impact("main..main"), StatusCode::OK).await;
    assert!(
        answered.is_empty(),
        "`main` changes no definition: {answered:?}"
    );

    // The same range, in a repository git can no longer read.
    std::fs::rename(path.join(".git"), path.join("git-gone")).unwrap();
    let failed = h
        .error(impact("main..main"), StatusCode::INTERNAL_SERVER_ERROR)
        .await;
    assert_eq!(failed.error.code, "internal_error");
}

/// A store that fails is the daemon's own failure: a 5xx, which no client
/// reads as a wrong argument.
#[tokio::test]
async fn a_store_error_answers_5xx() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let repo = h.repository(&code_repo(&h, "repo")).await;
    indexed(&mut rx, &repo.id, "main", None).await;

    let pool = knowledge_db(&h.launcher.cfg.knowledge_db_path()).await;
    sqlx::raw_sql("DROP TABLE files")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let failed = h
        .error(
            get(&search_uri("add", &repo.id, None)),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .await;
    assert_eq!(failed.error.code, "internal_error");
    assert!(
        failed
            .error
            .message
            .starts_with("the knowledge base failed: "),
        "{failed:?}"
    );
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
        "/v1/knowledge/path",
        "/v1/knowledge/interactions",
        "/v1/knowledge/map",
        "/v1/knowledge/graph",
    ] {
        assert!(document["paths"].get(path).is_some(), "no {path}");
    }
    for schema in [
        "KnowledgeGraphDto",
        "KnowledgeGraphNodeDto",
        "KnowledgeGraphEdgeDto",
        "KnowledgeGraphConfidence",
    ] {
        assert!(
            document["components"]["schemas"].get(schema).is_some(),
            "no {schema}"
        );
    }
}

/// The two repositories the interactions are found between: `api`, a Rust
/// service that defines the package `api-types`, a type `Item`, a route
/// `/v1/items/{id}` and a read of `API_TOKEN`; and `web`, a TypeScript front
/// end that depends on `api-types`, names `Item`, requests `/v1/items/42`
/// and sets `API_TOKEN` in its `.env`.
const API_LIB: &str = "\
/// One item.
pub struct Item {
    pub id: u64,
}

pub fn router() -> Router {
    Router::new().route(\"/v1/items/{id}\", get(get_item))
}

pub fn get_item() -> Item {
    let _token = std::env::var(\"API_TOKEN\");
    Item { id: 1 }
}
";

const WEB_CLIENT: &str = "\
export async function fetchItem() {
  const item = new Item();
  const response = await fetch(\"/v1/items/42\");
  return response.json();
}
";

const WEB_PACKAGE: &str = "\
{
  \"name\": \"web\",
  \"dependencies\": {
    \"api-types\": \"^1.0.0\"
  }
}
";

fn write_all(repo: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = repo.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

/// `api` and `web`, each committed on `main` and registered, both indexed.
async fn api_and_web(h: &Harness, rx: &mut Receiver<BusEvent>) -> (Repository, Repository) {
    let api_path = h.git_repo("api");
    write_all(
        &api_path,
        &[
            ("Cargo.toml", "[package]\nname = \"api-types\"\n"),
            ("src/lib.rs", API_LIB),
        ],
    );
    commit(&api_path, "api");
    let web_path = h.git_repo("web");
    write_all(
        &web_path,
        &[
            ("package.json", WEB_PACKAGE),
            (".env", "API_TOKEN=secret\n"),
            ("src/client.ts", WEB_CLIENT),
        ],
    );
    commit(&web_path, "web");
    let api = h.repository(&api_path).await;
    indexed(rx, &api.id, "main", None).await;
    let web = h.repository(&web_path).await;
    indexed(rx, &web.id, "main", None).await;
    (api, web)
}

/// `kind from -> to confidence step` per edge, each end as `repo:path:line
/// symbol` with the repository named rather than its id.
fn interaction_lines(
    groups: &[KnowledgeInteractionGroupDto],
    api: &Repository,
    web: &Repository,
) -> Vec<String> {
    let name = |id: &str| match id {
        _ if id == api.id => "api".to_string(),
        _ if id == web.id => "web".to_string(),
        _ => id.to_string(),
    };
    groups
        .iter()
        .flat_map(|group| {
            group.edges.iter().map(move |edge| {
                format!(
                    "{} {}:{}:{} {} -> {}:{}:{} {} {} {}",
                    group.kind,
                    name(&edge.from.repository_id),
                    edge.from.path,
                    edge.from.line,
                    edge.from.symbol,
                    name(&edge.to.repository_id),
                    edge.to.path,
                    edge.to.line,
                    edge.to.symbol,
                    edge.confidence,
                    edge.step
                )
            })
        })
        .collect()
}

/// `interactions` for `web` lists one edge of each kind with its two ends,
/// its confidence and the step that joined them: the dependency by name is a
/// guess, the reference is a guess, the route use with a wildcard segment is
/// a guess, and the variable is exact. `api` lists the same edges from its
/// side. Removing the dependency and reading `web` again removes the
/// `depends_on` and `references` edges, but keeps the route and variable.
#[tokio::test]
async fn interactions_between_two_repositories_are_listed_by_kind() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let (api, web) = api_and_web(&h, &mut rx).await;

    let expected = [
        "depends_on web:package.json:4 api-types -> api:Cargo.toml:2 api-types heuristic name",
        "references web:src/client.ts:2 fetchItem -> api:src/lib.rs:2 Item heuristic name",
        "calls_route web:src/client.ts:3 fetchItem -> api:src/lib.rs:10 get_item heuristic route",
        "sets_env web:.env:1 API_TOKEN -> api:src/lib.rs:11 get_item exact name",
    ];
    let groups: Vec<KnowledgeInteractionGroupDto> = h
        .get(&format!("/v1/knowledge/interactions?repository={}", web.id))
        .await;
    assert_eq!(interaction_lines(&groups, &api, &web), expected);
    let kinds: Vec<&str> = groups.iter().map(|group| group.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["depends_on", "references", "calls_route", "sets_env"]
    );

    let from_api: Vec<KnowledgeInteractionGroupDto> = h
        .get(&format!("/v1/knowledge/interactions?repository={}", api.id))
        .await;
    assert_eq!(
        interaction_lines(&from_api, &api, &web),
        expected,
        "the same edges, seen from the other end"
    );

    // The manifest link and its name reference go, but the other edges stay.
    std::fs::write(
        Path::new(&web.path).join("package.json"),
        "{\n  \"name\": \"web\"\n}\n",
    )
    .unwrap();
    let head = commit(Path::new(&web.path), "no-dependency");
    h.state.knowledge.index(&web.id, "main");
    indexed(&mut rx, &web.id, "main", Some(&head)).await;
    let groups: Vec<KnowledgeInteractionGroupDto> = h
        .get(&format!("/v1/knowledge/interactions?repository={}", web.id))
        .await;
    assert_eq!(interaction_lines(&groups, &api, &web), expected[2..]);
}

/// `interactions` is empty when two repositories only share a symbol name
/// and no manifest, route or variable joins them.
#[tokio::test]
async fn interactions_are_empty_without_a_true_repository_relation() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let caller_path = h.git_repo("caller");
    write_all(
        &caller_path,
        &[(
            "src/client.ts",
            "export function useSharedThing() { return new SharedThing(); }\n",
        )],
    );
    commit(&caller_path, "caller");
    let definitions_path = h.git_repo("definitions");
    write_all(
        &definitions_path,
        &[("src/lib.rs", "pub struct SharedThing;\n")],
    );
    commit(&definitions_path, "definitions");
    let caller = h.repository(&caller_path).await;
    indexed(&mut rx, &caller.id, "main", None).await;
    let definitions = h.repository(&definitions_path).await;
    indexed(&mut rx, &definitions.id, "main", None).await;

    let groups: Vec<KnowledgeInteractionGroupDto> = h
        .get(&format!(
            "/v1/knowledge/interactions?repository={}",
            caller.id
        ))
        .await;
    assert!(groups.is_empty(), "{groups:#?}");
}

/// `impact --diff` for a change to the route handler in `api` lists the
/// call site in `web`, under `web`, and says the route joined them.
#[tokio::test]
async fn a_route_handler_change_reaches_the_call_site_in_the_other_repository() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let (api, web) = api_and_web(&h, &mut rx).await;
    let api_path = Path::new(&api.path);
    let before = sh(api_path, "git rev-parse main");

    std::fs::write(
        api_path.join("src/lib.rs"),
        API_LIB.replace("    Item { id: 1 }", "    let _ = 2;\n    Item { id: 1 }"),
    )
    .unwrap();
    let after = commit(api_path, "handler");
    h.state.knowledge.index(&api.id, "main");
    indexed(&mut rx, &api.id, "main", Some(&after)).await;

    let impact: Vec<KnowledgeImpactDto> = h
        .get(&format!(
            "/v1/knowledge/impact?repository={}&diff={before}..{after}",
            api.id
        ))
        .await;
    assert_eq!(
        impact
            .iter()
            .map(|impact| impact.symbol.name.as_str())
            .collect::<Vec<_>>(),
        ["get_item"],
        "{impact:?}"
    );
    let callers: Vec<(i64, &str, &str, i64, &str, &str, &str)> = impact[0]
        .callers
        .iter()
        .map(|caller| {
            (
                caller.depth,
                caller.repository_id.as_str(),
                caller.path.as_str(),
                caller.line,
                caller.name.as_str(),
                caller.confidence.as_str(),
                caller.step.as_str(),
            )
        })
        .collect();
    assert_eq!(
        callers,
        [(
            1,
            web.id.as_str(),
            "src/client.ts",
            1,
            "fetchItem",
            "heuristic",
            "route"
        )]
    );
}

#[tokio::test]
async fn a_path_crosses_a_route_into_the_other_repository() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let (api, web) = api_and_web(&h, &mut rx).await;

    let path: KnowledgePathDto = h
        .get(&format!(
            "/v1/knowledge/path?repository={}&from=fetchItem&to=get_item",
            web.id
        ))
        .await;
    assert_eq!(path.hops.len(), 2, "{path:?}");
    assert!(path.skipped.is_empty(), "{path:?}");
    assert_eq!(path.hops[0].repository_id, web.id);
    assert_eq!(path.hops[0].path, "src/client.ts");
    assert_eq!(path.hops[0].name, "fetchItem");
    assert_eq!(path.hops[0].edge_kind, None);
    assert_eq!(path.hops[1].repository_id, api.id);
    assert_eq!(path.hops[1].path, "src/lib.rs");
    assert_eq!(path.hops[1].name, "get_item");
    assert_eq!(path.hops[1].edge_kind.as_deref(), Some("calls_route"));
    assert_eq!(path.hops[1].confidence.as_deref(), Some("heuristic"));
}

/// `symbol Item --detail context` from `api` lists the `web` reference under
/// `web`, as a guess the name alone answered.
#[tokio::test]
async fn a_type_named_in_the_other_repository_lists_that_reference_as_a_guess() {
    let h = harness().knowledge().await;
    let mut rx = h.bus.subscribe();
    let (api, web) = api_and_web(&h, &mut rx).await;

    let found: Vec<KnowledgeSymbolDto> = h
        .get(&format!(
            "/v1/knowledge/symbol?name=Item&repository={}&detail=context",
            api.id
        ))
        .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].repository_id, api.id);
    let context = found[0].context.as_ref().expect("a context");
    let references: Vec<String> = context
        .references
        .iter()
        .map(|end| {
            format!(
                "{}:{}:{} {} {} via {}",
                end.repository_id,
                end.path,
                end.line,
                end.name,
                end.confidence,
                end.step.as_deref().unwrap_or("-")
            )
        })
        .collect();
    assert_eq!(
        references,
        [format!(
            "{}:src/client.ts:1 fetchItem heuristic via name",
            web.id
        )]
    );
    assert!(context.callers.is_empty(), "{:?}", context.callers);
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
    let feat_head = commit(&repo, "move-b");
    sh(&repo, "git checkout -q main");
    h.state.knowledge.index(&repository.id, "feat-graph");
    indexed(&mut rx, &repository.id, "feat-graph", None).await;
    sh(&repo, "git branch same-feat feat-graph");
    h.state.knowledge.index(&repository.id, "same-feat");
    indexed(&mut rx, &repository.id, "same-feat", Some(&feat_head)).await;

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

    let three_dot = h
        .error(
            get(&format!(
                "/v1/knowledge/impact?repository={}&diff=main...feat-graph",
                repository.id
            )),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        three_dot
            .error
            .message
            .contains("a diff is `<base>..<head>`"),
        "{three_dot:?}"
    );

    let mismatched = h
        .error(
            get(&format!(
                "/v1/knowledge/impact?repository={}&diff=main..feat-graph&git_ref=main",
                repository.id
            )),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        mismatched
            .error
            .message
            .contains("diff head feat-graph does not match git_ref main"),
        "{mismatched:?}"
    );

    let sha_head: Vec<KnowledgeImpactDto> = h
        .get(&format!(
            "/v1/knowledge/impact?repository={}&diff=main..{feat_head}",
            repository.id
        ))
        .await;
    assert_eq!(
        sha_head
            .iter()
            .map(|impact| (impact.symbol.name.as_str(), impact.symbol.line))
            .collect::<Vec<_>>(),
        [("helper", 2), ("b", 5)],
        "an indexed ref at the SHA head answers"
    );
    let named_same_commit: Vec<KnowledgeImpactDto> = h
        .get(&format!(
            "/v1/knowledge/impact?repository={}&diff=main..{feat_head}&git_ref=same-feat",
            repository.id
        ))
        .await;
    assert_eq!(
        named_same_commit
            .iter()
            .map(|impact| (impact.symbol.name.as_str(), impact.symbol.line))
            .collect::<Vec<_>>(),
        [("helper", 2), ("b", 5)],
        "a named ref at the SHA head commit answers"
    );
    sh(&repo, "git checkout -q -b unindexed-sha main");
    std::fs::write(repo.join("unindexed.rs"), "pub fn unindexed_sha() {}\n").unwrap();
    let unindexed_sha = commit(&repo, "unindexed-sha");
    sh(&repo, "git checkout -q main");
    let unindexed_head = h
        .error(
            get(&format!(
                "/v1/knowledge/impact?repository={}&diff=main..{unindexed_sha}",
                repository.id
            )),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(unindexed_head.error.code, "knowledge_not_ready");
    assert!(
        unindexed_head.error.message.contains(&unindexed_sha),
        "{unindexed_head:?}"
    );

    // One of `symbol` and `diff`, never both and never neither.
    for query in ["", "&symbol=b&diff=main...feat-graph"] {
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
