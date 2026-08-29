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
//!   `session_meta` line marks the boundary (a rollout carries one, written at
//!   creation — a sub-agent's carries a second, see below). Reading only the
//!   last `token_count` would therefore lose everything before the last
//!   resume, so [`codex_usage`] splits the file into segments and sums the
//!   last total of each.
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
//!
//! # What a Codex sub-agent looks like, verified on 2026-08-29
//!
//! codex-cli 0.150.1, `multi_agent` stable and enabled, spawning one sub-agent
//! from a `codex exec` and waiting for it. Every spawned agent is a *thread* of
//! its own, with its own rollout file and its own `total_token_usage`; the
//! parent's reports never include it, so [`codex_usage`] reads the children
//! too.
//!
//! - **The parent names its children.** A spawn writes an `event_msg` of
//!   `item_completed` whose `item` is
//!   `{"type":"SubAgentActivity","kind":"started","agent_thread_id":"<child>",
//!   "agent_path":"/root/<task name>"}`, and a second one with
//!   `"kind":"completed"` when the child finishes. That `agent_thread_id` is
//!   the only place the child's id appears: the `spawn_agent` call is a
//!   `function_call` in the `collaboration` namespace whose output is nothing
//!   but `{"task_name":"/root/<task name>"}`, and the `wait_agent` call's
//!   `CollabAgentToolCall` item had empty `receiver_thread_ids`.
//! - **The child is a rollout beside the parent's.**
//!   `<sessions>/YYYY/MM/DD/rollout-<timestamp>-<child thread id>.jsonl`,
//!   under the date the child was *created* — the parent's own day directory
//!   for anything but a session that crossed midnight, and never an earlier
//!   one. That is what bounds the lookup in [`codex_rollout_of`].
//! - **The child says whose it is.** Its first `session_meta` carries
//!   `"thread_source":"subagent"`, `parent_thread_id`, `forked_from_id`, a
//!   `source` of `{"subagent":{"thread_spawn":{"parent_thread_id":…,
//!   "depth":1,"agent_path":…}}}` and `subagent_history_start_ordinal`, the
//!   turn the fork was taken at. Only the `thread_source` is read, and only to
//!   answer the paragraph below — finding a child is the parent's job, and
//!   reading rollouts to learn whose they are would mean opening the whole
//!   sessions tree.
//! - **A child rollout carries two `session_meta` lines**, not the one every
//!   other rollout has: its own first, then a copy of the parent's, as the
//!   head of the history it was forked with. So the thread a rollout *is* is
//!   the first one, and taking the last would name the parent.
//! - **Codex runs Ariadne's hooks for a sub-agent thread too.** A child's own
//!   tool calls fire `PreToolUse` and `PostToolUse` with the *child's* rollout
//!   as `transcript_path` and the *parent's* thread id as `session_id` — so
//!   they arrive as the same Ariadne session, and `post_tool_use` is one of
//!   the events that reads the transcript. Left alone, a child would be
//!   counted twice: once inside the figure its parent now reports, and once
//!   again under a `source` of its own. So [`codex_usage`] answers `None` for
//!   a sub-agent's rollout, which is Ariadne's side of what the OpenCode
//!   plugin does with `if (session.parentID) return undefined`. (The verified
//!   run fired no `SessionStart`, `UserPromptSubmit`, `Stop` or `SessionEnd`
//!   for the child; those are the other events that read a transcript, and
//!   the same answer covers them.)

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

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

/// How many rollouts one reading may open, the thread it was asked about
/// included.
///
/// A team of codex agents is a handful of threads — the developer message the
/// verified run carried offers four concurrency slots — and a spawn tree far
/// past this is a runaway rather than a session. The hook has 2s for
/// everything it does, so it stops counting rather than keeps opening files.
const MAX_ROLLOUTS: usize = 64;

