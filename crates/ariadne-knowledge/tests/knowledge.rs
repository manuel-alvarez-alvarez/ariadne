//! The index over a real git repository: one fixture file per language,
//! committed and read back through the store.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ariadne_knowledge::{KnowledgeStore, SearchQuery, State};

/// Where the fixture files live: one per language, each holding a
/// definition, a reference to it and a doc comment.
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn sh(dir: &Path, cmd: &str) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed in {}: {cmd}\n{}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A repository on `main` holding every fixture file, in one commit.
fn fixture_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    for entry in std::fs::read_dir(fixtures()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), repo.join(entry.file_name())).unwrap();
    }
    sh(
        &repo,
        "git init -q -b main && git add . && \
         git -c user.email=t@t -c user.name=t commit -qm fixtures",
    );
    repo
}

async fn store(dir: &Path) -> KnowledgeStore {
    KnowledgeStore::open(dir.join("knowledge.db"))
        .await
        .unwrap()
}

fn query(q: &str, scopes: &[(&str, &str)]) -> SearchQuery {
    SearchQuery {
        q: q.to_string(),
        scopes: scopes
            .iter()
            .map(|(repo, git_ref)| (repo.to_string(), git_ref.to_string()))
            .collect(),
        kind: None,
        path: None,
        limit: 20,
    }
}

/// For each language, `search_code` finds the fixture's definition and
/// `outline` lists it with its line range; a Markdown file's headings
/// appear in its outline.
#[tokio::test]
async fn every_language_definition_is_found_and_outlined_with_its_line_range() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;

    let indexed = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(indexed.files, 27, "{indexed:?}");
    assert_eq!(indexed.parsed, 27);

    // (file, definition, kind, first line, last line), the lines read off
    // the fixture files themselves.
    let expected = [
        ("lib.rs", "add", "function", 4, 6),
        ("lib.rs", "bump", "method", 15, 17),
        ("app.ts", "multiply", "function", 2, 4),
        ("app.ts", "scale", "method", 13, 15),
        ("view.tsx", "App", "function", 2, 4),
        ("util.js", "halve", "function", 2, 4),
        ("util.js", "quarter", "function", 6, 6),
        ("widget.jsx", "Widget", "function", 2, 4),
        ("Program.cs", "Greeter", "class", 4, 22),
        ("Program.cs", "Greet", "method", 7, 10),
        ("tool.py", "clamp", "function", 6, 8),
        ("tool.py", "Gauge", "class", 11, 16),
        ("README.md", "Install", "heading", 5, 12),
        ("calc.go", "Add", "function", 4, 6),
        ("Calculator.java", "add", "method", 3, 5),
        ("calc.c", "add", "function", 2, 2),
        ("calc.cpp", "add", "function", 2, 2),
        ("calc.rb", "add", "method", 2, 4),
        ("calc.php", "add", "function", 4, 6),
        ("calc.kt", "add", "function", 3, 5),
        ("calc.swift", "add", "function", 3, 5),
        ("calc.dart", "add", "function", 2, 4),
        ("calc.scala", "add", "function", 3, 5),
        ("calc.bats", "add", "function", 2, 4),
        ("calc.lua", "add", "function", 2, 4),
        ("calc.ex", "add", "function", 2, 4),
    ];
    for (file, name, kind, start, end) in expected {
        let hits = store
            .search(&query(name, &[("repo", "main")]))
            .await
            .unwrap();
        let hit = hits
            .iter()
            .find(|hit| hit.path == file && hit.name == name)
            .unwrap_or_else(|| panic!("search for {name} did not find {file}: {hits:#?}"));
        assert_eq!(hit.kind, kind, "{file}::{name}");
        assert_eq!(hit.line, start, "{file}::{name}");

        let outline = store
            .outline("repo", "main", file)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{file} is not indexed"));
        let entry = outline
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{file}'s outline lacks {name}: {outline:#?}"));
        assert_eq!(
            (entry.start_line, entry.end_line),
            (start, end),
            "{file}::{name}"
        );
        assert_eq!(entry.kind, kind);
    }

    let readme: Vec<String> = store
        .outline("repo", "main", "README.md")
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(readme, ["Fixture", "Install", "Details", "Use"]);

    let status = store.status("repo").await.unwrap();
    assert_eq!(status.files, 27);
    assert_eq!(status.refs.len(), 1);
    assert_eq!(status.refs[0].git_ref, "main");
    assert_eq!(status.refs[0].commit, sh(&repo, "git rev-parse main"));
    let languages: Vec<(&str, i64)> = status
        .languages
        .iter()
        .map(|count| (count.language.as_str(), count.files))
        .collect();
    assert_eq!(
        languages,
        [
            ("bash", 1),
            ("c", 1),
            ("cpp", 1),
            ("csharp", 1),
            ("css", 1),
            ("dart", 1),
            ("elixir", 1),
            ("go", 1),
            ("html", 1),
            ("java", 1),
            ("javascript", 1),
            ("json", 1),
            ("jsx", 1),
            ("kotlin", 1),
            ("lua", 1),
            ("markdown", 1),
            ("php", 1),
            ("python", 1),
            ("ruby", 1),
            ("rust", 1),
            ("scala", 1),
            ("sql", 1),
            ("swift", 1),
            ("toml", 1),
            ("tsx", 1),
            ("typescript", 1),
            ("yaml", 1),
        ]
    );
}

