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
//!
//! # Where each agent's own figure comes from, established on 2026-08-28
//!
//! Neither CLI has a command that prints the counters the contract asks for,
//! so what the [`captured`] tests compare against had to be found first, on
//! claude 2.1.251 and codex-cli 0.150.1:
//!
//! - **Claude Code.** `/cost` is no use on a subscription: it prints how much
//!   of the limits the last day and week used, and no token counts at all.
//!   `claude -p --output-format json` does print them, in two places that mean
//!   different things — `usage` is the parent transcript on its own, and
//!   `modelUsage` is the session including everything its subagents spent.
//!   `modelUsage` can also carry a *second* model, a short background call
//!   that titles the session; it is written to no transcript, so it is in
//!   neither reader's reach and in neither reader's answer.
//! - **Codex.** The `tokens used` a `codex exec` prints as it exits is the
//!   spend that was not served from cache — the last `total_token_usage` of
//!   that process, read as `input_tokens - cached_input_tokens +
//!   output_tokens`, and so a check on the whole reading rather than a fourth
//!   counter.

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
///
/// The parent does carry one echo of them: the `Task` tool result is a `user`
/// line whose `toolUseResult.usage` repeats the subagent's *last* message,
/// already counted in the subagent's own file. It has no `message.id` to
/// deduplicate it by, so what keeps it out is the assistant test in
/// [`claude_file`].
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
/// skipped — a shape rare enough that only 5 of the ~500 rollouts on the
/// machine the fixture came from carry one.
///
/// The report also breaks out a `cache_write_input_tokens`, which is not read:
/// `input_tokens` is the whole prompt whatever it is, and it has never been
/// anything but zero anyway (0 of 3094 reports on that machine).
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

/// The readers, against real records rather than against JSON this file wrote.
///
/// One session per agent was run on a developer machine on 2026-08-28 and its
/// transcript reduced to the fields these readers touch; what each agent said
/// it had spent was written down at the same time, and is the expected value
/// below. `fixtures/README.md` records how each capture was made and what was
/// stripped out of it.
#[cfg(test)]
mod captured {
    use super::*;

    use std::fs;

    use tempfile::TempDir;

    /// `~/.claude/projects/<slug>/<session>.jsonl`: three assistant messages
    /// over five usage-bearing lines, and — on a `user` line — the `usage` the
    /// `Task` tool result echoes back.
    const CLAUDE_SESSION: &str = include_str!("fixtures/claude-session.jsonl");

    /// `<session>/subagents/agent-*.jsonl`, written by the `Task` subagent the
    /// session above launched.
    const CLAUDE_SUBAGENT: &str = include_str!("fixtures/claude-subagent.jsonl");

    /// `~/.codex/sessions/2026/08/28/rollout-*.jsonl`: `codex exec`, then
    /// `codex exec resume --last` into the same file.
    const CODEX_ROLLOUT: &str = include_str!("fixtures/codex-rollout.jsonl");

    fn write(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create the directory");
        fs::write(&path, body).expect("write the fixture");
        path
    }

    /// The capture, laid out the way Claude Code laid it out.
    fn claude_capture(dir: &TempDir) -> std::path::PathBuf {
        let path = write(dir, "session.jsonl", CLAUDE_SESSION);
        write(
            dir,
            "session/subagents/agent-ad464e423450139cd.jsonl",
            CLAUDE_SUBAGENT,
        );
        path
    }

    /// What Claude Code itself reported when the captured session ended.
    ///
    /// `claude -p --output-format json` printed, in `modelUsage`:
    ///
    /// ```text
    /// "claude-sonnet-5": { "inputTokens": 10, "cacheReadInputTokens": 55108,
    ///                      "cacheCreationInputTokens": 19473, "outputTokens": 1059 }
    /// ```
    ///
    /// Three counters against the contract's three: the prompt total is the
    /// sum of the first three, and the cached part of it is the middle one.
    #[test]
    fn claude_reports_what_claude_code_reported() {
        let dir = TempDir::new().expect("a temp dir");
        assert_eq!(
            claude_usage(&claude_capture(&dir)),
            Some(TokenUsage {
                input_tokens: 10 + 55108 + 19473,
                cached_input_tokens: 55108,
                output_tokens: 1059,
            })
        );
    }

