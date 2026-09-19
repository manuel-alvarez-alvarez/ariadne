//! Integration tests for token usage read from the agent's own transcript:
//! a Codex rollout or a Claude Code transcript, written as fixtures under
//! the harness's transcript homes, for the stub's ACP session id.
//!
//! The prompt responses of these stubs report other figures, so a test that
//! reads the transcript's figures proves the response was not reported.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::common;
use ariadne_api::sessions::SessionDto;
use ariadne_api::usage::TokenUsageDto;
use ariadne_core::SessionStatus;
use ariadne_daemon::timeouts::Timeouts;
use common::acp::{discovery_settled, registry_home, script, stub_acp_agent};
use common::{Cast, Harness, TIMEOUT, eventually, harness};
use serde_json::{Value, json};

fn tokens(input_tokens: u64, cached_input_tokens: u64, output_tokens: u64) -> TokenUsageDto {
    TokenUsageDto {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    }
}

/// A prompt response far below any transcript here: what codex-acp reports
/// for the last model request of a turn.
fn small_usage() -> Value {
    json!({"totalTokens": 14686, "inputTokens": 3545, "cachedReadTokens": 11136, "outputTokens": 5})
}

async fn cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    h.cast_pinned("stub:test-model", 1).await
}

async fn usage(h: &Harness, session_id: &str) -> TokenUsageDto {
    let session: SessionDto = h.get(&format!("/v1/sessions/{session_id}")).await;
    session.usage
}

fn append(path: &Path, lines: &[Value]) {
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

/// The stub session's rollout, where Codex writes it.
fn rollout(h: &Harness) -> PathBuf {
    h.transcript_homes()
        .codex
        .join("sessions/2026/09/19/rollout-2026-09-19T10-00-00-stub-session.jsonl")
}

/// A `token_count` line: the running total, and a last request that differs.
fn token_count(input: u64, cached: u64, output: u64) -> Value {
    json!({"timestamp": "2026-09-19T10:00:01Z", "type": "event_msg", "payload": {
        "type": "token_count",
        "info": {
            "total_token_usage": {"input_tokens": input, "cached_input_tokens": cached,
                "cache_write_input_tokens": 0, "output_tokens": output,
                "reasoning_output_tokens": 3, "total_tokens": input + output},
            "last_token_usage": {"input_tokens": 14681, "cached_input_tokens": 11136,
                "cache_write_input_tokens": 0, "output_tokens": 5,
                "reasoning_output_tokens": 0, "total_tokens": 14686},
        },
    }})
}

/// A script whose one turn reports `small_usage` and is held open on
/// `release`.
fn held_script(release: &Path) -> Value {
    let mut scripted = script();
    scripted["stored_sessions"] = json!(["stub-session"]);
    scripted["prompts"] = json!([{
        "updates": [],
        "usage": small_usage(),
        "wait_for": release.display().to_string(),
    }]);
    scripted
}

async fn held_open(release: &Path) {
    let reached = release.with_extension("reached");
    eventually(TIMEOUT, "the turn to be held open", || async {
        reached.exists()
    })
    .await;
}

#[tokio::test]
async fn a_codex_session_stores_its_rollouts_total_not_its_prompt_response() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["prompts"] = json!([{"updates": [], "usage": small_usage()}]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    append(
        &rollout(&h),
        &[
            json!({"type": "session_meta", "payload": {"id": "stub-session"}}),
            json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}}),
            token_count(57928, 50688, 228),
            token_count(72609, 61824, 233),
        ],
    );
    let cast = cast(&h).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(usage(&h, &session.id).await, tokens(72609, 61824, 233));
}

/// One Claude Code request as its transcript writes it, one line per
/// content block.
fn assistant(id: &str, input: u64, read: u64, created: u64, output: u64) -> Value {
    json!({"type": "assistant", "message": {"id": id, "role": "assistant", "usage": {
        "input_tokens": input, "cache_read_input_tokens": read,
        "cache_creation_input_tokens": created, "output_tokens": output,
    }}})
}