/// A definition marked as a test by the rule of its language is stored as
/// one.
#[tokio::test]
async fn a_test_definition_is_marked_in_every_language() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    let mut tests = store
        .search(&SearchQuery {
            kind: Some("test".into()),
            ..query("halves", &[("repo", "main")])
        })
        .await
        .unwrap();
    assert_eq!(tests.len(), 1, "{tests:#?}");
    assert_eq!(tests.remove(0).path, "util.js");

    // The tests the languages mark on a definition of another kind.
    let mut marked: Vec<(String, String)> = Vec::new();
    for file in [
        "lib.rs",
        "app.ts",
        "util.js",
        "Program.cs",
        "tool.py",
        "calc.go",
        "Calculator.java",
        "calc.c",
        "calc.cpp",
        "calc.rb",
        "calc.php",
        "calc.kt",
        "calc.swift",
        "calc.dart",
        "calc.scala",
        "calc.bats",
        "calc.lua",
        "calc.ex",
    ] {
        for entry in store.outline("repo", "main", file).await.unwrap().unwrap() {
            let is_test = store
                .search(&SearchQuery {
                    kind: Some(entry.kind.clone()),
                    path: Some(file.into()),
                    ..query(&entry.name, &[("repo", "main")])
                })
                .await
                .unwrap();
            assert!(!is_test.is_empty(), "{file}::{}", entry.name);
            if entry.kind == "test"
                || [
                    "adds",
                    "GreetsByName",
                    "test_clamp",
                    "TestAdd",
                    "addsNumbers",
                    "testAdd",
                    "testAdds",
                ]
                .contains(&entry.name.as_str())
            {
                marked.push((file.to_string(), entry.name.clone()));
            }
        }
    }
    marked.sort();
    assert_eq!(
        marked,
        [
            ("Calculator.java".to_string(), "addsNumbers".to_string()),
            ("Program.cs".to_string(), "GreetsByName".to_string()),
            ("app.ts".to_string(), "multiplies".to_string()),
            ("calc.bats".to_string(), "adds".to_string()),
            ("calc.dart".to_string(), "adds".to_string()),
            ("calc.ex".to_string(), "adds".to_string()),
            ("calc.go".to_string(), "TestAdd".to_string()),
            ("calc.kt".to_string(), "addsNumbers".to_string()),
            ("calc.php".to_string(), "testAdd".to_string()),
            ("calc.rb".to_string(), "add".to_string()),
            ("calc.rb".to_string(), "adds".to_string()),
            ("calc.scala".to_string(), "adds".to_string()),
            ("calc.swift".to_string(), "testAdds".to_string()),
            ("lib.rs".to_string(), "adds".to_string()),
            ("tool.py".to_string(), "test_clamp".to_string()),
            ("util.js".to_string(), "halves".to_string()),
        ]
    );
}

