//! The conversations on disk: `GET /v1/sessions` lists a Claude transcript
//! that `session/list` never answered with, and the rows of the page it
//! answers carry the model and the tokens their transcripts hold.
//!
//! A transcript here is written by hand, under the harness's own Claude home,
//! so no test reads the conversations of the machine it runs on. The stub
//! agent is registered as `claude-acp`, which is the agent the disk reader
//! answers for.

use crate::common;
use crate::session_list::{home_with_agents, hours_before, listing_script};

use std::fs::FileTimes;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};

use ariadne_api::sessions::{SessionEntryDto, SessionKind, SessionPageDto};

use common::acp::{StubAcpAgent, discovery_accepted, stub_acp_agent};
use common::{Harness, harness};

/// The agent whose transcripts the disk reader answers for.
const CLAUDE: &str = "claude-acp";

/// A harness whose one registry agent is `stub`, under the id the disk reader
/// is registered by.
async fn harness_with(stub: &StubAcpAgent) -> Harness {
    let h = harness()
        .home(home_with_agents(&[(CLAUDE, &stub.bin)]))
        .discover_agents()
        .await;
    // The disk half is read for the agents discovery accepted, so a probe
    // that timed out under load is probed again first.
    discovery_accepted(&h, stub, CLAUDE).await;
    h
}

/// A stub that answers `session/list` with `sessions` and nothing else.
fn stub_listing(dir: &Path, sessions: Value) -> StubAcpAgent {
    stub_acp_agent(dir, listing_script(sessions, None))
}

/// One listing request, `query` being what follows the `?`.
async fn listing(h: &Harness, query: &str) -> SessionPageDto {
    h.get(&format!("/v1/sessions?{query}")).await
}

fn ids(page: &SessionPageDto) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.id.as_str())
        .collect()
}

/// The row of a page with that id.
fn row<'a>(page: &'a SessionPageDto, id: &str) -> &'a SessionEntryDto {
    page.sessions
        .iter()
        .find(|session| session.id == id)
        .unwrap_or_else(|| panic!("{id} is not in the page: {:?}", ids(page)))
}

/// One prompt of a person, as Claude Code writes it: the directory the
/// conversation runs in, and the text in a content block.
fn user(cwd: &str, text: &str) -> String {
    json!({"type": "user", "cwd": cwd,
           "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
    .to_string()
}

/// One model request, as Claude Code writes it: the model it ran on, the id
/// that identifies the request, and what it cost. Three-digit figures, so a
/// [`rewrite`] of the same line keeps the file's size.
fn assistant(id: &str, model: &str, input: u64, read: u64, created: u64, output: u64) -> String {
    json!({"type": "assistant", "message": {"id": id, "model": model, "usage": {
        "input_tokens": input, "cache_read_input_tokens": read,
        "cache_creation_input_tokens": created, "output_tokens": output,
    }}})
    .to_string()
}

/// A transcript of `id` under the project of `cwd`, last written at `at`.
fn transcript(h: &Harness, cwd: &str, id: &str, lines: &[String], at: DateTime<Utc>) -> PathBuf {
    let slug: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = h
        .transcript_homes()
        .claude
        .join("projects")
        .join(slug)
        .join(format!("{id}.jsonl"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body(lines)).unwrap();
    written_at(&path, at);
    path
}

fn body(lines: &[String]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// Write more lines at the end of a transcript, as a conversation that goes on
/// does.
fn append(path: &Path, lines: &[String]) {
    use std::io::Write;

    std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(body(lines).as_bytes())
        .unwrap();
}

/// Write `path`'s bytes again, keeping the size and the modification time a
/// reader knows the file by. A reader that answers out of what it read before
/// answers the old figures, and one that reads the file again answers the new.
fn rewrite(path: &Path, lines: &[String]) {
    let before = std::fs::metadata(path).unwrap();
    let body = body(lines);
    assert_eq!(
        body.len() as u64,
        before.len(),
        "a rewrite must keep the file's size"
    );
    std::fs::write(path, body).unwrap();
    set_times(path, before.modified().unwrap());
}

fn written_at(path: &Path, at: DateTime<Utc>) {
    let millis = u64::try_from(at.timestamp_millis()).expect("a moment after the epoch");
    set_times(path, SystemTime::UNIX_EPOCH + Duration::from_millis(millis));
}

fn set_times(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_accessed(at).set_modified(at))
        .unwrap();
}

/// A transcript `session/list` never answers with is in the listing all the
/// same, under the directory and the title its own file holds, and it carries
/// the model it ran and the tokens it spent. One model request is written over
/// as many lines as it has content blocks, so the request written twice here
/// is counted once.
#[tokio::test]
async fn a_transcript_the_agent_does_not_list_is_listed_with_its_model_and_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([{"sessionId": "listed", "cwd": "/work/listed", "title": "Listed by the agent",
                "updatedAt": hours_before(now, 2)}]),
    );
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/on-disk",
        "on-disk",
        &[
            user("/work/on-disk", "Read the config\nand say what it holds"),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, 100),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, 100),
            assistant("msg_2", "claude-opus-5", 200, 200, 200, 200),
        ],
        now - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["on-disk", "listed"]);
    let found = row(&page, "on-disk");
    assert_eq!(found.kind, SessionKind::Outside);
    assert_eq!(found.agent_id, CLAUDE);
    assert_eq!(found.internal_session_id.as_deref(), Some("on-disk"));
    assert_eq!(found.working_directory.as_deref(), Some("/work/on-disk"));
    assert_eq!(found.title.as_deref(), Some("Read the config"));
    assert_eq!(found.model.as_deref(), Some("claude-acp:claude-opus-5"));
    let usage = found.usage.expect("the transcript's tokens");
    assert_eq!(usage.input_tokens, 900);
    assert_eq!(usage.cached_input_tokens, 300);
    assert_eq!(usage.output_tokens, 300);
}

