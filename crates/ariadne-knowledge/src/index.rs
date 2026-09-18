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

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::languages::Language;
use crate::parser;
use crate::store::{FileChanges, FileRow, KnowledgeStore, ParsedBlob};

/// Files over this size are skipped: generated code and data dumps, not the
/// definitions an agent is looking for.
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// How many blobs are read and parsed at a time, which bounds what one
/// `cat-file` answer holds in memory.
const BATCH: usize = 200;

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

        let mut wanted: Vec<(String, Language)> = Vec::new();
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
                wanted.push((entry.blob.clone(), language));
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

        let blobs: Vec<String> = wanted.iter().map(|(blob, _)| blob.clone()).collect();
        let known = self.known_blobs(&blobs).await?;
        let to_parse: Vec<(String, Language)> = wanted
            .into_iter()
            .filter(|(blob, _)| !known.contains(blob))
            .collect();
        let mut parsed = 0;
        for chunk in to_parse.chunks(BATCH) {
            let ids: Vec<&str> = chunk.iter().map(|(blob, _)| blob.as_str()).collect();
            let contents = cat_file(repo, &ids).await?;
            let chunk = chunk.to_vec();
            let batch = tokio::task::spawn_blocking(move || {
                chunk
                    .into_iter()
                    .map(|(blob, language)| {
                        let symbols = match contents.get(&blob) {
                            Some(bytes) if !is_binary(bytes) => {
                                parser::parse(language, &String::from_utf8_lossy(bytes))
                            }
                            _ => Vec::new(),
                        };
                        ParsedBlob {
                            blob,
                            language: language.name(),
                            symbols,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .context("parsing")?;
            parsed += batch.len();
            self.commit_blobs(&batch).await?;
        }
        self.commit_files(repository_id, git_ref, &commit, &changes)
            .await?;
        let (files, symbols) = self.counts(repository_id, git_ref).await?;
        Ok(Indexed {
            git_ref: git_ref.to_string(),
            commit,
            files,
            symbols,
            parsed,
        })
    }
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

    #[test]
    fn a_nul_byte_marks_a_binary() {
        assert!(is_binary(b"\x89PNG\0\0"));
        assert!(!is_binary("fn main() {}".as_bytes()));
    }
}