/// How far into a rollout a `session_meta` line can be.
///
/// Codex writes one at creation, and a sub-agent's rollout opens with its own
/// and then a copy of its parent's — lines 0 and 1 of the verified capture.
/// Nothing past the head of the file is scanned for it, so a rollout that
/// carries none costs one extra look at four lines rather than at every line
/// of a multi-megabyte one.
const META_LINES: usize = 4;

/// Sum a Codex rollout, and every rollout the threads in it spawned.
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
/// A resume restarts that total in place (see the module docs), so each file
/// is read as a sequence of segments — a report whose total is its own
/// `last_token_usage` rather than the previous total plus it starts a new one
/// — and its figure is the sum of the last total of each.
///
/// A sub-agent is a thread of its own whose spend appears in no rollout but
/// its own (see the module docs), so the rollouts a thread spawned are read
/// too, and the ones *they* spawned after them. Every one of those is a bonus:
/// a child that cannot be found, or is unreadable, or is empty, adds nothing
/// and leaves the answer the parent's own. A thread counts once however many
/// routes name it — a rollout names each child twice, spawned and finished,
/// and a sibling forked from the same parent carries the history that named
/// the earlier ones.
///
/// And a sub-agent's own rollout answers nothing at all. Codex runs the hooks
/// of a child thread under the parent's `session_id`, so an answer here would
/// reach the daemon as a second `source` of the same session and count the
/// child twice. Its spend is the thread that spawned it to report.
pub fn codex_usage(transcript: &Path) -> Option<TokenUsage> {
    let mut total = TokenUsage::default();
    let mut found = false;
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue = VecDeque::from([transcript.to_path_buf()]);
    // True while the rollout in hand is still the one the caller asked about.
    let mut asked_about = true;

    for _ in 0..MAX_ROLLOUTS {
        let Some(path) = queue.pop_front() else {
            break;
        };
        let rollout = codex_file(&path);
        if asked_about && rollout.subagent {
            return None;
        }
        asked_about = false;
        if let Some(usage) = rollout.usage {
            total.input_tokens += usage.input_tokens;
            total.cached_input_tokens += usage.cached_input_tokens;
            total.output_tokens += usage.output_tokens;
            found = true;
        }
        // Before the children, so that a rollout naming itself — or an
        // ancestor — is a thread already seen rather than one to open again.
        if let Some(thread) = rollout.thread {
            seen.insert(thread);
        }
        for child in rollout.children {
            if seen.insert(child.clone())
                && let Some(path) = codex_rollout_of(&path, &child)
            {
                queue.push_back(path);
            }
        }
    }

    found.then_some(total)
}

/// One codex rollout, read once.
struct Rollout {
    /// What the processes that wrote it spent, or `None` if it reported
    /// nothing — an unreadable file among them.
    usage: Option<TokenUsage>,
    /// The thread it is, from the *first* `session_meta`: a sub-agent's
    /// rollout carries the parent's after its own.
    thread: Option<String>,
    /// Whether that first `session_meta` calls it a sub-agent's.
    subagent: bool,
    /// The threads it spawned, in the order it named them.
    children: Vec<String>,
}

fn codex_file(path: &Path) -> Rollout {
    let mut banked = TokenUsage::default();
    let mut segment: Option<TokenUsage> = None;
    let mut thread: Option<String> = None;
    let mut subagent = false;
    let mut meta_read = false;
    let mut children = Vec::new();

    for (index, line) in lines(path).enumerate() {
        // Cheap gates, and one scan of a line before it is rejected: both
        // shapes the counting rests on are `event_msg`s, so the conversation
        // itself — the bulk of a rollout, and the multi-megabyte part of a
        // large one — costs the same single `contains` it always did. What
        // reaches the second scan and the parse is a few dozen small lines.
        if line.contains("\"event_msg\"") {
            if line.contains("token_count") {
                token_count(&line, &mut banked, &mut segment);
            } else if line.contains("SubAgentActivity") {
                spawned_thread(&line, &mut children);
            }
        } else if index < META_LINES && !meta_read && line.contains("session_meta") {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if value.get("type").and_then(|v| v.as_str()) != Some("session_meta") {
                continue;
            }
            meta_read = true;
            thread = value["payload"]["id"].as_str().map(str::to_string);
            subagent = value["payload"]["thread_source"] == "subagent";
        }
    }

    Rollout {
        usage: segment.map(|last| TokenUsage {
            input_tokens: banked.input_tokens + last.input_tokens,
            cached_input_tokens: banked.cached_input_tokens + last.cached_input_tokens,
            output_tokens: banked.output_tokens + last.output_tokens,
        }),
        thread,
        subagent,
        children,
    }
}