/// Each outline-only format's structure stands in for a definition: a
/// YAML, TOML or JSON key at the top level or nested one level, an HTML
/// element that carries an `id`, a CSS selector, and the object name of a
/// SQL `CREATE` or `ALTER` statement.
#[tokio::test]
async fn every_format_is_outlined() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    let yaml = store
        .outline("repo", "main", "config.yaml")
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = yaml.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["name", "server", "host", "tls", "same_host"]);
    let server = yaml.iter().find(|entry| entry.name == "server").unwrap();
    assert_eq!(
        (server.kind.as_str(), server.start_line, server.end_line),
        ("key", 3, 6)
    );

    let toml = store
        .outline("repo", "main", "config.toml")
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = toml.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["name", "server", "host", "server.tls", "enabled"]);
    assert_eq!(
        toml.iter()
            .find(|entry| entry.name == "server")
            .unwrap()
            .kind,
        "table"
    );

    let json = store
        .outline("repo", "main", "config.json")
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = json.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["name", "server", "host", "tls"]);

    let html = store
        .outline("repo", "main", "page.html")
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = html.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, ["main", "inner", "query"]);
    assert!(html.iter().all(|entry| entry.kind == "element"));

    let css = store
        .outline("repo", "main", "style.css")
        .await
        .unwrap()
        .unwrap();
    let names: Vec<&str> = css.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, [".card .title", "#hero"]);
    assert!(css.iter().all(|entry| entry.kind == "selector"));

    let sql = store
        .outline("repo", "main", "schema.sql")
        .await
        .unwrap()
        .unwrap();
    let rows: Vec<(&str, &str)> = sql
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.name.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("table", "users"),
            ("table", "users"),
            ("view", "active_users"),
            ("view", "active_users"),
            ("index", "users_id"),
            ("index", "users_id"),
            ("sequence", "user_ids"),
            ("type", "mood"),
            ("schema", "app"),
            ("materialized_view", "recent_users"),
            ("function", "next_id"),
            ("trigger", "users_trigger"),
        ]
    );
}

/// A second commit that changes one file parses only that file: every other
/// blob is the one already in the store.
#[tokio::test]
async fn a_second_commit_that_changes_one_file_parses_only_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    let first = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(first.parsed, 27);

    std::fs::write(
        repo.join("tool.py"),
        "def clamp(value):\n    return value\n\ndef release(value):\n    return clamp(value)\n",
    )
    .unwrap();
    sh(
        &repo,
        "git add . && git -c user.email=t@t -c user.name=t commit -qm change",
    );
    let second = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(second.parsed, 1, "{second:?}");
    assert_eq!(second.files, 27);
    assert_eq!(second.commit, sh(&repo, "git rev-parse main"));

    let hits = store
        .search(&query("release", &[("repo", "main")]))
        .await
        .unwrap();
    assert_eq!(hits.len(), 1, "{hits:#?}");
    assert_eq!(hits[0].line, 4);
    let gone = store
        .search(&query("Gauge", &[("repo", "main")]))
        .await
        .unwrap();
    assert!(
        gone.is_empty(),
        "the old symbols of the file are gone: {gone:#?}"
    );

    // Nothing moved: nothing is parsed, and nothing is lost.
    let third = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(third.parsed, 0);
    assert_eq!(third.files, 27);
}

/// A branch cut from an indexed base shares every blob with it, so indexing
/// it parses only what the branch added; and each ref answers for its own
/// tree.
#[tokio::test]
async fn a_branch_is_indexed_on_its_own_and_shares_the_base_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    sh(
        &repo,
        "git checkout -q -b task && printf 'pub fn only_on_the_branch() {}\\n' > extra.rs && \
         git add . && git -c user.email=t@t -c user.name=t commit -qm branch && git checkout -q main",
    );
    let branch = store.index("repo", &repo, "task").await.unwrap();
    assert_eq!(branch.parsed, 1, "{branch:?}");
    assert_eq!(branch.files, 28);

    let on_branch = store
        .search(&query("only_on_the_branch", &[("repo", "task")]))
        .await
        .unwrap();
    assert_eq!(on_branch.len(), 1, "{on_branch:#?}");
    assert_eq!(on_branch[0].path, "extra.rs");
    let on_base = store
        .search(&query("only_on_the_branch", &[("repo", "main")]))
        .await
        .unwrap();
    assert!(on_base.is_empty(), "{on_base:#?}");

    let status = store.status("repo").await.unwrap();
    let refs: Vec<&str> = status.refs.iter().map(|r| r.git_ref.as_str()).collect();
    assert_eq!(refs, ["main", "task"]);
    assert_eq!(status.files, 28, "distinct paths across both refs");

    store.drop_repository("repo").await.unwrap();
    let status = store.status("repo").await.unwrap();
    assert!(status.refs.is_empty());
    assert_eq!(status.files, 0);
    assert_eq!(status.symbols, 0);
    // The blobs went with the files that held them: a read after the drop
    // parses every file again.
    let again = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(again.parsed, 27, "{again:?}");
}

