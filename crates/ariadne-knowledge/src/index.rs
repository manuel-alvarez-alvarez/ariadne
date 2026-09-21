//! Walking one git ref into the store.
//!
//! The first index of a ref reads every tracked file at its head; every
//! later one asks `git diff --name-only <last indexed commit> <head>` and
//! reads only what changed. A file is read only where its blob is not in the
//! store yet, so a commit that touches one file parses one file, and a
//! branch cut from an indexed base parses nothing at all.
//!
//! Only what git tracks is read, over `git ls-tree` and `git cat-file
//! --batch`, never the working tree: the ref may be checked out nowhere, and
//! an author's uncommitted edits are not the branch.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::languages::Language;
use crate::store::{FileChanges, FileRow, KnowledgeStore, ParsedBlob};
use crate::{interfaces, parser, resolve};

/// Files over this size are skipped: generated code and data dumps, not the
/// definitions an agent is looking for.
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// How many blobs are read and parsed at a time, which bounds what one
/// `cat-file` answer holds in memory.
const BATCH: usize = 200;

/// How many blobs have their edges derived at a time, each batch in a write
/// transaction of its own. What one batch holds in memory is the mentions and
/// imports of at most this many files of at most [`MAX_FILE_SIZE`] each, and
/// the edges they make. Of the definitions of each name they hold, it keeps
/// what can answer the name ([`resolve::Keeper`]): 4 anywhere, and 21 in the
/// file, the directory and the imports of each blob that names it. That bound
/// holds whatever the size of the ref and however many files define one name.
/// A write waits on one batch at most, not on the whole ref.
const RESOLVE_BATCH: usize = 500;

/// What one index run did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indexed {
    pub git_ref: String,
    pub commit: String,
    /// Files indexed at the ref, after the run.
    pub files: i64,
    /// Symbols of those files.
    pub symbols: i64,
    /// Blobs parsed by this run: the files that changed and were not known.
    pub parsed: usize,
    /// Blobs whose edges this run derived again: the ones it parsed, and the
    /// ones that name a definition the run moved.
    pub resolved: usize,
}

/// One entry of `git ls-tree`.
struct TreeEntry {
    mode: String,
    blob: String,
    size: u64,
    path: String,
}

impl KnowledgeStore {
    /// Index `git_ref` of the repository at `repo` under `repository_id`, from
    /// wherever it was last indexed.
    pub async fn index(&self, repository_id: &str, repo: &Path, git_ref: &str) -> Result<Indexed> {
        let commit = git(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{git_ref}^{{commit}}"),
            ],
        )
        .await
        .with_context(|| format!("resolving {git_ref} in {}", repo.display()))?;
        let previous = self.ref_commit(repository_id, git_ref).await?;
        if previous.as_deref() == Some(commit.as_str()) {
            let (files, symbols) = self.counts(repository_id, git_ref).await?;
            return Ok(Indexed {
                git_ref: git_ref.to_string(),
                commit,
                files,
                symbols,
                parsed: 0,
                resolved: 0,
            });
        }

        // What this run decides about: every tracked path on a first read,
        // else the paths that changed since the last commit.
        let (entries, changed) = match &previous {
            Some(previous) => match changed_paths(repo, previous, &commit).await {
                Ok(paths) => (list_tree(repo, &commit, Some(&paths)).await?, Some(paths)),
                // The last indexed commit is gone — rewritten history, a
                // pruned object store — so the ref is read whole again.
                Err(_) => (list_tree(repo, &commit, None).await?, None),
            },
            None => (list_tree(repo, &commit, None).await?, None),
        };
        let mut changes = FileChanges {
            replace_all: changed.is_none(),
            ..Default::default()
        };

        // Each blob with its language and the first path it was seen at,
        // which is what its manifest, if it is one, is read by.
        let mut wanted: Vec<(String, Language, String)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for entry in entries {
            // Symlinks and submodules are not files of this repository.
            if entry.mode.starts_with("120") || entry.mode.starts_with("160") {
                continue;
            }
            if entry.size > MAX_FILE_SIZE {
                continue;
            }
            let Some(language) = Language::of_path(&entry.path) else {
                continue;
            };
            if seen.insert(entry.blob.clone()) {
                wanted.push((entry.blob.clone(), language, entry.path.clone()));
            }
            changes.upserted.push(FileRow {
                path: entry.path,
                blob: entry.blob,
                language: language.name(),
            });
        }
        // Every changed path this run did not keep is gone from the ref:
        // deleted, grown past the size cap, a symlink now. Its old row would
        // answer for content the ref no longer has.
        if let Some(changed) = changed {
            let kept: HashSet<&str> = changes
                .upserted
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            changes.removed = changed
                .into_iter()
                .filter(|path| !kept.contains(path.as_str()))
                .collect();
        }