/// Fold one `token_count` line into the segments read so far.
fn token_count(line: &str, banked: &mut TokenUsage, segment: &mut Option<TokenUsage>) {
    /// What a report has to be compared on: `cached_input_tokens` is a subset
    /// of `input_tokens` and would count twice in a sum.
    fn spent(usage: &TokenUsage) -> u64 {
        usage.input_tokens + usage.output_tokens
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let payload = &value["payload"];
    if value.get("type").and_then(|v| v.as_str()) != Some("event_msg")
        || payload.get("type").and_then(|v| v.as_str()) != Some("token_count")
    {
        return;
    }
    // Skipped by the same miss as the `"info": null` rate-limit updates.
    let Some(total) = counters(&payload["info"]["total_token_usage"]) else {
        return;
    };
    // A report codex emitted twice: every counter stands still, so there is
    // nothing to add and nothing to start. It has to be caught before the
    // segment test, or a repeated *first* report of a process — which is its
    // own `last_token_usage`, and so looks exactly like a restart — would bank
    // the segment it repeats and count it twice.
    if *segment == Some(total) {
        return;
    }
    let last = counters(&payload["info"]["last_token_usage"]);

    let restarted = match (*segment, last) {
        // A process's first report is its own last one. Without a
        // `last_token_usage` to say so, only a total that went backwards is
        // evidence of a restart.
        (Some(previous), Some(last)) => {
            spent(&total) == spent(&last) && spent(&total) != spent(&previous) + spent(&last)
        }
        (Some(previous), None) => spent(&total) < spent(&previous),
        (None, _) => false,
    };
    if restarted && let Some(previous) = *segment {
        banked.input_tokens += previous.input_tokens;
        banked.cached_input_tokens += previous.cached_input_tokens;
        banked.output_tokens += previous.output_tokens;
    }
    *segment = Some(total);
}

/// Add the thread one `SubAgentActivity` line names to `children`.
fn spawned_thread(line: &str, children: &mut Vec<String>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let item = &value["payload"]["item"];
    if item.get("type").and_then(|v| v.as_str()) != Some("SubAgentActivity") {
        return;
    }
    if let Some(child) = item.get("agent_thread_id").and_then(|v| v.as_str()) {
        children.push(child.to_string());
    }
}

/// The rollout of `thread`, given the rollout of the thread that spawned it.
///
/// Codex files a rollout under `<sessions>/YYYY/MM/DD/` by the local date the
/// thread was created on, and names it `rollout-<timestamp>-<thread>.jsonl`. A
/// spawned thread is created no earlier than its spawner, so its file is in
/// the spawner's own day directory — where everything but a session that
/// crossed midnight ends — or in a later one, and the days before it are never
/// opened.
///
/// That bound is the point. The other way to find a child is to read every
/// rollout under `~/.codex/sessions` for the `parent_thread_id` in its
/// `session_meta`; the tree holds hundreds of files across years of days and
/// the hook has 2s for everything it does, so nothing here opens a file it was
/// not sent to by name.
fn codex_rollout_of(spawner: &Path, thread: &str) -> Option<PathBuf> {
    let day = spawner.parent()?;
    rollout_in(day, thread).or_else(|| {
        later_days(day)
            .into_iter()
            .find_map(|dir| rollout_in(&dir, thread))
    })
}

/// The rollout of `thread` in one day directory, by the id its name ends with.
fn rollout_in(dir: &Path, thread: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread}.jsonl");
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        path.file_name()?
            .to_str()?
            .ends_with(&suffix)
            .then_some(path)
    })
}