/// Dropping one ref takes its files with it, and the blobs only that ref
/// held; what the other refs hold stays.
#[tokio::test]
async fn a_dropped_ref_takes_its_files_and_its_orphan_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();
    sh(
        &repo,
        "git checkout -q -b task && printf 'pub fn only_on_the_branch() {}\\n' > extra.rs && \
         git add . && git -c user.email=t@t -c user.name=t commit -qm branch && git checkout -q main",
    );
    store.index("repo", &repo, "task").await.unwrap();

    store.drop_ref("repo", "task").await.unwrap();
    let status = store.status("repo").await.unwrap();
    let refs: Vec<&str> = status.refs.iter().map(|r| r.git_ref.as_str()).collect();
    assert_eq!(refs, ["main"]);
    assert_eq!(status.files, 27);
    let gone = store
        .search(&query("only_on_the_branch", &[("repo", "task")]))
        .await
        .unwrap();
    assert!(gone.is_empty(), "{gone:#?}");

    // The branch's own blob was pruned and is parsed again; the 27 the
    // base branch still holds are not.
    let again = store.index("repo", &repo, "task").await.unwrap();
    assert_eq!(again.parsed, 1, "{again:?}");
    let base = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(base.parsed, 0, "{base:?}");
}

/// A changed file the run skips — grown past the size cap — loses its row:
/// its old symbols would otherwise answer for content the ref no longer has.
#[tokio::test]
async fn a_changed_file_the_run_skips_loses_its_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();
    assert!(
        store
            .outline("repo", "main", "tool.py")
            .await
            .unwrap()
            .is_some()
    );

    let mut grown = std::fs::read_to_string(repo.join("tool.py")).unwrap();
    grown.push_str(&"# padding\n".repeat(120_000));
    assert!(grown.len() as u64 > ariadne_knowledge::index::MAX_FILE_SIZE);
    std::fs::write(repo.join("tool.py"), grown).unwrap();
    sh(
        &repo,
        "git add . && git -c user.email=t@t -c user.name=t commit -qm grown",
    );

    let second = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(second.parsed, 0, "{second:?}");
    assert_eq!(second.files, 26);
    assert!(
        store
            .outline("repo", "main", "tool.py")
            .await
            .unwrap()
            .is_none()
    );
    let hits = store
        .search(&query("clamp", &[("repo", "main")]))
        .await
        .unwrap();
    assert!(hits.is_empty(), "{hits:#?}");
}

/// A failed run is recorded as such, and a repository's state is readable
/// before anything was indexed.
#[tokio::test]
async fn a_ref_that_does_not_resolve_fails_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let repo = fixture_repo(dir.path());
    let store = store(dir.path()).await;
    let error = store
        .index("repo", &repo, "no-such-branch")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no-such-branch"), "{error}");
    store
        .set_state("repo", State::Failed, Some(&error))
        .await
        .unwrap();
    let status = store.status("repo").await.unwrap();
    assert_eq!(status.state, State::Failed);
    assert_eq!(status.error.as_deref(), Some(error.as_str()));
}

/// Indexing this repository at HEAD completes in under 30 seconds, and
/// `add_worktree` is found in `gitwt.rs` on the line the function starts.
#[tokio::test]
async fn indexing_this_repository_at_head_finds_add_worktree_in_gitwt() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gitwt = std::fs::read_to_string(repo.join("crates/ariadne-daemon/src/gitwt.rs")).unwrap();
    let expected_line = gitwt
        .lines()
        .position(|line| line.contains("pub async fn add_worktree("))
        .expect("gitwt.rs defines add_worktree") as i64
        + 1;

    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let started = Instant::now();
    let indexed = store.index("ariadne", &repo, "HEAD").await.unwrap();
    let took = started.elapsed();
    eprintln!(
        "indexed {} files, {} symbols in {took:?}",
        indexed.files, indexed.symbols
    );
    assert!(took < Duration::from_secs(30), "indexing took {took:?}");
    assert!(indexed.symbols > 1000, "{indexed:?}");

    let hits = store
        .search(&query("add_worktree", &[("ariadne", "HEAD")]))
        .await
        .unwrap();
    assert_eq!(
        hits[0].path, "crates/ariadne-daemon/src/gitwt.rs",
        "{hits:#?}"
    );
    assert_eq!(hits[0].line, expected_line);
    assert_eq!(hits[0].kind, "method");
    assert!(
        hits[0].signature.starts_with("pub async fn add_worktree("),
        "{}",
        hits[0].signature
    );
}

// -- the symbol graph --------------------------------------------------------

/// A repository on `main` holding `files`, in one commit.
fn graph_repo(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    let repo = dir.join("graph");
    std::fs::create_dir_all(&repo).unwrap();
    sh(&repo, "git init -q -b main");
    write_files(&repo, files);
    sh(
        &repo,
        "git add . && git -c user.email=t@t -c user.name=t commit -qm graph",
    );
    repo
}