#[tokio::test]
async fn a_claude_session_counts_each_request_once_with_its_subagents() {
    let agent_dir = tempfile::tempdir().unwrap();
    let release = agent_dir.path().join("release");
    let stub = stub_acp_agent(agent_dir.path(), held_script(&release));
    let h = harness().home(registry_home(&stub)).await;
    let cast = cast(&h).await;
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    held_open(&release).await;

    // Claude Code's project directory: the worktree's real path with every
    // character but a letter or digit made a `-`.
    let worktree = h
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .worktree_path
        .unwrap();
    let real = std::fs::canonicalize(worktree).unwrap();
    let slug: String = real
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let project = h.transcript_homes().claude.join("projects").join(slug);
    let three_lines = assistant("msg_2", 1, 10000, 100, 60);
    append(
        &project.join("stub-session.jsonl"),
        &[
            json!({"type": "user", "message": {"role": "user", "content": "go"}}),
            assistant("msg_1", 3, 0, 10000, 50),
            three_lines.clone(),
            three_lines.clone(),
            three_lines,
            assistant("msg_3", 2, 30000, 50, 70),
            assistant("msg_4", 5, 25000, 20, 41),
        ],
    );
    append(
        &project.join("stub-session/subagents/agent-a1.jsonl"),
        &[assistant("msg_5", 8, 25232, 10, 70)],
    );
    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(usage(&h, &session.id).await, tokens(100431, 90232, 291));
}

#[tokio::test]
async fn a_running_turns_figure_moves_before_its_stop() {
    let agent_dir = tempfile::tempdir().unwrap();
    let release = agent_dir.path().join("release");
    let stub = stub_acp_agent(agent_dir.path(), held_script(&release));
    let h = harness()
        .home(registry_home(&stub))
        .timeouts(Timeouts {
            transcript_poll: Duration::from_millis(20),
            ..Timeouts::default()
        })
        .await;
    append(&rollout(&h), &[token_count(100, 50, 10)]);
    let cast = cast(&h).await;
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    held_open(&release).await;
    eventually(TIMEOUT, "the first total to be stored", || async {
        usage(&h, &session.id).await == tokens(100, 50, 10)
    })
    .await;

    append(&rollout(&h), &[token_count(500, 300, 40)]);

    eventually(TIMEOUT, "the second total to be stored", || async {
        usage(&h, &session.id).await == tokens(500, 300, 40)
    })
    .await;
    assert_eq!(h.session_status(&session).await, SessionStatus::Running);
    assert!(!release.exists(), "the turn is still held open");
}

#[tokio::test]
async fn two_launches_of_one_codex_session_add_up() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut first = script();
    first["stored_sessions"] = json!(["stub-session"]);
    first["prompts"] = json!([{"updates": [], "usage": small_usage()}]);
    let stub = stub_acp_agent(agent_dir.path(), first);
    let h = harness().home(registry_home(&stub)).discover_agents().await;
    discovery_settled(&h, &stub).await;
    append(&rollout(&h), &[token_count(1000, 800, 100)]);
    let cast = cast(&h).await;
    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the first launch's turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    assert_eq!(usage(&h, &session.id).await, tokens(1000, 800, 100));

    let release = agent_dir.path().join("release");
    stub.reprogram(held_script(&release));
    let resumed = h
        .launcher
        .resume_author(&cast.task.id, "here is your review")
        .await
        .unwrap();
    held_open(&release).await;
    // A resume starts the rollout's running total again.
    append(&rollout(&h), &[token_count(200, 150, 20)]);
    std::fs::write(&release, "go").unwrap();
    eventually(TIMEOUT, "the resumed turn to end", || async {
        stub.calls_of("session/prompt").len() == 2
            && h.session_status(&resumed).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(usage(&h, &resumed.id).await, tokens(1200, 950, 120));
}

#[tokio::test]
async fn a_session_without_a_transcript_keeps_its_prompt_responses_figure() {
    let agent_dir = tempfile::tempdir().unwrap();
    let mut scripted = script();
    scripted["prompts"] = json!([{"updates": [], "usage": small_usage()}]);
    let stub = stub_acp_agent(agent_dir.path(), scripted);
    let h = harness().home(registry_home(&stub)).await;
    let cast = cast(&h).await;

    let session = h.launcher.spawn_author(&cast.task.id).await.unwrap();
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;

    assert_eq!(usage(&h, &session.id).await, tokens(14681, 11136, 5));
}
