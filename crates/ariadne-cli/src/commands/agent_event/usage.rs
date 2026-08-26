//! Token usage, read out of the agent's own transcript.
//!
//! Claude Code and Codex both hand their hooks a `transcript_path`, and both
//! write their token counters into that file. Neither reports them to the hook
//! itself, so this is the only place the numbers exist.
//!
//! Both readers are cumulative for the file they read: they report the whole
//! transcript so far, never a delta, so a lost or repeated event costs
//! nothing. Both are fail-safe: any surprise — a missing file, a line that is
//! not JSON, a shape nobody expected — yields `None` and no field.
//!
//! # What resuming does, verified on 2026-08-26
//!
//! Measured against the installed CLIs (claude 2.1.246, codex-cli 0.149.1) by
//! running a session, resuming it and reading the files back.
//!
//! - `claude --resume <id>` keeps the session id and **appends to the same
//!   transcript file**. Nothing is rewritten, so summing the file is right.
//! - `codex resume <thread>` also **continues the same rollout file**, but the
//!   resumed process starts its own `total_token_usage` from zero, and no
//!   `session_meta` line marks the boundary (real rollouts carry exactly one,
//!   written at creation). Reading only the last `token_count` would therefore
//!   lose everything before the last resume, so [`codex_usage`] splits the
//!   file into segments and sums the last total of each.
//!
//!   A segment boundary is not a drop in the running total: the resumed
//!   process re-sends the whole conversation as its first prompt, so its first
//!   total is usually *larger* than the one before it (14630 -> 14860 in the
//!   measured pair). What marks it is codex's own bookkeeping,
//!   `total = previous total + last_token_usage`: only a process's first
//!   report has `total == last`. Checked over 211 real rollouts — 2453 reports
//!   continued their total that way, 101 started a segment, and 3 repeated the
//!   previous total unchanged. That last shape is a report codex emitted
//!   twice: it is recognised by every counter standing still, and skipped
//!   before the segment test, so that a repeated first report — which is its
//!   own `last_token_usage`, and so reads as a restart — neither opens a
//!   segment nor counts again.
//!
//! Because the daemon keys what it stores on `source`, an agent that started a
//! new file on resume would stay correct too — but neither of these does.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Cumulative counters for one transcript.
///
/// `input_tokens` counts every prompt token, cache reads and cache writes
/// included; `cached_input_tokens` is the part of it served from the prompt
/// cache; `output_tokens` counts completion tokens, thinking and reasoning
/// included.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

/// Transcripts larger than this are not read at all.
///
/// The hook has 2s for everything it does, and a file this size is a corrupt
/// or pathological one rather than a conversation.
const MAX_TRANSCRIPT_BYTES: u64 = 256 * 1024 * 1024;

/// Sum a Claude Code transcript, and the subagent transcripts beside it.
///
/// `~/.claude/projects/<slug>/<session>.jsonl` records one assistant message
/// as **several lines sharing one `message.id`**, each repeating the same
/// `message.usage` (46 lines for 16 ids in a real transcript), so a
/// line-by-line sum overcounts about threefold: each id is counted once here.
///
/// Subagents write to `<session>/subagents/agent-*.jsonl` in the same format.
/// Their messages never appear in the parent transcript (verified: zero
/// overlap between the two id sets) and they are part of what the session
/// spent, so they are summed in.
pub fn claude_usage(transcript: &Path) -> Option<TokenUsage> {
    let mut total = TokenUsage::default();
    let mut seen = HashSet::new();
    let mut found = claude_file(transcript, &mut total, &mut seen);

    if let (Some(dir), Some(stem)) = (transcript.parent(), transcript.file_stem())
        && let Ok(entries) = std::fs::read_dir(dir.join(stem).join("subagents"))
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                found |= claude_file(&path, &mut total, &mut seen);
            }
        }
    }

    found.then_some(total)
}

/// Add one Claude transcript file to `total`, skipping ids already in `seen`.
/// Reports whether it contributed anything.
fn claude_file(path: &Path, total: &mut TokenUsage, seen: &mut HashSet<String>) -> bool {
    let mut found = false;
    for line in lines(path) {
        // Cheap gate: only assistant lines carry usage, and parsing every line
        // of a multi-megabyte transcript is the expensive part.
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let message = &value["message"];
        // No id, no way to tell a repeated line from a second message: leave
        // it out rather than risk counting one message three times.
        let Some(id) = message.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let usage = &message["usage"];
        let field = |name: &str| usage.get(name).and_then(serde_json::Value::as_u64);
        let (Some(input), Some(output)) = (field("input_tokens"), field("output_tokens")) else {
            continue;
        };
        let cached = field("cache_read_input_tokens").unwrap_or(0);
        let created = field("cache_creation_input_tokens").unwrap_or(0);
        total.input_tokens += input + cached + created;
        total.cached_input_tokens += cached;
        total.output_tokens += output;
        found = true;
    }
    found
}