/// The `<sessions>/YYYY/MM/DD` directories after `day`, oldest first.
///
/// The components are zero-padded and fixed-width, so they sort as the dates
/// do and a year or a month before the spawner's is skipped whole.
fn later_days(day: &Path) -> Vec<PathBuf> {
    let Some((root, from_year, from_month, from_day)) = day_parts(day) else {
        return Vec::new();
    };
    let mut days = Vec::new();
    for year in sorted_dirs(&root).into_iter().filter(|it| *it >= from_year) {
        let in_year = root.join(&year);
        for month in sorted_dirs(&in_year)
            .into_iter()
            .filter(|it| year > from_year || *it >= from_month)
        {
            let in_month = in_year.join(&month);
            days.extend(
                sorted_dirs(&in_month)
                    .into_iter()
                    .filter(|it| year > from_year || month > from_month || *it > from_day)
                    .map(|it| in_month.join(it)),
            );
        }
    }
    days
}

/// A `<sessions>/YYYY/MM/DD` path, split into the sessions root and the date.
fn day_parts(day: &Path) -> Option<(PathBuf, String, String, String)> {
    let name = |dir: &Path| dir.file_name()?.to_str().map(str::to_string);
    let month = day.parent()?;
    let year = month.parent()?;
    Some((
        year.parent()?.to_path_buf(),
        name(year)?,
        name(month)?,
        name(day)?,
    ))
}