        let blobs: Vec<String> = wanted.iter().map(|(blob, _, _)| blob.clone()).collect();
        let known = self.known_blobs(&blobs).await?;
        let to_parse: Vec<(String, Language, String)> = wanted
            .into_iter()
            .filter(|(blob, _, _)| !known.contains(blob))
            .collect();
        // The names the ref held at the paths this run touches, before it
        // does: a definition that goes takes the edges into it with it.
        let touched: Vec<String> = changes
            .upserted
            .iter()
            .map(|file| file.path.clone())
            .chain(changes.removed.iter().cloned())
            .collect();
        let mut moved_names = self.names_at(repository_id, git_ref, &touched).await?;

        let mut parsed = 0;
        for chunk in to_parse.chunks(BATCH) {
            let ids: Vec<&str> = chunk.iter().map(|(blob, _, _)| blob.as_str()).collect();
            let contents = cat_file(repo, &ids).await?;
            // A core each: reading every reference of a file costs several
            // times what reading its definitions alone did, and the files of
            // one batch are read one from another.
            let contents = Arc::new(contents);
            let workers = std::thread::available_parallelism().map_or(4, |cores| cores.get());
            let mut reading = Vec::new();
            for piece in chunk.chunks(chunk.len().div_ceil(workers).max(1)) {
                let piece = piece.to_vec();
                let contents = Arc::clone(&contents);
                reading.push(tokio::task::spawn_blocking(move || {
                    piece
                        .into_iter()
                        .map(|(blob, language, path)| {
                            let (read, found) = match contents.get(&blob) {
                                Some(bytes) if !is_binary(bytes) => {
                                    let text = String::from_utf8_lossy(bytes);
                                    let read = parser::read(language, &text);
                                    let found =
                                        interfaces::read(&path, language, &text, &read.symbols);
                                    (read, found)
                                }
                                _ => (parser::Parsed::default(), Vec::new()),
                            };
                            ParsedBlob {
                                blob,
                                language: language.name(),
                                symbols: read.symbols,
                                references: read.references,
                                imports: read.imports,
                                interfaces: found,
                            }
                        })
                        .collect::<Vec<_>>()
                }));
            }
            let mut batch = Vec::with_capacity(chunk.len());
            for read in reading {
                batch.extend(read.await.context("parsing")?);
            }
            parsed += batch.len();
            self.commit_blobs(&batch).await?;
        }
        self.commit_files(repository_id, git_ref, &commit, &changes)
            .await?;

        // Every blob this run parsed, and every blob of the ref that names a
        // definition the run moved: those are the edges the run invalidated,
        // and no others. The names the touched paths hold now count too: on
        // the first read of a branch nothing was parsed, and the blobs that
        // name what the branch changed still have to point at it.
        let mut to_resolve: HashSet<String> =
            to_parse.into_iter().map(|(blob, _, _)| blob).collect();
        moved_names.extend(self.names_at(repository_id, git_ref, &touched).await?);
        let moved: Vec<String> = moved_names.into_iter().collect();
        to_resolve.extend(self.blobs_naming(repository_id, git_ref, &moved).await?);
        let to_resolve: Vec<String> = to_resolve.into_iter().collect();
        self.resolve(repository_id, git_ref, &to_resolve).await?;
        // What this ref offers the other repositories and takes from them,
        // derived whole: a change here can move a package, a route or a
        // variable that another repository's edges point at.
        self.link(repository_id, git_ref).await?;

        let (files, symbols) = self.counts(repository_id, git_ref).await?;
        Ok(Indexed {
            git_ref: git_ref.to_string(),
            commit,
            files,
            symbols,
            parsed,
            resolved: to_resolve.len(),
        })
    }

    /// Derive the edges of `blobs` again, against the definitions this ref
    /// holds.
    async fn resolve(&self, repository_id: &str, git_ref: &str, blobs: &[String]) -> Result<()> {
        self.resolve_in_batches(repository_id, git_ref, blobs, RESOLVE_BATCH)
            .await
            .map(|_| ())
    }

    /// [`Self::resolve`], `batch` blobs at a time: each batch reads its names
    /// and their definitions and writes its edges in a transaction of its
    /// own. A blob's edges depend on its own names and the definitions of the
    /// ref alone, so the split changes no edge.
    async fn resolve_in_batches(
        &self,
        repository_id: &str,
        git_ref: &str,
        blobs: &[String],
        batch: usize,
    ) -> Result<Batches> {
        let mut batches = Batches::default();
        for chunk in blobs.chunks(batch.max(1)) {
            let named = self.names_of(repository_id, git_ref, chunk).await?;
            let mut keeper = resolve::Keeper::new(&named);
            let names = keeper.names();
            self.each_candidate(repository_id, git_ref, &names, |name, candidate| {
                keeper.offer(name, candidate)
            })
            .await?;
            batches.most_candidates = batches.most_candidates.max(keeper.len());
            let edges = resolve::edges_of(&named, &keeper.into_candidates());
            self.commit_edges(repository_id, git_ref, chunk, &edges)
                .await?;
            batches.transactions += 1;
        }
        Ok(batches)
    }
}

