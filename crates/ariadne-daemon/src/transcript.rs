//! What a session has spent, read from the transcript its agent writes.
//!
//! The ACP prompt response is late and, for Codex, wrong: codex-acp reports
//! the last model request of a turn, not the turn. Codex and Claude Code
//! both write the true running figures to disk while the turn runs, in a
//! file keyed by the ACP session id (`agent_sessions.internal_session_id`):
//!
//! - Codex: `$CODEX_HOME/sessions/**/rollout-*<id>.jsonl`, whose
//!   `token_count` lines each carry the session's running total.
//! - Claude Code: `$CLAUDE_CONFIG_DIR/projects/<slug>/<id>.jsonl`, and the
//!   subagent files under `<slug>/<id>/subagents/agent-*.jsonl`, whose
//!   `assistant` lines each carry one model request's usage.
//!
//! One [`LaunchTranscript`] reads for one launch, and counts only what that
//! launch spent: a resumed conversation's file already holds what earlier
//! launches spent, and each of them reports under a source of its own.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ariadne_core::TokenUsage;
use serde_json::Value;

/// Where the agents keep their transcripts.
#[derive(Debug, Clone)]
pub struct TranscriptHomes {
    /// Codex's home: `$CODEX_HOME`, else `~/.codex`.
    pub codex: PathBuf,
    /// Claude Code's home: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    pub claude: PathBuf,
    /// OpenCode's data directory, which holds `opencode.db`:
    /// `$XDG_DATA_HOME/opencode`, else `~/.local/share/opencode`.
    pub opencode: PathBuf,
}

impl TranscriptHomes {
    /// The homes the daemon's own environment names, which its agents inherit.
    pub(crate) fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let dir = |var: &str, default: &str| {
            std::env::var_os(var)
                .filter(|value| !value.is_empty())
                .map_or_else(|| home.join(default), PathBuf::from)
        };
        Self {
            codex: dir("CODEX_HOME", ".codex"),
            claude: dir("CLAUDE_CONFIG_DIR", ".claude"),
            opencode: dir("XDG_DATA_HOME", ".local/share").join("opencode"),
        }
    }
}

/// One launch's reading of its session's transcript.
pub(crate) struct LaunchTranscript {
    homes: TranscriptHomes,
    internal_session_id: String,
    cwd: PathBuf,
    /// Whether the launch resumed a conversation: what the file holds when
    /// the launch starts is then an earlier launch's, and not counted.
    resumed: bool,
    found: Option<Found>,
}

enum Found {
    Codex(CodexRollout),
    Claude(ClaudeTranscript),
}

impl LaunchTranscript {
    /// Start reading for a launch of `internal_session_id` in `cwd`. On a
    /// resume, whatever the file holds now is taken as the baseline.
    pub(crate) fn open(
        homes: TranscriptHomes,
        internal_session_id: &str,
        cwd: &Path,
        resumed: bool,
    ) -> Self {
        let mut transcript = Self {
            homes,
            internal_session_id: internal_session_id.to_string(),
            cwd: cwd.to_path_buf(),
            resumed,
            found: None,
        };
        transcript.find();
        transcript
    }

    /// What this launch has spent so far, or `None` where no transcript of
    /// the session is found.
    pub(crate) fn usage(&mut self) -> Option<TokenUsage> {
        if self.found.is_none() {
            self.find();
        }
        match self.found.as_mut()? {
            Found::Codex(rollout) => Some(rollout.read()),
            Found::Claude(transcript) => Some(transcript.read()),
        }
    }

    /// Look the file up; one found on a resume is read to its end as the
    /// baseline. A file that first appears later is new to this launch.
    fn find(&mut self) {
        // Only the lookup at the launch's start takes a baseline, whether it
        // finds the file or not.
        let baseline = std::mem::take(&mut self.resumed);
        let id = &self.internal_session_id;
        let claude = claude_project_dir(&self.homes.claude, &self.cwd)
            .map(|dir| {
                (
                    dir.join(format!("{id}.jsonl")),
                    dir.join(id).join("subagents"),
                )
            })
            .filter(|(main, _)| main.is_file());
        let found = match claude {
            Some((main, subagents)) => Found::Claude(ClaudeTranscript::new(main, subagents)),
            None => match codex_rollout(&self.homes.codex.join("sessions"), id) {
                Some(path) => Found::Codex(CodexRollout::new(path)),
                None => return,
            },
        };
        let found = self.found.insert(found);
        if baseline {
            match found {
                Found::Codex(rollout) => rollout.tail.skip_to_end(),
                Found::Claude(transcript) => transcript.take_baseline(),
            }
        }
    }
}

/// Claude Code's project directory for `cwd`: the real path, with every
/// character other than an ASCII letter or digit made a `-`. Symlinks are
/// resolved first, as Claude Code does: `/tmp/x` is `-private-tmp-x` on
/// macOS.
fn claude_project_dir(claude_home: &Path, cwd: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(cwd).ok()?;
    let slug: String = real
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Some(claude_home.join("projects").join(slug))
}