fn write_files(repo: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = repo.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

/// Commit `files` on the branch that is checked out.
fn commit_files(repo: &Path, message: &str, files: &[(&str, &str)]) {
    write_files(repo, files);
    sh(
        repo,
        &format!("git add . && git -c user.email=t@t -c user.name=t commit -qm {message}"),
    );
}

/// The one definition of `name` at `main`, by which everything else is asked.
async fn only(store: &KnowledgeStore, name: &str) -> ariadne_knowledge::Definition {
    let mut found = store
        .definitions(name, &[("repo".into(), "main".into())])
        .await
        .unwrap();
    assert_eq!(found.len(), 1, "one definition of {name}: {found:#?}");
    found.remove(0)
}

/// The definition of `name` at `path`, where a decoy of the same name sits
/// somewhere else.
async fn definition_at(
    store: &KnowledgeStore,
    name: &str,
    path: &str,
) -> ariadne_knowledge::Definition {
    let found = store
        .definitions(name, &[("repo".into(), "main".into())])
        .await
        .unwrap();
    found
        .iter()
        .find(|definition| definition.path == path)
        .cloned()
        .unwrap_or_else(|| panic!("no definition of {name} at {path}: {found:#?}"))
}

/// `path:line name confidence` per end, so an assertion reads as the answer
/// does.
fn ends(list: &[ariadne_knowledge::Related]) -> Vec<String> {
    list.iter()
        .map(|end| format!("{}:{} {} {}", end.path, end.line, end.name, end.confidence))
        .collect()
}

/// A Rust file that calls a definition it brought in with `use` names that
/// definition exactly, and the caller is listed as one.
///
/// A second `b` sits in a directory the file does not import, so the import
/// is what decides: without it the name would match two definitions and the
/// edge would be a guess to each.
#[tokio::test]
async fn a_call_through_an_import_resolves_to_the_definition_it_named() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/inner/m.rs", "/// Adds.\npub fn b() {}\n"),
            ("src/decoy/m.rs", "pub fn b() {}\n"),
            (
                "src/a.rs",
                "use crate::inner::m::b;\n\npub fn a() {\n    b();\n}\n",
            ),
        ],
    );
    let store = store(dir.path()).await;
    let indexed = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(indexed.resolved, 3, "every blob is resolved: {indexed:?}");

    let b = definition_at(&store, "b", "src/inner/m.rs").await;
    assert_eq!(b.doc.as_deref(), Some("Adds."));
    let context = store.context(b.id, "repo", "main", 20).await.unwrap();
    assert_eq!(ends(&context.callers), ["src/a.rs:3 a exact"]);
    assert!(context.callees.is_empty(), "{:#?}", context.callees);

    // The `b` the file did not import is called by nobody.
    let decoy = definition_at(&store, "b", "src/decoy/m.rs").await;
    let context = store.context(decoy.id, "repo", "main", 20).await.unwrap();
    assert!(context.callers.is_empty(), "{:#?}", context.callers);

    // And the other way round: what `a` calls is the `b` it imported.
    let a = only(&store, "a").await;
    let context = store.context(a.id, "repo", "main", 20).await.unwrap();
    assert_eq!(ends(&context.callees), ["src/inner/m.rs:2 b exact"]);
}

/// A name two files define is a guess: the caller points at both, each
/// marked `heuristic`, and the count of what matched is kept.
#[tokio::test]
async fn a_name_defined_twice_resolves_to_both_as_a_guess() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/one/m.rs", "pub fn b() {}\n"),
            ("src/two/m.rs", "pub fn b() {}\n"),
            // No import: nothing says which `b` is meant.
            ("src/a.rs", "pub fn a() {\n    b();\n}\n"),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    let found = store
        .definitions("b", &[("repo".into(), "main".into())])
        .await
        .unwrap();
    assert_eq!(found.len(), 2, "{found:#?}");
    for definition in &found {
        let context = store
            .context(definition.id, "repo", "main", 20)
            .await
            .unwrap();
        assert_eq!(
            ends(&context.callers),
            ["src/a.rs:1 a heuristic"],
            "{}",
            definition.path
        );
    }
}