/// What one resolve pass held and wrote.
#[derive(Debug, Default)]
struct Batches {
    /// Write transactions: one per batch.
    transactions: usize,
    /// The most definitions one batch held at once.
    most_candidates: usize,
}

/// The lines a diff changed, per path on its right-hand side: what
/// `git diff --unified=0 <base>..<head>` reports.
pub async fn changed_lines(repo: &Path, range: &str) -> Result<Vec<(String, Vec<(u32, u32)>)>> {
    // A range is two commits and nothing else: a value that could read as a
    // flag is refused rather than handed to git.
    if range.starts_with('-') || !range.contains("..") {
        bail!("a diff is `<base>..<head>`, not {range:?}");
    }
    let output = git_output(repo, &["diff", "--unified=0", "--no-color", range]).await?;
    Ok(parse_hunks(&String::from_utf8_lossy(&output)))
}

/// The text of one blob, from the repository's object store.
pub async fn blob_text(repo: &Path, blob: &str) -> Result<String> {
    let contents = cat_file(repo, &[blob]).await?;
    let bytes = contents
        .get(blob)
        .with_context(|| format!("git has no object {blob} in {}", repo.display()))?;
    Ok(String::from_utf8_lossy(bytes).to_string())
}

/// `+++ b/<path>` and `@@ -a,b +c,d @@` of a unified diff, as the lines each
/// path gained. A hunk that only deletes is the line it deleted at, so the
/// definition around it is still named.
fn parse_hunks(diff: &str) -> Vec<(String, Vec<(u32, u32)>)> {
    let mut changed: Vec<(String, Vec<(u32, u32)>)> = Vec::new();
    // Which file the hunks now being read belong to. `None` for a file the
    // diff deletes, whose right-hand side is `/dev/null`: its hunks belong to
    // no path, and adding them to the file before it would report lines that
    // file never changed.
    let mut at: Option<usize> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.strip_prefix("b/").unwrap_or(path).trim();
            at = match path {
                "/dev/null" => None,
                path => {
                    changed.push((path.to_string(), Vec::new()));
                    Some(changed.len() - 1)
                }
            };
            continue;
        }
        let Some(hunk) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some(at) = at else {
            continue;
        };
        let Some(after) = hunk.split_whitespace().find(|part| part.starts_with('+')) else {
            continue;
        };
        let mut fields = after[1..].split(',');
        let Some(start): Option<u32> = fields.next().and_then(|f| f.parse().ok()) else {
            continue;
        };
        let count: u32 = fields.next().and_then(|f| f.parse().ok()).unwrap_or(1);
        changed[at]
            .1
            .push((start.max(1), start.max(1) + count.saturating_sub(1)));
    }
    changed.retain(|(_, ranges)| !ranges.is_empty());
    changed
}

/// Run `git` in `repo` and answer its trimmed stdout.
async fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = git_output(repo, args).await?;
    Ok(String::from_utf8_lossy(&output).trim().to_string())
}