    /// The same run's `usage` — as opposed to its `modelUsage` — is the parent
    /// transcript alone, without the subagent: 6 input, 7339 cache creation,
    /// 44019 cache read, 778 output. Reading the parent on its own has to give
    /// exactly that, or the subagent files are being counted twice.
    #[test]
    fn a_claude_transcript_without_its_subagents_is_the_parent_alone() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "session.jsonl", CLAUDE_SESSION);
        assert_eq!(
            claude_usage(&path),
            Some(TokenUsage {
                input_tokens: 6 + 7339 + 44019,
                cached_input_tokens: 44019,
                output_tokens: 778,
            })
        );
    }

    /// The contract in `crates/ariadne-api/src/usage.rs`, on the capture.
    #[test]
    fn the_claude_capture_keeps_the_shared_contract() {
        let dir = TempDir::new().expect("a temp dir");
        let usage = claude_usage(&claude_capture(&dir)).expect("the capture reports usage");

        // `cached_input_tokens` is a subset of `input_tokens`, never added to
        // it: Claude Code counts the cache read (55108) separately from the
        // uncached prompt (10) and the cache write (19473), and the contract
        // wants all three in `input_tokens` with only the read in the subset.
        assert!(usage.cached_input_tokens <= usage.input_tokens);
        assert_eq!(usage.cached_input_tokens, 55108);
        assert_eq!(usage.input_tokens, 10 + 55108 + 19473);
        // The cache write is in there, which is the half a `input + read` sum
        // would silently drop.
        assert_eq!(usage.input_tokens - usage.cached_input_tokens, 10 + 19473);

        // Thinking is inside `output_tokens`, not beside it: the capture's
        // assistant messages carry 337 thinking tokens between them, and the
        // 1059 Claude Code reported already contains them.
        assert_eq!(usage.output_tokens, 1059);
        assert!(usage.output_tokens > 337);
        assert_eq!(
            thinking_tokens(CLAUDE_SESSION) + thinking_tokens(CLAUDE_SUBAGENT),
            337
        );

        // One message, several lines, one usage between them. The parent's
        // three messages arrive on five usage-bearing lines; summing lines
        // instead of ids would count two of them twice, and the transcript
        // has no other mark to tell a repeat from a second message.
        assert_eq!(usage_bearing_lines(CLAUDE_SESSION), 5);
        assert_eq!(message_ids(CLAUDE_SESSION), 3);
    }

    /// The `Task` tool result is a `user` line, and it repeats the subagent's
    /// last `usage` — 2 input, 1045 cache creation, 11089 cache read, already
    /// counted in the subagent's own transcript. Nothing deduplicates it,
    /// because it carries no `message.id`; the assistant test in
    /// [`claude_file`] is what keeps it out.
    #[test]
    fn a_claude_tool_result_carrying_usage_is_not_counted() {
        let echoes = CLAUDE_SESSION
            .lines()
            .filter(|line| line.contains("\"usage\"") && line.contains("\"type\":\"user\""))
            .count();
        assert_eq!(
            echoes, 1,
            "the capture no longer covers the tool-result case"
        );

        let dir = TempDir::new().expect("a temp dir");
        let usage = claude_usage(&claude_capture(&dir)).expect("the capture reports usage");
        assert_eq!(usage.input_tokens, 10 + 55108 + 19473);
        assert_ne!(usage.input_tokens, 10 + 55108 + 19473 + 2 + 1045 + 11089);
    }

    /// What codex itself reported for the captured rollout.
    ///
    /// The file holds two processes — the `codex exec` and the
    /// `codex exec resume --last` that continued it — and each one's own last
    /// `total_token_usage` is its whole spend:
    ///
    /// ```text
    /// input 27240, cached 24064, output 280   (first process)
    /// input 14490, cached 11008, output 154   (the resume)
    /// ```
    ///
    /// and on exit each printed a `tokens used` of its own, 3456 and 3636 —
    /// everything it was not served from cache.
    #[test]
    fn codex_reports_what_codex_reported() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "rollout.jsonl", CODEX_ROLLOUT);
        let usage = codex_usage(&path).expect("the capture reports usage");

        assert_eq!(
            usage,
            TokenUsage {
                input_tokens: 27240 + 14490,
                cached_input_tokens: 24064 + 11008,
                output_tokens: 280 + 154,
            }
        );
        assert_eq!(
            usage.input_tokens - usage.cached_input_tokens + usage.output_tokens,
            3456 + 3636,
        );
    }

    /// The contract in `crates/ariadne-api/src/usage.rs`, on the capture.
    #[test]
    fn the_codex_capture_keeps_the_shared_contract() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, "rollout.jsonl", CODEX_ROLLOUT);
        let usage = codex_usage(&path).expect("the capture reports usage");

        // Every report codex wrote says the same three things: the cached part
        // is inside the prompt total, the reasoning is inside the completion,
        // and codex's own `total_tokens` is the two totals and nothing else —
        // so neither the cache nor the reasoning is a fourth thing to add.
        let reports = codex_reports(CODEX_ROLLOUT);
        assert_eq!(reports.len(), 3);
        for report in &reports {
            let field = |name: &str| report[name].as_u64().expect("a counter");
            assert!(field("cached_input_tokens") <= field("input_tokens"));
            assert!(field("reasoning_output_tokens") <= field("output_tokens"));
            assert_eq!(
                field("total_tokens"),
                field("input_tokens") + field("output_tokens")
            );
        }
        assert!(usage.cached_input_tokens <= usage.input_tokens);

        // And each is cumulative for its process, so the answer is the last of
        // each segment — never the sum of the reports, which counts the whole
        // of the first process again on every report it made.
        assert_eq!(usage.input_tokens, 27240 + 14490);
        assert_ne!(usage.input_tokens, 13532 + 27240 + 14490);
        assert_eq!(usage.output_tokens, 280 + 154);
    }

    /// Every `total_token_usage` a rollout carries, in the order codex wrote
    /// them — the reports whose shape the reading rests on.
    fn codex_reports(rollout: &str) -> Vec<serde_json::Value> {
        rollout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|line| line["payload"]["info"]["total_token_usage"].clone())
            .filter(serde_json::Value::is_object)
            .collect()
    }

    /// The `thinking_tokens` a transcript declares, one message at a time, for
    /// the assertion that they are already inside `output_tokens`.
    fn thinking_tokens(transcript: &str) -> u64 {
        let mut seen = HashSet::new();
        assistant_lines(transcript)
            .filter(|line| seen.insert(line["message"]["id"].to_string()))
            .map(|line| {
                line["message"]["usage"]["output_tokens_details"]["thinking_tokens"]
                    .as_u64()
                    .unwrap_or(0)
            })
            .sum()
    }

    /// How many lines carry a `message.usage` — more than there are messages.
    fn usage_bearing_lines(transcript: &str) -> usize {
        assistant_lines(transcript)
            .filter(|line| line["message"]["usage"].is_object())
            .count()
    }

    /// How many distinct `message.id`s those lines are between them.
    fn message_ids(transcript: &str) -> usize {
        assistant_lines(transcript)
            .filter_map(|line| line["message"]["id"].as_str().map(str::to_string))
            .collect::<HashSet<_>>()
            .len()
    }

    fn assistant_lines(transcript: &str) -> impl Iterator<Item = serde_json::Value> {
        transcript
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|line| line["type"] == "assistant")
    }
}