/// Read the running totals out of a Codex rollout.
///
/// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` carries
/// `{"type":"event_msg","payload":{"type":"token_count","info":{…}}}` lines
/// whose `info.total_token_usage` is cumulative for the process —
/// `input_tokens` already includes the cached part and `output_tokens` already
/// includes reasoning. `info` is `null` on rate-limit-only updates; those are
/// skipped.
///
/// A resume restarts that total in place (see the module docs), so the file is
/// read as a sequence of segments — a report whose total is its own
/// `last_token_usage` rather than the previous total plus it starts a new one
/// — and the answer is the sum of the last total of each.
pub fn codex_usage(transcript: &Path) -> Option<TokenUsage> {
    /// What a report has to be compared on: `cached_input_tokens` is a subset
    /// of `input_tokens` and would count twice in a sum.
    fn spent(usage: &TokenUsage) -> u64 {
        usage.input_tokens + usage.output_tokens
    }

    let mut banked = TokenUsage::default();
    let mut segment: Option<TokenUsage> = None;

    for line in lines(transcript) {
        if !line.contains("token_count") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let payload = &value["payload"];
        if value.get("type").and_then(|v| v.as_str()) != Some("event_msg")
            || payload.get("type").and_then(|v| v.as_str()) != Some("token_count")
        {
            continue;
        }
        // Skipped by the same miss as the `"info": null` rate-limit updates.
        let Some(total) = counters(&payload["info"]["total_token_usage"]) else {
            continue;
        };
        // A report codex emitted twice: every counter stands still, so there
        // is nothing to add and nothing to start. It has to be caught before
        // the segment test, or a repeated *first* report of a process — which
        // is its own `last_token_usage`, and so looks exactly like a restart —
        // would bank the segment it repeats and count it twice.
        if segment == Some(total) {
            continue;
        }
        let last = counters(&payload["info"]["last_token_usage"]);

        let restarted = match (segment, last) {
            // A process's first report is its own last one. Without a
            // `last_token_usage` to say so, only a total that went backwards
            // is evidence of a restart.
            (Some(previous), Some(last)) => {
                spent(&total) == spent(&last) && spent(&total) != spent(&previous) + spent(&last)
            }
            (Some(previous), None) => spent(&total) < spent(&previous),
            (None, _) => false,
        };
        if restarted && let Some(previous) = segment {
            banked.input_tokens += previous.input_tokens;
            banked.cached_input_tokens += previous.cached_input_tokens;
            banked.output_tokens += previous.output_tokens;
        }
        segment = Some(total);
    }

    let last = segment?;
    Some(TokenUsage {
        input_tokens: banked.input_tokens + last.input_tokens,
        cached_input_tokens: banked.cached_input_tokens + last.cached_input_tokens,
        output_tokens: banked.output_tokens + last.output_tokens,
    })
}

/// One `*_token_usage` object of a codex report.
fn counters(usage: &serde_json::Value) -> Option<TokenUsage> {
    let field = |name: &str| usage.get(name).and_then(serde_json::Value::as_u64);
    Some(TokenUsage {
        input_tokens: field("input_tokens")?,
        cached_input_tokens: field("cached_input_tokens").unwrap_or(0),
        output_tokens: field("output_tokens")?,
    })
}