/// A conversation both sources hold is one row: the agent's own, under the
/// title it listed, with the figures of the file behind it.
#[tokio::test]
async fn a_conversation_both_sources_hold_is_one_row_under_the_agents_title() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([{"sessionId": "both", "cwd": "/work/both", "title": "Listed by the agent",
                "updatedAt": hours_before(now, 1)}]),
    );
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/both",
        "both",
        &[
            user("/work/both", "Written on disk"),
            assistant("msg_1", "claude-sonnet-5", 100, 100, 100, 100),
        ],
        now - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["both"]);
    let found = row(&page, "both");
    assert_eq!(found.title.as_deref(), Some("Listed by the agent"));
    assert_eq!(found.model.as_deref(), Some("claude-acp:claude-sonnet-5"));
    assert_eq!(found.usage.expect("the tokens").output_tokens, 100);
}

/// A transcript that holds no turn is a conversation nobody had, and it is not
/// listed.
#[tokio::test]
async fn a_transcript_with_no_turn_is_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/empty",
        "empty",
        &[
            json!({"type": "ai-title", "aiTitle": "Nothing happened", "sessionId": "empty"})
                .to_string(),
            json!({"type": "queue-operation", "operation": "enqueue"}).to_string(),
        ],
        Utc::now() - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), [] as [&str; 0]);
    assert_eq!(page.total, 0);
}

/// A row the agent answered with stays listed whatever the disk says: one
/// whose transcript holds no turn, and one with no file at all.
#[tokio::test]
async fn a_row_the_agent_lists_stays_listed_whatever_the_disk_says() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_listing(
        dir.path(),
        json!([
            {"sessionId": "no-turn", "cwd": "/work/no-turn", "title": "Opened and left",
             "updatedAt": hours_before(now, 1)},
            {"sessionId": "no-file", "cwd": "/work/no-file", "title": "No file of its own",
             "updatedAt": hours_before(now, 2)},
        ]),
    );
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/no-turn",
        "no-turn",
        &[json!({"type": "ai-title", "aiTitle": "Opened and left"}).to_string()],
        now - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["no-turn", "no-file"]);
    assert_eq!(
        row(&page, "no-turn").title.as_deref(),
        Some("Opened and left")
    );
}