/// Run `git` in `repo` and answer its raw stdout.
async fn git_output(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .await
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// The paths that differ between two commits.
async fn changed_paths(repo: &Path, from: &str, to: &str) -> Result<Vec<String>> {
    let output = git_output(repo, &["diff", "--name-only", "-z", from, to]).await?;
    Ok(String::from_utf8_lossy(&output)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

/// The tracked files at `commit`: all of them, or the ones among `paths`
/// that exist there.
async fn list_tree(repo: &Path, commit: &str, paths: Option<&[String]>) -> Result<Vec<TreeEntry>> {
    let mut entries = Vec::new();
    match paths {
        None => {
            let output = git_output(repo, &["ls-tree", "-r", "-l", "-z", commit]).await?;
            entries.extend(parse_ls_tree(&output));
        }
        Some(paths) => {
            // In chunks: a diff can name more paths than one command line
            // takes.
            for chunk in paths.chunks(200) {
                let mut args = vec!["ls-tree", "-l", "-z", commit, "--"];
                args.extend(chunk.iter().map(String::as_str));
                let output = git_output(repo, &args).await?;
                entries.extend(parse_ls_tree(&output));
            }
        }
    }
    Ok(entries)
}

/// `<mode> <type> <object> <size>\t<path>`, one per NUL-terminated record.
fn parse_ls_tree(output: &[u8]) -> Vec<TreeEntry> {
    String::from_utf8_lossy(output)
        .split('\0')
        .filter_map(|record| {
            let (meta, path) = record.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let mode = fields.next()?.to_string();
            let kind = fields.next()?;
            let blob = fields.next()?.to_string();
            let size = fields.next()?;
            if kind != "blob" {
                return None;
            }
            Some(TreeEntry {
                mode,
                blob,
                size: size.parse().unwrap_or(u64::MAX),
                path: path.to_string(),
            })
        })
        .collect()
}

/// The contents of `blobs`, over one `git cat-file --batch`.
async fn cat_file(repo: &Path, blobs: &[&str]) -> Result<HashMap<String, Vec<u8>>> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git cat-file")?;
    let mut stdin = child.stdin.take().context("cat-file stdin")?;
    let request = blobs
        .iter()
        .map(|blob| format!("{blob}\n"))
        .collect::<String>();
    // Written while the output is read: a request longer than the pipe
    // would otherwise wait on an answer nobody is reading yet.
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(request.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });
    let output = child
        .wait_with_output()
        .await
        .context("reading git cat-file")?;
    let _ = writer.await;
    if !output.status.success() {
        bail!(
            "git cat-file --batch failed in {}: {}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(parse_batch(&output.stdout))
}

/// `<sha> <type> <size>\n<content>\n` per object; `<sha> missing\n` for one
/// git does not have.
fn parse_batch(output: &[u8]) -> HashMap<String, Vec<u8>> {
    let mut contents = HashMap::new();
    let mut at = 0;
    while at < output.len() {
        let Some(newline) = output[at..].iter().position(|b| *b == b'\n') else {
            break;
        };
        let header = String::from_utf8_lossy(&output[at..at + newline]).to_string();
        at += newline + 1;
        let mut fields = header.split_whitespace();
        let (Some(sha), Some(kind)) = (fields.next(), fields.next()) else {
            break;
        };
        if kind == "missing" {
            continue;
        }
        let size: usize = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let end = (at + size).min(output.len());
        contents.insert(sha.to_string(), output[at..end].to_vec());
        // The newline git writes after every object.
        at = end + 1;
    }
    contents
}

/// Git's own test: a NUL byte in the first 8000 bytes.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repository at `dir/repo` holding `files`, in one commit on `main`.
    fn repo_with(dir: &Path, files: &[(String, String)]) -> std::path::PathBuf {
        let repo = dir.join("repo");
        for (path, text) in files {
            let at = repo.join(path);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(
                "git init -q -b main && git add -A && \
                 git -c user.email=t@t -c user.name=t -c commit.gpgsign=false commit -qm files",
            )
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        repo
    }

    /// Index `repo` at `main`, then derive the edges of every blob of the ref
    /// again at each of `batches`. Answers the edges of every blob derived
    /// at once over every definition of every name, as the pass did before
    /// it was split, and the edges and the counts of each batched run.
    async fn edges_by_batch(
        dir: &Path,
        repo: &Path,
        batches: &[usize],
    ) -> (
        Vec<crate::store::EdgeRow>,
        Vec<(Batches, Vec<crate::store::EdgeRow>)>,
    ) {
        let store = KnowledgeStore::open(dir.join("knowledge.db"))
            .await
            .unwrap();
        store.index("r", repo, "main").await.unwrap();
        let blobs = store.blobs_at("r", "main").await.unwrap();
        let named = store.names_of("r", "main", &blobs).await.unwrap();
        let mut names: Vec<String> = named
            .values()
            .flat_map(|names| {
                names
                    .mentions
                    .iter()
                    .map(|mention| mention.name.clone())
                    .chain(names.imports.iter().filter_map(|i| i.name.clone()))
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        let candidates = store.candidates("r", "main", &names).await.unwrap();
        let edges = resolve::edges_of(&named, &candidates);
        store
            .commit_edges("r", "main", &blobs, &edges)
            .await
            .unwrap();
        let whole = store.symbol_edges_at("r", "main").await.unwrap();
        let mut runs = Vec::new();
        for &batch in batches {
            let counts = store
                .resolve_in_batches("r", "main", &blobs, batch)
                .await
                .unwrap();
            runs.push((counts, store.symbol_edges_at("r", "main").await.unwrap()));
        }
        (whole, runs)
    }

    #[tokio::test]
    async fn the_fixture_edges_are_the_same_whatever_the_batch_size() {
        let dir = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let files: Vec<(String, String)> = std::fs::read_dir(fixtures)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().to_string(),
                    std::fs::read_to_string(entry.path()).unwrap(),
                )
            })
            .collect();
        let repo = repo_with(dir.path(), &files);
        let (whole, runs) = edges_by_batch(dir.path(), &repo, &[1, 7, RESOLVE_BATCH]).await;
        assert!(!whole.is_empty(), "the fixtures make edges");
        for (at, (_, edges)) in runs.iter().enumerate() {
            assert_eq!(edges, &whole, "run {at} changed the edges");
        }
    }

    /// `files` files, each in a directory of its own. Each calls the file
    /// before it, a name defined once in the ref, at step `repository`, and
    /// `shared`, a name every file defines, at step `file`.
    fn many_files(files: usize) -> Vec<(String, String)> {
        (0..files)
            .map(|n| {
                let before = (n + files - 1) % files;
                (
                    format!("src/m{n}/f.rs"),
                    format!(
                        "pub fn f{n}() {{\n    f{before}();\n    shared();\n}}\n\npub fn shared() {{}}\n"
                    ),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn a_ref_of_many_blobs_resolves_in_many_transactions_to_the_same_edges() {
        const FILES: usize = 60;
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with(dir.path(), &many_files(FILES));
        let (whole, runs) = edges_by_batch(dir.path(), &repo, &[4]).await;
        let (counts, edges) = &runs[0];
        assert_eq!(counts.transactions, FILES.div_ceil(4));
        assert!(counts.transactions > 1);
        assert_eq!(whole.len(), FILES * 2, "each file calls two definitions");
        assert_eq!(edges, &whole);
    }

    /// Worked example, a batch of 4 of the files of [`many_files`]: the 4
    /// names `f{before}` have one definition each, and `shared` keeps each
    /// blob's own definition (its file and its directory) and the first 4 read
    /// for step `repository`. That is 12 at most, of the `FILES + 4` the
    /// names have in the ref.
    #[tokio::test]
    async fn a_name_every_file_defines_holds_only_what_one_batch_can_resolve_to() {
        const FILES: usize = 200;
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with(dir.path(), &many_files(FILES));
        let (whole, runs) = edges_by_batch(dir.path(), &repo, &[4]).await;
        let (counts, edges) = &runs[0];
        assert!(
            counts.most_candidates <= 12,
            "a batch held {} definitions",
            counts.most_candidates
        );
        assert_eq!(edges, &whole);
    }

    #[test]
    fn an_ls_tree_record_is_read_for_its_mode_blob_size_and_path() {
        let output = b"100644 blob 0123abcd     42\tsrc/lib.rs\x00120000 blob 89ab      7\tlink\x00160000 commit ffff       -\tsub\x00";
        let entries = parse_ls_tree(output);
        assert_eq!(entries.len(), 2, "the submodule is no blob");
        assert_eq!(entries[0].mode, "100644");
        assert_eq!(entries[0].blob, "0123abcd");
        assert_eq!(entries[0].size, 42);
        assert_eq!(entries[0].path, "src/lib.rs");
        assert_eq!(entries[1].mode, "120000");
    }

    #[test]
    fn a_cat_file_batch_is_read_object_by_object() {
        let output = b"aaaa blob 5\nhello\nbbbb missing\ncccc blob 0\n\n";
        let contents = parse_batch(output);
        assert_eq!(contents.get("aaaa").map(Vec::as_slice), Some(&b"hello"[..]));
        assert_eq!(contents.get("cccc").map(Vec::as_slice), Some(&b""[..]));
        assert!(!contents.contains_key("bbbb"));
    }

    /// A hunk belongs to the file its `+++` named, and a file the diff
    /// deletes has no right-hand side: its hunks belong to nothing, and never
    /// to the file before it.
    #[test]
    fn the_hunks_of_a_deleted_file_belong_to_no_path() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -40,0 +41,2 @@ fn a() {
+    let x = 1;
+    let y = 2;
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
--- a/src/old.rs
+++ /dev/null
@@ -1,10 +0,0 @@
-fn gone() {}
";
        assert_eq!(
            parse_hunks(diff),
            [("src/a.rs".to_string(), vec![(41, 42)])],
            "the deleted file's hunk is not src/a.rs's"
        );
    }

    #[test]
    fn a_nul_byte_marks_a_binary() {
        assert!(is_binary(b"\x89PNG\0\0"));
        assert!(!is_binary("fn main() {}".as_bytes()));
    }
}