/// One import shape per language of the index: the name it brought in is the
/// definition the call resolves to, exactly.
///
/// Every name is defined twice, once where the import points and once in a
/// directory nothing imports. The import is what decides: without it the name
/// would match both and the edge would be a guess. Each decoy says something
/// of its own, because two files of the same text are one blob and so one
/// definition.
#[tokio::test]
async fn an_import_of_every_language_resolves_the_name_it_brought_in() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            // Rust `use`.
            ("src/inner/rust.rs", "pub fn rust_target() {}\n"),
            (
                "decoys/rust.rs",
                "pub fn rust_target() {\n    let _ = 1;\n}\n",
            ),
            (
                "caller.rs",
                "use crate::src::inner::rust::rust_target;\n\npub fn rust_caller() {\n    rust_target();\n}\n",
            ),
            // Python `from pkg.m import x`.
            ("pkg/m.py", "def python_target():\n    pass\n"),
            ("decoys/m.py", "def python_target():\n    return 1\n"),
            (
                "caller.py",
                "from pkg.m import python_target\n\ndef python_caller():\n    python_target()\n",
            ),
            // TypeScript `import { x } from './m'`.
            ("lib/m.ts", "export function tsTarget(): void {}\n"),
            (
                "decoys/m.ts",
                "export function tsTarget(): void {\n    return;\n}\n",
            ),
            (
                "caller.ts",
                "import { tsTarget } from './lib/m';\n\nexport function tsCaller(): void {\n    tsTarget();\n}\n",
            ),
            // JavaScript `require`.
            (
                "lib/util.js",
                "function jsTarget() {}\nmodule.exports = { jsTarget };\n",
            ),
            (
                "decoys/util.js",
                "function jsTarget() {\n    return 1;\n}\nmodule.exports = { jsTarget };\n",
            ),
            (
                "caller.js",
                "const { jsTarget } = require('./lib/util');\n\nfunction jsCaller() {\n    jsTarget();\n}\n",
            ),
            // C# `using`, which names a namespace rather than a path: the
            // namespace is what tells the two `Run` methods apart.
            (
                "api/Helper.cs",
                "namespace Lib {\n    public class Helper {\n        public static void Run() {}\n    }\n}\n",
            ),
            (
                "decoys/Other.cs",
                "namespace Elsewhere {\n    public class Other {\n        public static void Run() {}\n    }\n}\n",
            ),
            (
                "caller.cs",
                "using Lib;\n\npublic class Caller {\n    public void Call() {\n        Helper.Run();\n    }\n}\n",
            ),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    for (target, path, decoy, caller) in [
        (
            "rust_target",
            "src/inner/rust.rs",
            "decoys/rust.rs",
            "caller.rs:3 rust_caller exact",
        ),
        (
            "python_target",
            "pkg/m.py",
            "decoys/m.py",
            "caller.py:3 python_caller exact",
        ),
        (
            "tsTarget",
            "lib/m.ts",
            "decoys/m.ts",
            "caller.ts:3 tsCaller exact",
        ),
        (
            "jsTarget",
            "lib/util.js",
            "decoys/util.js",
            "caller.js:3 jsCaller exact",
        ),
        (
            "Run",
            "api/Helper.cs",
            "decoys/Other.cs",
            "caller.cs:4 Call exact",
        ),
    ] {
        let imported = definition_at(&store, target, path).await;
        let context = store
            .context(imported.id, "repo", "main", 20)
            .await
            .unwrap();
        assert_eq!(ends(&context.callers), [caller], "{target}");

        let spare = definition_at(&store, target, decoy).await;
        let context = store.context(spare.id, "repo", "main", 20).await.unwrap();
        assert!(
            context.callers.is_empty(),
            "{target} at {decoy}: {:#?}",
            context.callers
        );
    }
}