/// A conversation goes by the title Claude Code gave it, and by the first line
/// of its first prompt where it gave none.
#[tokio::test]
async fn a_conversation_goes_by_the_title_claude_gave_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let now = Utc::now();
    transcript(
        &h,
        "/work/titled",
        "titled",
        &[
            json!({"type": "ai-title", "aiTitle": "Page the sessions"}).to_string(),
            user("/work/titled", "make the listing page\nplease"),
        ],
        now - TimeDelta::hours(1),
    );
    transcript(
        &h,
        "/work/untitled",
        "untitled",
        &[user("/work/untitled", "  fix the flaky test\nunder load")],
        now - TimeDelta::hours(2),
    );

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["titled", "untitled"]);
    assert_eq!(
        row(&page, "titled").title.as_deref(),
        Some("Page the sessions")
    );
    assert_eq!(
        row(&page, "untitled").title.as_deref(),
        Some("fix the flaky test")
    );
}

/// The page of the disk half keeps the order, the cursor and the window of the
/// listing: newest activity first, one page continuing at the next, and the
/// last seven days alone until `all` widens it.
#[tokio::test]
async fn the_disk_half_keeps_the_order_the_cursor_and_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let now = Utc::now();
    for (id, hours) in [("newest", 1), ("older", 3), ("last-month", 24 * 30)] {
        transcript(
            &h,
            &format!("/work/{id}"),
            id,
            &[
                user(&format!("/work/{id}"), id),
                assistant("msg_1", "claude-opus-5", 100, 100, 100, 100),
            ],
            now - TimeDelta::hours(hours),
        );
    }

    let page = listing(&h, "").await;
    assert_eq!(ids(&page), ["newest", "older"]);
    assert_eq!(page.total, 2);

    let first = listing(&h, "limit=1").await;
    assert_eq!(ids(&first), ["newest"]);
    let cursor = first.next_cursor.clone().expect("a second page");
    let second = listing(&h, &format!("limit=1&cursor={cursor}")).await;
    assert_eq!(ids(&second), ["older"]);

    let all = listing(&h, "all=true").await;
    assert_eq!(ids(&all), ["newest", "older", "last-month"]);
}

/// One line of a kind a listing passes over, `bytes` long: what a transcript
/// holds between the lines a listing is after.
fn filler(bytes: usize) -> String {
    let line = json!({"type": "attachment", "text": ""}).to_string();
    let text = "x".repeat(bytes.saturating_sub(line.len()));
    json!({"type": "attachment", "text": text}).to_string()
}

/// A conversation whose first turn lies far into its file is listed all the
/// same: a listing reads for a turn however deep it is, so no conversation is
/// lost to a reading budget.
#[tokio::test]
async fn a_transcript_whose_first_turn_lies_deep_in_the_file_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/deep",
        "deep",
        &[
            filler(512 * 1024),
            user("/work/deep", "Say what the attachment holds"),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, 100),
        ],
        Utc::now() - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    let found = row(&page, "deep");
    assert_eq!(found.working_directory.as_deref(), Some("/work/deep"));
    assert_eq!(
        found.title.as_deref(),
        Some("Say what the attachment holds")
    );
    assert_eq!(found.usage.expect("the tokens").output_tokens, 100);
}

/// The title a transcript records names its conversation however far into the
/// file it stands: a listing reads for it to the end of the file, and it takes
/// a megabyte of other lines here to prove that no budget cuts the reading
/// short.
#[tokio::test]
async fn a_title_a_megabyte_into_a_transcript_still_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    transcript(
        &h,
        "/work/named",
        "named",
        &[
            user("/work/named", "make the listing page"),
            filler(1024 * 1024 + 1),
            json!({"type": "ai-title", "aiTitle": "Page the sessions"}).to_string(),
        ],
        Utc::now() - TimeDelta::hours(1),
    );

    let page = listing(&h, "").await;

    assert_eq!(
        row(&page, "named").title.as_deref(),
        Some("Page the sessions")
    );
}