/// The lines of a transcript, or none of them if it cannot be read.
///
/// Lines that are not UTF-8 are dropped rather than ending the read: one bad
/// line must not cost the rest of the file.
fn lines(path: &Path) -> impl Iterator<Item = String> {
    let readable = std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_TRANSCRIPT_BYTES)
        .and_then(|_| File::open(path).ok());
    readable
        .into_iter()
        .flat_map(|file| BufReader::new(file).lines().map_while(Result::ok))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    /// One assistant message written over two lines (same `message.id`, same
    /// usage, as Claude writes it) and a second message.
    const CLAUDE: &str = r#"{"type":"user","message":{"role":"user"}}
{"type":"assistant","message":{"id":"msg_a","usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":7}}}
{"type":"assistant","message":{"id":"msg_a","usage":{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":7}}}
{"type":"assistant","message":{"id":"msg_b","usage":{"input_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":2000,"output_tokens":3}}}
"#;

    const SUBAGENT: &str = r#"{"type":"assistant","message":{"id":"msg_s","usage":{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":4,"output_tokens":8}}}
"#;

    /// One report: what the process has spent in all, and what its last
    /// request cost. The two are equal on the first report of a process.
    fn token_count(total: (u64, u64, u64), last: (u64, u64, u64)) -> String {
        let usage = |(input, cached, output): (u64, u64, u64)| {
            format!(
                r#"{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{output},"reasoning_output_tokens":0,"total_tokens":{}}}"#,
                input + output
            )
        };
        format!(
            r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{},"last_token_usage":{}}}}}}}"#,
            usage(total),
            usage(last),
        )
    }

    fn write(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create the directory");
        fs::write(&path, body).expect("write the fixture");
        path
    }

    #[test]
    fn a_claude_message_split_over_lines_counts_once() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "session.jsonl", CLAUDE);
        assert_eq!(
            claude_usage(&path),
            Some(TokenUsage {
                input_tokens: 10 + 100 + 1000 + 5 + 2000,
                cached_input_tokens: 1000 + 2000,
                output_tokens: 7 + 3,
            })
        );
    }

    #[test]
    fn claude_subagent_transcripts_are_summed_in() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "session.jsonl", CLAUDE);
        write(&dir, "session/subagents/agent-abc.jsonl", SUBAGENT);
        assert_eq!(
            claude_usage(&path),
            Some(TokenUsage {
                input_tokens: 10 + 100 + 1000 + 5 + 2000 + 1 + 2 + 4,
                cached_input_tokens: 1000 + 2000 + 4,
                output_tokens: 7 + 3 + 8,
            })
        );
    }

    #[test]
    fn a_claude_transcript_without_usable_lines_is_none() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "broken.jsonl", "not json\n{\"usage\": oops\n{}\n");
        assert_eq!(claude_usage(&path), None);
        assert_eq!(claude_usage(&dir.path().join("absent.jsonl")), None);
    }

    #[test]
    fn codex_reads_the_last_total_and_skips_a_null_info() {
        let dir = TempDir::new().expect("a temp dir");
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"type":"session_meta","payload":{"id":"01a0"}}"#,
            token_count((1000, 800, 20), (1000, 800, 20)),
            token_count((2400, 1600, 45), (1400, 800, 25)),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{}}}"#,
        );
        let path = write(&dir, "rollout.jsonl", &body);
        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 2400,
                cached_input_tokens: 1600,
                output_tokens: 45,
            })
        );
    }

    /// A resume restarts the running total in the same file, and its first
    /// report is larger than the total it restarted from — the whole
    /// conversation goes back in as the prompt. Both segments count.
    #[test]
    fn codex_sums_the_segments_a_resume_leaves_behind() {
        let dir = TempDir::new().expect("a temp dir");
        let body = format!(
            "{}\n{}\n{}\n{}\n",
            token_count((1000, 800, 20), (1000, 800, 20)),
            token_count((2400, 1600, 45), (1400, 800, 25)),
            token_count((3000, 900, 5), (3000, 900, 5)),
            token_count((3900, 1200, 11), (900, 300, 6)),
        );
        let path = write(&dir, "rollout.jsonl", &body);
        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 2400 + 3900,
                cached_input_tokens: 1600 + 1200,
                output_tokens: 45 + 11,
            })
        );
    }

    /// A report codex emitted twice: the total stands still, and it must not
    /// be read as the start of anything.
    #[test]
    fn a_repeated_codex_report_counts_once() {
        let dir = TempDir::new().expect("a temp dir");
        let body = format!(
            "{}\n{}\n{}\n",
            token_count((1000, 800, 20), (1000, 800, 20)),
            token_count((2400, 1600, 45), (1400, 800, 25)),
            token_count((2400, 1600, 45), (1400, 800, 25)),
        );
        let path = write(&dir, "rollout.jsonl", &body);
        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 2400,
                cached_input_tokens: 1600,
                output_tokens: 45,
            })
        );
    }

    /// The report a process opens with is its own `last_token_usage`, which
    /// is what a resume looks like too. Repeated, it must still not bank the
    /// segment it repeats.
    #[test]
    fn a_repeated_first_codex_report_is_not_a_resume() {
        let dir = TempDir::new().expect("a temp dir");
        let body = format!(
            "{}\n{}\n{}\n",
            token_count((1000, 800, 20), (1000, 800, 20)),
            token_count((1000, 800, 20), (1000, 800, 20)),
            token_count((2400, 1600, 45), (1400, 800, 25)),
        );
        let path = write(&dir, "rollout.jsonl", &body);
        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 2400,
                cached_input_tokens: 1600,
                output_tokens: 45,
            })
        );
    }

    #[test]
    fn a_codex_rollout_without_token_counts_is_none() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "rollout.jsonl", "not json\ntoken_count\n{}\n");
        assert_eq!(codex_usage(&path), None);
        assert_eq!(codex_usage(&dir.path().join("absent.jsonl")), None);
    }
}