/// The rollout of `id` anywhere under Codex's `sessions` directory.
fn codex_rollout(dir: &Path, id: &str) -> Option<PathBuf> {
    let suffix = format!("{id}.jsonl");
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if let Some(found) = codex_rollout(&path, id) {
                return Some(found);
            }
        } else if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
        {
            return Some(path);
        }
    }
    None
}

/// A file read a line at a time as it grows: each read hands back the whole
/// lines written since the last, and keeps a line still being written for
/// the next.
struct Tail {
    path: PathBuf,
    offset: u64,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    /// The whole lines written since the last read, unparsed.
    fn new_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let read = File::open(&self.path).and_then(|mut file| {
            file.seek(SeekFrom::Start(self.offset))?;
            file.read_to_end(&mut bytes)
        });
        if read.is_err() {
            return Vec::new();
        }
        let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        self.offset += end as u64 + 1;
        bytes.truncate(end);
        bytes
    }

    fn new_lines(&mut self) -> Vec<Value> {
        self.new_bytes()
            .split(|&b| b == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }

    /// Pass over every whole line written so far, without parsing one.
    fn skip_to_end(&mut self) {
        self.new_bytes();
    }
}

/// A Codex rollout. Each `token_count` line carries the running total of
/// the process that wrote it, and a resume starts that total again from
/// zero in the same file, so the lines fall into segments: this launch's
/// figure is the last total of each segment, summed.
///
/// `input_tokens` already holds `cached_input_tokens`, and `output_tokens`
/// already holds `reasoning_output_tokens`, so neither pair is added. Nor is
/// `cache_write_input_tokens`: in 14156 `token_count` lines of Codex 0.13x
/// rollouts on 2026-09-19, `total_tokens` was always `input_tokens +
/// output_tokens` and `cache_write_input_tokens` always 0. A cache write is
/// part of the prompt, so it stays inside `input_tokens`, and
/// `cached_input_tokens` counts cache reads only.
struct CodexRollout {
    tail: Tail,
    /// The segments that have ended, summed.
    ended: TokenUsage,
    /// The last total of the segment being written.
    current: Option<TokenUsage>,
}

impl CodexRollout {
    fn new(path: PathBuf) -> Self {
        Self {
            tail: Tail::new(path),
            ended: TokenUsage::default(),
            current: None,
        }
    }

    fn read(&mut self) -> TokenUsage {
        for line in self.tail.new_lines() {
            let Some(total) = line
                .get("payload")
                .filter(|payload| {
                    payload.get("type").and_then(Value::as_str) == Some("token_count")
                })
                .and_then(|payload| payload.pointer("/info/total_token_usage"))
            else {
                continue;
            };
            let count = |key: &str| total.get(key).and_then(Value::as_u64).unwrap_or(0);
            let total = TokenUsage {
                input_tokens: count("input_tokens"),
                cached_input_tokens: count("cached_input_tokens"),
                output_tokens: count("output_tokens"),
            };
            if let Some(last) = self.current
                && (total.input_tokens < last.input_tokens
                    || total.output_tokens < last.output_tokens)
            {
                self.ended += last;
            }
            self.current = Some(total);
        }
        self.ended + self.current.unwrap_or_default()
    }
}

/// A Claude Code transcript and its subagents' files. One model request is
/// written over as many lines as it has content blocks, each with the same
/// usage, so requests are counted once each by `message.id`
/// ([`claude_request_usage`]).
struct ClaudeTranscript {
    main: Tail,
    subagents_dir: PathBuf,
    subagents: Vec<Tail>,
    /// The requests written before this launch: an earlier launch's.
    baseline: HashSet<String>,
    requests: HashMap<String, TokenUsage>,
}

impl ClaudeTranscript {
    fn new(main: PathBuf, subagents_dir: PathBuf) -> Self {
        Self {
            main: Tail::new(main),
            subagents_dir,
            subagents: Vec::new(),
            baseline: HashSet::new(),
            requests: HashMap::new(),
        }
    }

    fn take_baseline(&mut self) {
        self.read();
        self.baseline
            .extend(self.requests.drain().map(|(id, _)| id));
    }

    fn read(&mut self) -> TokenUsage {
        if let Ok(entries) = std::fs::read_dir(&self.subagents_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let named = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("agent-") && name.ends_with(".jsonl"));
                if named && !self.subagents.iter().any(|tail| tail.path == path) {
                    self.subagents.push(Tail::new(path));
                }
            }
        }
        let lines: Vec<Value> = std::iter::once(&mut self.main)
            .chain(self.subagents.iter_mut())
            .flat_map(Tail::new_lines)
            .collect();
        for line in lines {
            if line.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let (Some(id), Some(usage)) = (
                line.pointer("/message/id").and_then(Value::as_str),
                line.pointer("/message/usage"),
            ) else {
                continue;
            };
            if self.baseline.contains(id) {
                continue;
            }
            self.requests
                .insert(id.to_string(), claude_request_usage(usage));
        }
        self.requests.values().sum()
    }
}