/// A conversation that wrote another turn after a page read its figures is
/// listed by what it holds now: the listing's read of a file and the whole of
/// it stand for one moment, so the next listing makes both again rather than
/// calling the older of them fresh.
#[tokio::test]
async fn a_transcript_that_grew_under_a_page_is_listed_by_what_it_holds_now() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let now = Utc::now();
    let path = transcript(
        &h,
        "/work/grown",
        "grown",
        &[
            user("/work/grown", "Count the tokens"),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, 100),
        ],
        now - TimeDelta::hours(3),
    );

    let first = listing(&h, "").await;
    assert_eq!(
        row(&first, "grown").last_activity_at.as_deref(),
        Some(hours_before(now, 3).as_str())
    );

    // The conversation answers once more, and a page reads its figures again.
    append(
        &path,
        &[
            user("/work/grown", "And again"),
            assistant("msg_2", "claude-opus-5", 100, 100, 100, 100),
        ],
    );
    written_at(&path, now - TimeDelta::hours(1));
    listing(&h, "").await;

    let after = listing(&h, "refresh=true").await;

    let grown = row(&after, "grown");
    assert_eq!(
        grown.last_activity_at.as_deref(),
        Some(hours_before(now, 1).as_str())
    );
    assert_eq!(grown.usage.expect("the tokens").output_tokens, 200);
}

/// A second listing over a transcript nothing has written to reads no file
/// again: the reader knows a file by its path, its size and its modification
/// time, and this one still stands where it did.
#[tokio::test]
async fn a_second_listing_reads_no_unchanged_transcript_again() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let lines = |output: u64| {
        vec![
            user("/work/cached", "Count the tokens"),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, output),
        ]
    };
    let path = transcript(
        &h,
        "/work/cached",
        "cached",
        &lines(100),
        Utc::now() - TimeDelta::hours(1),
    );

    let first = listing(&h, "").await;
    assert_eq!(
        row(&first, "cached")
            .usage
            .expect("the tokens")
            .output_tokens,
        100
    );
    rewrite(&path, &lines(200));

    let second = listing(&h, "").await;

    assert_eq!(
        row(&second, "cached")
            .usage
            .expect("the tokens")
            .output_tokens,
        100
    );
}

/// A page reads the transcripts of its own rows and no others: over 51
/// conversations, a page of 50 leaves the 51st file unread, which the
/// bytes written behind both pages' backs show — the 50 answer what was read
/// for them, and the one that was never read answers what the file holds now.
#[tokio::test]
async fn a_page_of_fifty_reads_the_transcripts_of_its_own_rows_only() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_listing(dir.path(), json!([]));
    let h = harness_with(&stub).await;
    let now = Utc::now();
    let lines = |id: &str, output: u64| {
        vec![
            user("/work/many", id),
            assistant("msg_1", "claude-opus-5", 100, 100, 100, output),
        ]
    };
    let written: Vec<String> = (0..51).map(|at| format!("s{at:02}")).collect();
    let paths: Vec<PathBuf> = written
        .iter()
        .enumerate()
        .map(|(at, id)| {
            transcript(
                &h,
                "/work/many",
                id,
                &lines(id, 100),
                now - TimeDelta::minutes(at as i64 + 1),
            )
        })
        .collect();

    let first = listing(&h, "limit=50").await;
    assert_eq!(first.total, 51);
    assert_eq!(first.sessions.len(), 50);
    assert_eq!(ids(&first).last(), Some(&"s49"));
    for (id, path) in written.iter().zip(&paths) {
        rewrite(path, &lines(id, 200));
    }

    let second = listing(&h, "limit=51").await;

    assert_eq!(second.sessions.len(), 51);
    let tokens = |id: &str| {
        row(&second, id)
            .usage
            .expect("the tokens of the row")
            .output_tokens
    };
    assert_eq!(tokens("s00"), 100);
    assert_eq!(tokens("s49"), 100);
    assert_eq!(tokens("s50"), 200);
}