/// The callers of a definition are walked by depth, and the tests of it are
/// the test definitions at most two edges away.
#[tokio::test]
async fn the_callers_are_walked_by_depth_and_a_test_two_edges_away_is_a_test() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/inner/m.rs", "pub fn b() {}\n"),
            (
                "src/a.rs",
                "use crate::inner::m::b;\n\npub fn a() {\n    b();\n}\n",
            ),
            (
                "src/top.rs",
                "use crate::a::a;\n\npub fn top() {\n    a();\n}\n",
            ),
            (
                "src/proof.rs",
                "use crate::a::a;\n\n#[test]\nfn a_proof() {\n    a();\n}\n",
            ),
            ("src/aside.rs", "pub fn untouched() {}\n"),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    let b = only(&store, "b").await;
    let (callers, stopped) = store.impact(b.id, "repo", "main", 2).await.unwrap();
    let walked: Vec<String> = callers
        .iter()
        .map(|caller| {
            format!(
                "{} {}:{} {}",
                caller.depth, caller.path, caller.line, caller.name
            )
        })
        .collect();
    assert_eq!(
        walked,
        [
            "1 src/a.rs:3 a",
            "2 src/proof.rs:4 a_proof",
            "2 src/top.rs:3 top",
        ],
        "{callers:#?}"
    );
    assert!(stopped.is_empty(), "{stopped:?}");

    // The test reaches `b` over two edges: it calls `a`, and `a` calls `b`.
    let context = store.context(b.id, "repo", "main", 20).await.unwrap();
    assert_eq!(ends(&context.tests), ["src/proof.rs:4 a_proof exact"]);

    // Nothing calls `untouched`, so nothing is reached from it.
    let untouched = only(&store, "untouched").await;
    let (none, _) = store.impact(untouched.id, "repo", "main", 4).await.unwrap();
    assert!(none.is_empty(), "{none:#?}");
}

/// Edges belong to the ref they were resolved at. Two refs share the blob of
/// an unchanged caller, and each answers for the definition its own tree
/// holds: a later run on the base branch does not take the branch's edges.
#[tokio::test]
async fn edges_belong_to_the_ref_they_were_resolved_at() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/inner/m.rs", "pub fn b() {}\n"),
            ("src/inner/other.rs", "pub fn c() {}\n"),
            (
                "src/a.rs",
                "use crate::inner::m::b;\nuse crate::inner::other::c;\n\n                 pub fn a() {\n    b();\n    c();\n}\n",
            ),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    // A branch changes `b`, so the branch has a `b` of its own. The caller is
    // untouched, so both refs hold the same blob of it.
    sh(&repo, "git checkout -q -b task");
    commit_files(
        &repo,
        "change-b",
        &[("src/inner/m.rs", "pub fn b() {\n    let _ = 1;\n}\n")],
    );
    store.index("repo", &repo, "task").await.unwrap();

    // The base branch moves on, over a name the same caller also names. That
    // resolves the caller again, at the base branch.
    sh(&repo, "git checkout -q main");
    commit_files(
        &repo,
        "change-c",
        &[("src/inner/other.rs", "pub fn c() {\n    let _ = 2;\n}\n")],
    );
    store.index("repo", &repo, "main").await.unwrap();

    // Each ref still answers for its own `b`.
    for git_ref in ["main", "task"] {
        let found = store
            .definitions("b", &[("repo".into(), git_ref.into())])
            .await
            .unwrap();
        assert_eq!(found.len(), 1, "one `b` at {git_ref}: {found:#?}");
        let context = store
            .context(found[0].id, "repo", git_ref, 20)
            .await
            .unwrap();
        assert_eq!(
            ends(&context.callers),
            ["src/a.rs:4 a exact"],
            "the callers of `b` at {git_ref}"
        );
        let (callers, _) = store.impact(found[0].id, "repo", git_ref, 2).await.unwrap();
        assert_eq!(
            callers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["a"],
            "what a change to `b` reaches at {git_ref}"
        );
    }

    // Dropping the branch takes the branch's edges with it, and leaves the
    // base branch's whole.
    store.drop_ref("repo", "task").await.unwrap();
    let base = only(&store, "b").await;
    let context = store.context(base.id, "repo", "main", 20).await.unwrap();
    assert_eq!(ends(&context.callers), ["src/a.rs:4 a exact"]);
}

/// A branch that changes one definition: the diff names it, and its callers
/// are what the change reaches. An untouched definition is not in the answer.
#[tokio::test]
async fn a_diff_names_the_definitions_it_changed_and_their_callers() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/inner/m.rs", "pub fn b() {}\n"),
            (
                "src/a.rs",
                "use crate::inner::m::b;\n\npub fn a() {\n    b();\n}\n",
            ),
            ("src/aside.rs", "pub fn untouched() {}\n"),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    sh(&repo, "git checkout -q -b task");
    commit_files(
        &repo,
        "change",
        &[("src/inner/m.rs", "pub fn b() {\n    let _ = 1;\n}\n")],
    );
    store.index("repo", &repo, "task").await.unwrap();

    let changed = ariadne_knowledge::index::changed_lines(&repo, "main..task")
        .await
        .unwrap();
    let paths: Vec<&str> = changed.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(paths, ["src/inner/m.rs"], "{changed:?}");
    let touched = store
        .symbols_in_lines("repo", "task", &changed)
        .await
        .unwrap();
    let names: Vec<&str> = touched
        .iter()
        .map(|(_, symbol)| symbol.name.as_str())
        .collect();
    assert_eq!(names, ["b"], "only what the diff changed: {touched:#?}");

    let (callers, _) = store.impact(touched[0].0, "repo", "task", 2).await.unwrap();
    assert_eq!(
        callers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["a"]
    );

    // A range that is no range is refused rather than handed to git.
    let refused = ariadne_knowledge::index::changed_lines(&repo, "--output=/tmp/x")
        .await
        .unwrap_err()
        .to_string();
    assert!(refused.contains("<base>..<head>"), "{refused}");
}