/// What one model request of Claude Code's cost, off the `message.usage` of an
/// `assistant` line.
///
/// Input is `input_tokens` + `cache_read_input_tokens` +
/// `cache_creation_input_tokens`, and cached is the cache reads alone.
pub(crate) fn claude_request_usage(usage: &Value) -> TokenUsage {
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached_input_tokens = count("cache_read_input_tokens");
    TokenUsage {
        input_tokens: count("input_tokens")
            + cached_input_tokens
            + count("cache_creation_input_tokens"),
        cached_input_tokens,
        output_tokens: count("output_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::{LaunchTranscript, TranscriptHomes, claude_project_dir};
    use ariadne_core::TokenUsage;
    use serde_json::json;
    use std::io::Write;
    use std::path::Path;

    fn homes(dir: &Path) -> TranscriptHomes {
        TranscriptHomes {
            codex: dir.join("codex"),
            claude: dir.join("claude"),
            opencode: dir.join("opencode"),
        }
    }

    fn token_count(input: u64, cached: u64, output: u64) -> String {
        json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
            "total_token_usage": {"input_tokens": input, "cached_input_tokens": cached,
                "cache_write_input_tokens": 0, "output_tokens": output},
            "last_token_usage": {"input_tokens": 1, "cached_input_tokens": 1, "output_tokens": 1},
        }}})
        .to_string()
    }

    fn append(path: &Path, lines: &[String]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    fn rollout(dir: &Path) -> std::path::PathBuf {
        dir.join("codex/sessions/2026/09/19/rollout-2026-09-19T10-00-00-abc.jsonl")
    }

    #[test]
    fn a_codex_restart_inside_one_launch_adds_its_segments() {
        let dir = tempfile::tempdir().unwrap();
        append(
            &rollout(dir.path()),
            &[
                token_count(100, 50, 10),
                token_count(300, 200, 30),
                token_count(40, 20, 4),
            ],
        );
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), false);
        assert_eq!(
            transcript.usage(),
            Some(TokenUsage {
                input_tokens: 340,
                cached_input_tokens: 220,
                output_tokens: 34,
            })
        );
    }

    #[test]
    fn a_resumed_launch_counts_only_what_it_appends() {
        let dir = tempfile::tempdir().unwrap();
        append(&rollout(dir.path()), &[token_count(300, 200, 30)]);
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), true);
        assert_eq!(transcript.usage(), Some(TokenUsage::default()));
        append(&rollout(dir.path()), &[token_count(40, 20, 4)]);
        assert_eq!(
            transcript.usage(),
            Some(TokenUsage {
                input_tokens: 40,
                cached_input_tokens: 20,
                output_tokens: 4,
            })
        );
    }

    #[test]
    fn a_rollout_that_first_appears_after_a_resumed_launch_starts_is_all_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), true);
        append(&rollout(dir.path()), &[token_count(40, 20, 4)]);
        assert_eq!(
            transcript.usage(),
            Some(TokenUsage {
                input_tokens: 40,
                cached_input_tokens: 20,
                output_tokens: 4,
            })
        );
    }

    fn assistant(id: &str, input: u64, read: u64, created: u64, output: u64) -> String {
        json!({"type": "assistant", "message": {"id": id, "usage": {
            "input_tokens": input, "cache_read_input_tokens": read,
            "cache_creation_input_tokens": created, "output_tokens": output,
        }}})
        .to_string()
    }

    #[test]
    fn a_resumed_claude_launch_counts_only_the_requests_it_adds() {
        let dir = tempfile::tempdir().unwrap();
        let project = claude_project_dir(&dir.path().join("claude"), dir.path()).unwrap();
        let file = project.join("abc.jsonl");
        append(
            &file,
            &[
                assistant("msg_1", 1, 100, 10, 5),
                assistant("msg_2", 2, 200, 20, 6),
            ],
        );
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), true);
        append(
            &file,
            &[
                assistant("msg_3", 3, 300, 30, 7),
                assistant("msg_2", 2, 200, 20, 6),
            ],
        );
        assert_eq!(
            transcript.usage(),
            Some(TokenUsage {
                input_tokens: 333,
                cached_input_tokens: 300,
                output_tokens: 7,
            })
        );
    }

    #[test]
    fn a_line_still_being_written_waits_for_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = rollout(dir.path());
        append(&path, &[token_count(1, 0, 1)]);
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), false);
        let whole = token_count(9, 0, 9);
        let (head, rest) = whole.split_at(20);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(head.as_bytes())
            .unwrap();
        assert_eq!(transcript.usage().unwrap().input_tokens, 1);
        append(&path, &[rest.to_string()]);
        assert_eq!(transcript.usage().unwrap().input_tokens, 9);
    }

    #[test]
    fn no_transcript_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut transcript = LaunchTranscript::open(homes(dir.path()), "abc", dir.path(), false);
        assert_eq!(transcript.usage(), None);
    }

    #[test]
    fn a_claude_project_is_named_after_the_real_path() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.dir");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let canonical = std::fs::canonicalize(&real).unwrap();
        let expected: String = canonical.to_string_lossy().replace(['/', '.', '_'], "-");
        assert!(expected.ends_with("-real-dir"));
        assert_eq!(
            claude_project_dir(Path::new("/c"), &link),
            Some(Path::new("/c/projects").join(expected))
        );
    }
}