/// The subdirectory names of `dir`, sorted; none of them if it cannot be read.
fn sorted_dirs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
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

    /// The `session_meta` a rollout opens with, naming the thread it is.
    fn session_meta(thread: &str) -> String {
        format!(r#"{{"type":"session_meta","payload":{{"id":"{thread}","thread_source":"user"}}}}"#)
    }

    /// The one a sub-agent's rollout opens with, and the copy of its parent's
    /// that follows it.
    fn subagent_meta(thread: &str, parent: &str) -> String {
        format!(
            "{}\n{}",
            format_args!(
                r#"{{"type":"session_meta","payload":{{"id":"{thread}","parent_thread_id":"{parent}","thread_source":"subagent"}}}}"#
            ),
            session_meta(parent),
        )
    }

    /// What a spawn writes in the rollout of the thread that spawned it.
    fn sub_agent(child: &str) -> String {
        format!(
            r#"{{"type":"event_msg","payload":{{"type":"item_completed","item":{{"type":"SubAgentActivity","kind":"started","agent_thread_id":"{child}"}}}}}}"#
        )
    }

    /// A rollout of one thread: what it is, what it spawned, what it spent.
    fn rollout(thread: &str, children: &[&str], total: (u64, u64, u64)) -> String {
        spawned_rollout(&session_meta(thread), children, total)
    }

    /// The same, for a thread another one spawned.
    fn subagent_rollout(
        thread: &str,
        parent: &str,
        children: &[&str],
        total: (u64, u64, u64),
    ) -> String {
        spawned_rollout(&subagent_meta(thread, parent), children, total)
    }

    fn spawned_rollout(meta: &str, children: &[&str], total: (u64, u64, u64)) -> String {
        let mut body = meta.to_string();
        for child in children {
            body.push('\n');
            body.push_str(&sub_agent(child));
        }
        body.push('\n');
        body.push_str(&token_count(total, total));
        body.push('\n');
        body
    }

    /// Where codex files the rollout of `thread` created on `day`.
    fn rollout_path(day: &str, thread: &str) -> String {
        format!(
            "{day}/rollout-{}T00-00-00-{thread}.jsonl",
            day.replace('/', "-")
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

    /// A sub-agent's spend is in its own rollout and in no other, so the
    /// thread it was spawned from answers for both — and for what its own
    /// sub-agents spent under it.
    #[test]
    fn codex_subagent_rollouts_are_summed_in() {
        let dir = TempDir::new().expect("a temp dir");
        let day = "2026/08/29";
        let path = write(
            &dir,
            &rollout_path(day, "01a0aaaa"),
            &rollout("01a0aaaa", &["01a0bbbb"], (1000, 800, 20)),
        );
        write(
            &dir,
            &rollout_path(day, "01a0bbbb"),
            &subagent_rollout("01a0bbbb", "01a0aaaa", &["01a0cccc"], (500, 100, 9)),
        );
        write(
            &dir,
            &rollout_path(day, "01a0cccc"),
            &subagent_rollout("01a0cccc", "01a0bbbb", &[], (70, 30, 4)),
        );

        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 1000 + 500 + 70,
                cached_input_tokens: 800 + 100 + 30,
                output_tokens: 20 + 9 + 4,
            })
        );
    }

    /// A child is a bonus: one that was never written, or cannot be read,
    /// leaves the figure the parent's own rather than taking it away.
    #[test]
    fn a_codex_subagent_rollout_that_is_gone_leaves_the_parent_alone() {
        let dir = TempDir::new().expect("a temp dir");
        let day = "2026/08/29";
        let path = write(
            &dir,
            &rollout_path(day, "01a0aaaa"),
            &rollout("01a0aaaa", &["01a0bbbb", "01a0cccc"], (1000, 800, 20)),
        );
        write(&dir, &rollout_path(day, "01a0cccc"), "not json\n{}\n");

        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 1000,
                cached_input_tokens: 800,
                output_tokens: 20,
            })
        );
    }

    /// One thread, however many routes name it: a rollout names each child
    /// twice — spawned, then finished — and two siblings forked from the same
    /// parent carry the history that named the first of them.
    #[test]
    fn a_codex_thread_reachable_twice_counts_once() {
        let dir = TempDir::new().expect("a temp dir");
        let day = "2026/08/29";
        let path = write(
            &dir,
            &rollout_path(day, "01a0aaaa"),
            &rollout(
                "01a0aaaa",
                &["01a0bbbb", "01a0cccc", "01a0bbbb"],
                (1000, 800, 20),
            ),
        );
        write(
            &dir,
            &rollout_path(day, "01a0bbbb"),
            &subagent_rollout("01a0bbbb", "01a0aaaa", &[], (500, 100, 9)),
        );
        // The second child was forked after the first was spawned, so its own
        // history names it too.
        write(
            &dir,
            &rollout_path(day, "01a0cccc"),
            &subagent_rollout("01a0cccc", "01a0aaaa", &["01a0bbbb"], (70, 30, 4)),
        );

        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 1000 + 500 + 70,
                cached_input_tokens: 800 + 100 + 30,
                output_tokens: 20 + 9 + 4,
            })
        );
    }

    /// And a rollout that names itself is a thread already counted, not a
    /// file to open again.
    #[test]
    fn a_codex_rollout_that_names_itself_counts_once() {
        let dir = TempDir::new().expect("a temp dir");
        let day = "2026/08/29";
        let path = write(
            &dir,
            &rollout_path(day, "01a0aaaa"),
            &rollout("01a0aaaa", &["01a0aaaa"], (1000, 800, 20)),
        );

        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 1000,
                cached_input_tokens: 800,
                output_tokens: 20,
            })
        );
    }

    /// Codex fires a child thread's hooks under the parent's `session_id`, so
    /// a rollout that is a sub-agent's answers nothing: the thread that
    /// spawned it reports its spend, and a second answer here would be the
    /// same tokens under a second `source` of the same session.
    #[test]
    fn a_codex_subagent_rollout_asked_about_on_its_own_reports_nothing() {
        let dir = TempDir::new().expect("a temp dir");
        let day = "2026/08/29";
        write(
            &dir,
            &rollout_path(day, "01a0aaaa"),
            &rollout("01a0aaaa", &["01a0bbbb"], (1000, 800, 20)),
        );
        let child = write(
            &dir,
            &rollout_path(day, "01a0bbbb"),
            &subagent_rollout("01a0bbbb", "01a0aaaa", &[], (500, 100, 9)),
        );

        assert_eq!(codex_usage(&child), None);
    }

    /// A session that crossed midnight spawns into the next day's directory,
    /// so the lookup goes on past the parent's own — and no further back than
    /// it, which is what keeps the walk off the rest of the tree.
    #[test]
    fn a_codex_subagent_is_looked_for_from_the_parents_day_onwards() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(
            &dir,
            &rollout_path("2026/08/29", "01a0aaaa"),
            &rollout("01a0aaaa", &["01a0bbbb", "01a0cccc"], (1000, 800, 20)),
        );
        write(
            &dir,
            &rollout_path("2026/09/01", "01a0bbbb"),
            &subagent_rollout("01a0bbbb", "01a0aaaa", &[], (500, 100, 9)),
        );
        // A thread of the day before cannot be one this session spawned, and
        // is never opened.
        write(
            &dir,
            &rollout_path("2026/08/28", "01a0cccc"),
            &subagent_rollout("01a0cccc", "01a0aaaa", &[], (70, 30, 4)),
        );

        assert_eq!(
            codex_usage(&path),
            Some(TokenUsage {
                input_tokens: 1000 + 500,
                cached_input_tokens: 800 + 100,
                output_tokens: 20 + 9,
            })
        );
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

    /// `~/.codex/sessions/2026/08/29/rollout-*.jsonl`: the `codex exec` that
    /// spawned one sub-agent through the `collaboration` tools and waited for
    /// it.
    const CODEX_PARENT: &str = include_str!("fixtures/codex-parent-rollout.jsonl");

    /// The rollout that sub-agent wrote, filed under the same day beside its
    /// parent's.
    const CODEX_CHILD: &str = include_str!("fixtures/codex-child-rollout.jsonl");

    /// The names codex gave the two, kept because the child is found by the
    /// thread id its own file name ends with.
    const CODEX_PARENT_FILE: &str =
        "2026/08/29/rollout-2026-08-29T03-26-49-01a04b20-646d-71c2-b14f-4d98e40ae172.jsonl";
    const CODEX_CHILD_FILE: &str =
        "2026/08/29/rollout-2026-08-29T03-26-53-01a04b20-766c-7213-83a2-332002c3af62.jsonl";

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

    /// The spawn capture, laid out the way codex laid it out.
    fn codex_spawn_capture(dir: &TempDir) -> std::path::PathBuf {
        let parent = write(dir, CODEX_PARENT_FILE, CODEX_PARENT);
        write(dir, CODEX_CHILD_FILE, CODEX_CHILD);
        parent
    }

    /// What codex itself reported for the two processes of the spawn capture.
    ///
    /// Each is a thread with a rollout and a running total of its own, and the
    /// last `total_token_usage` of each is its whole spend:
    ///
    /// ```text
    /// input 45845, cached 40192, output 122   (the parent, /root)
    /// input 30645, cached 22016, output 170   (the sub-agent, /root/read_notes)
    /// ```
    ///
    /// The `codex exec` printed a `tokens used` of 5775 as it exited, which is
    /// the parent's line and nothing of the child's — the sub-agent's 8799 is
    /// what was silently missing from the task before this reader followed it.
    #[test]
    fn codex_reports_what_the_parent_and_its_subagent_reported() {
        let dir = TempDir::new().expect("a temp dir");
        let usage = codex_usage(&codex_spawn_capture(&dir)).expect("the capture reports usage");

        assert_eq!(
            usage,
            TokenUsage {
                input_tokens: 45845 + 30645,
                cached_input_tokens: 40192 + 22016,
                output_tokens: 122 + 170,
            }
        );
        assert_eq!(
            usage.input_tokens - usage.cached_input_tokens + usage.output_tokens,
            5775 + 8799,
        );
    }

    /// The same parent without its sub-agent's rollout beside it is the
    /// parent alone — the `tokens used` its own process printed.
    #[test]
    fn a_codex_rollout_without_its_subagents_is_the_parent_alone() {
        let dir = TempDir::new().expect("a temp dir");
        let path = write(&dir, CODEX_PARENT_FILE, CODEX_PARENT);
        let usage = codex_usage(&path).expect("the capture reports usage");

        assert_eq!(
            usage,
            TokenUsage {
                input_tokens: 45845,
                cached_input_tokens: 40192,
                output_tokens: 122,
            }
        );
        assert_eq!(
            usage.input_tokens - usage.cached_input_tokens + usage.output_tokens,
            5775,
        );
    }

    /// The capture names its one sub-agent twice — `kind` `started` when the
    /// spawn returns and `completed` when the wait does — and the child's
    /// 30645 is in the answer once.
    #[test]
    fn the_subagent_the_capture_names_twice_counts_once() {
        let named = CODEX_PARENT
            .lines()
            .filter(|line| line.contains("SubAgentActivity"))
            .count();
        assert_eq!(named, 2, "the capture no longer covers the repeated case");

        let dir = TempDir::new().expect("a temp dir");
        let usage = codex_usage(&codex_spawn_capture(&dir)).expect("the capture reports usage");
        assert_eq!(usage.input_tokens, 45845 + 30645);
        assert_ne!(usage.input_tokens, 45845 + 30645 + 30645);
    }

    /// A sub-agent's rollout opens with two `session_meta` lines — its own,
    /// then the parent's, at the head of the history it was forked with — so
    /// the thread a rollout *is* has to be read off the first. Taken off the
    /// last, the child would call itself the parent, and the parent would be
    /// a thread already seen and never read.
    #[test]
    fn a_codex_subagent_rollout_is_its_own_thread_not_its_parents() {
        let metas: Vec<serde_json::Value> = CODEX_CHILD
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|line| line["type"] == "session_meta")
            .collect();
        assert_eq!(metas.len(), 2);
        assert_eq!(
            metas[0]["payload"]["id"],
            "01a04b20-766c-7213-83a2-332002c3af62"
        );
        assert_eq!(metas[0]["payload"]["thread_source"], "subagent");
        assert_eq!(
            metas[0]["payload"]["parent_thread_id"],
            "01a04b20-646d-71c2-b14f-4d98e40ae172"
        );
        assert_eq!(
            metas[1]["payload"]["id"],
            "01a04b20-646d-71c2-b14f-4d98e40ae172"
        );

        // Both files read, both figures in: the parent was not skipped as a
        // thread the child claimed to be.
        let dir = TempDir::new().expect("a temp dir");
        assert_eq!(
            codex_usage(&codex_spawn_capture(&dir)).map(|usage| usage.input_tokens),
            Some(45845 + 30645)
        );
    }

    /// And the sub-agent's own rollout, handed over as its own hooks hand it
    /// over, reports nothing — the 30645 is already inside the parent's
    /// answer, and a second report of it would reach the daemon as another
    /// `source` of the same session.
    #[test]
    fn the_captured_subagent_rollout_reports_nothing_of_its_own() {
        let dir = TempDir::new().expect("a temp dir");
        codex_spawn_capture(&dir);
        assert_eq!(codex_usage(&dir.path().join(CODEX_CHILD_FILE)), None);
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