/// A definition with more callers than the cap is not walked past, and the
/// answer names it.
#[tokio::test]
async fn a_definition_with_more_than_two_hundred_callers_is_not_walked_past() {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<(String, String)> = vec![(
        "src/hot.rs".to_string(),
        "pub fn inner() {}\n\npub fn hot() {\n    inner();\n}\n".to_string(),
    )];
    for n in 0..201 {
        files.push((
            format!("src/callers/c{n}.rs"),
            format!("use crate::hot::hot;\n\npub fn caller{n}() {{\n    hot();\n}}\n"),
        ));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let repo = graph_repo(dir.path(), &borrowed);
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    // One step out from `inner`, the walk reaches `hot` and stops there.
    let inner = only(&store, "inner").await;
    let (callers, stopped) = store.impact(inner.id, "repo", "main", 3).await.unwrap();
    assert_eq!(
        callers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["hot"],
        "{callers:#?}"
    );
    assert_eq!(stopped, ["hot"]);

    // Asked about `hot` itself, the walk stops before it starts.
    let hot = only(&store, "hot").await;
    let (callers, stopped) = store.impact(hot.id, "repo", "main", 3).await.unwrap();
    assert!(callers.is_empty(), "{callers:#?}");
    assert_eq!(stopped, ["hot"]);
}

/// A changed blob has its own edges derived again, and so has every blob that
/// names a definition the change moved. Nothing else is resolved.
#[tokio::test]
async fn a_changed_blob_resolves_itself_and_what_names_what_moved() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            ("src/inner/m.rs", "pub fn b() {}\n"),
            (
                "src/a.rs",
                "use crate::inner::m::b;\n\npub fn a() {\n    b();\n}\n",
            ),
            ("src/aside.rs", "pub fn untouched() {}\n"),
        ],
    );
    let store = store(dir.path()).await;
    let first = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(first.resolved, 3, "a first read resolves every blob");

    // `b` moves: the file it is in, and the file that calls it.
    commit_files(
        &repo,
        "move-b",
        &[("src/inner/m.rs", "pub fn b() {\n    let _ = 1;\n}\n")],
    );
    let second = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(second.parsed, 1, "{second:?}");
    assert_eq!(second.resolved, 2, "the changed blob and its caller");

    // A file nothing names moves alone.
    commit_files(
        &repo,
        "move-aside",
        &[("src/aside.rs", "pub fn untouched() {}\npub fn alone() {}\n")],
    );
    let third = store.index("repo", &repo, "main").await.unwrap();
    assert_eq!(third.parsed, 1, "{third:?}");
    assert_eq!(third.resolved, 1, "nothing names what moved");

    // The edge into `b` is still there, and still exact.
    let b = only(&store, "b").await;
    let context = store.context(b.id, "repo", "main", 20).await.unwrap();
    assert_eq!(ends(&context.callers), ["src/a.rs:3 a exact"]);
}

/// A Rust `impl Trait for Type` is an edge from the type to the trait, and
/// the trait lists it as an implementation.
#[tokio::test]
async fn an_implementation_is_listed_under_the_trait_it_is_of() {
    let dir = tempfile::tempdir().unwrap();
    let repo = graph_repo(
        dir.path(),
        &[
            (
                "src/inner/t.rs",
                "pub trait Shape {\n    fn area(&self);\n}\n",
            ),
            (
                "src/widget.rs",
                "use crate::inner::t::Shape;\n\npub struct Widget;\n\nimpl Shape for Widget {\n    fn area(&self) {}\n}\n",
            ),
        ],
    );
    let store = store(dir.path()).await;
    store.index("repo", &repo, "main").await.unwrap();

    let shape = only(&store, "Shape").await;
    let context = store.context(shape.id, "repo", "main", 20).await.unwrap();
    assert_eq!(
        ends(&context.implementations),
        ["src/widget.rs:3 Widget exact"]
    );
}
