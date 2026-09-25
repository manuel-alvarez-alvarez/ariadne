//! The session listing: `GET /v1/sessions` pages the sessions Ariadne runs
//! and the conversations the ACP agents stored themselves, filtered, newest
//! activity first — and follows an agent's own pages when it takes the
//! snapshot of the outside half.

use crate::common;

use std::time::Duration;

use ariadne_core::{AttentionReason, Seat, SessionStatus};
use ariadne_daemon::timeouts::Timeouts;

use axum::http::StatusCode;
use chrono::{DateTime, SecondsFormat, TimeDelta, Timelike, Utc};
use serde_json::{Value, json};

use ariadne_api::sessions::{SessionEntryDto, SessionKind, SessionPageDto};
use ariadne_store::NewSession;

use common::acp::{StubAcpAgent, discovery_accepted, script, stub_acp_agent};
use common::{Harness, get, harness, post_json};

/// A home whose `config.toml` registers one ACP agent per `(id, bin)`.
pub(crate) fn home_with_agents(agents: &[(&str, &str)]) -> std::path::PathBuf {
    let home = std::path::Path::new(agents[0].1)
        .parent()
        .unwrap()
        .join("home");
    std::fs::create_dir_all(&home).unwrap();
    let entries: String = agents
        .iter()
        .map(|(id, bin)| format!("[[acp_agents]]\nid = {id:?}\ncommand = [{bin:?}]\n"))
        .collect();
    std::fs::write(home.join("config.toml"), entries).unwrap();
    home
}

/// A moment `hours` before `now`, RFC 3339. The listing holds the last seven
/// days by default, so a fixture dates itself against the clock the test
/// holds rather than against a year somebody wrote down.
pub(crate) fn hours_before(now: DateTime<Utc>, hours: i64) -> String {
    (now - TimeDelta::hours(hours)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Five stored sessions, written oldest first so the listing's order is the
/// daemon's and not the agent's: two under `/work/alpha`, one beside it in
/// `/work/alphabet`, one in `/work/beta`, one elsewhere. `s1` was active five
/// hours before `now`, and every one after it an hour later.
fn five_sessions(now: DateTime<Utc>) -> Value {
    json!([
        {"sessionId": "s1", "cwd": "/work/alpha", "title": "Fix the flaky test",
         "updatedAt": hours_before(now, 5)},
        {"sessionId": "s2", "cwd": "/work/alpha/sub", "title": "Add paging",
         "updatedAt": hours_before(now, 4)},
        {"sessionId": "s3", "cwd": "/work/beta", "title": "fix the login",
         "updatedAt": hours_before(now, 3)},
        {"sessionId": "s4", "cwd": "/work/alphabet", "title": "Rename the module",
         "updatedAt": hours_before(now, 2)},
        {"sessionId": "s5", "cwd": "/other", "title": "Write the docs",
         "updatedAt": hours_before(now, 1)},
    ])
}

/// A stub that lists `sessions` — whole, or `page_size` at a time — and can
/// load any of them.
pub(crate) fn listing_script(sessions: Value, page_size: Option<usize>) -> Value {
    let mut setup = script();
    setup["capabilities"] = json!({"loadSession": true, "sessionCapabilities": {"list": {}}});
    setup["stored_sessions"] = Value::Array(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["sessionId"].clone())
            .collect(),
    );
    setup["session_list"] = sessions;
    if let Some(size) = page_size {
        setup["session_page_size"] = json!(size);
    }
    setup
}

async fn harness_with(stub: &StubAcpAgent) -> Harness {
    harness_waiting(stub, Timeouts::default()).await
}

/// [`harness_with`], waiting on the agent as long as `timeouts` says.
async fn harness_waiting(stub: &StubAcpAgent, timeouts: Timeouts) -> Harness {
    let h = harness()
        .home(home_with_agents(&[("test-agent", &stub.bin)]))
        .discover_agents()
        .timeouts(timeouts)
        .await;
    // A probe that times out under load leaves the agent unlisted, and the
    // snapshot asks only the agents discovery accepted.
    discovery_accepted(&h, stub, "test-agent").await;
    h
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

fn titles(page: &SessionPageDto) -> Vec<&str> {
    page.sessions
        .iter()
        .map(|session| session.title.as_deref().unwrap_or(""))
        .collect()
}

fn kinds(page: &SessionPageDto) -> Vec<SessionKind> {
    page.sessions.iter().map(|session| session.kind).collect()
}

/// The one row of a page that answers a single session.
fn only(page: &SessionPageDto) -> &SessionEntryDto {
    assert_eq!(page.total, 1, "{:?}", ids(page));
    &page.sessions[0]
}

/// A directory named after a title: no space in it, since a path with one
/// cannot go in a URI.
fn slug(title: &str) -> String {
    title.replace(' ', "-")
}

/// One Ariadne session of its own, on a goal titled `title` and a model of
/// `agent`: the row the listing reads a goal, an agent, a title and a
/// directory off. Its worktree is `slug(title)` under the harness.
async fn ariadne_session(h: &Harness, title: &str, agent: &str) -> ariadne_store::AgentSession {
    let repo = h.repository(&h.at(&format!("repo-{}", slug(title)))).await;
    let goal = h
        .store
        .create_goal(ariadne_store::NewGoal {
            title: title.into(),
            description: "desc".into(),
            repository_ids: vec![repo.id.clone()],
            pin: common::test_pin(),
        })
        .await
        .unwrap();
    h.store
        .create_session(NewSession {
            goal_id: Some(goal.id.clone()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: format!("{agent}:test-model"),
            effort: None,
            worktree_path: Some(h.at(&slug(title)).display().to_string()),
        })
        .await
        .unwrap()
}

/// An agent that answers its list two sessions at a time, over three pages,
/// still has every one of its sessions in the listing: the daemon follows
/// each `nextCursor` it is given.
#[tokio::test]
async fn a_paging_agents_every_session_is_in_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(
        dir.path(),
        listing_script(five_sessions(Utc::now()), Some(2)),
    );
    let h = harness_with(&stub).await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["s5", "s4", "s3", "s2", "s1"]);
    let cursors: Vec<Value> = stub
        .calls_of("session/list")
        .iter()
        .map(|call| call.get("cursor").cloned().unwrap_or(Value::Null))
        .collect();
    assert_eq!(cursors, [Value::Null, json!("2"), json!("4")]);
}

/// An agent that answers its first page and never the second is listed with
/// what arrived: the budget ends the listing, and the pages inside it are
/// kept rather than dropped with the timeout. The budget is shortened from a
/// daemon's 5 s, but not to [`common::RUNS_OUT`]: the first page still has to
/// arrive inside it, from an agent process the listing starts.
#[tokio::test]
async fn the_pages_that_arrived_are_kept_when_an_agents_budget_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut setup = listing_script(five_sessions(Utc::now()), Some(2));
    setup["session_list_stall_from"] = json!(2);
    let stub = stub_acp_agent(dir.path(), setup);
    let h = harness_waiting(
        &stub,
        Timeouts {
            probe: Duration::from_secs(2),
            ..Timeouts::default()
        },
    )
    .await;

    let page = listing(&h, "").await;

    assert_eq!(ids(&page), ["s2", "s1"]);
    assert_eq!(page.total, 2);
}

/// A page holds both kinds at once, ordered by last activity, newest first:
/// a session Ariadne started a moment ago leads the conversations the agent
/// stored hours before it.
#[tokio::test]
async fn a_page_holds_both_kinds_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;
    let session = ariadne_session(&h, "Ship the UI", "stub").await;

    let page = listing(&h, "").await;

    assert_eq!(
        ids(&page),
        [session.id.as_str(), "s5", "s4", "s3", "s2", "s1"]
    );
    assert_eq!(page.sessions[0].kind, SessionKind::Ariadne);
    assert_eq!(page.sessions[0].title.as_deref(), Some("Ship the UI"));
    assert_eq!(kinds(&page)[1..], [SessionKind::Outside; 5]);
}

/// `kind` narrows the page to one of the two.
#[tokio::test]
async fn kind_narrows_the_page_to_one_of_the_two() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;
    let session = ariadne_session(&h, "Ship the UI", "stub").await;

    let ariadne = listing(&h, "kind=ariadne").await;
    assert_eq!(ids(&ariadne), [session.id.as_str()]);

    let outside = listing(&h, "kind=outside").await;
    assert_eq!(ids(&outside), ["s5", "s4", "s3", "s2", "s1"]);
    assert_eq!(outside.total, 5);
}

/// An outside row carries the conversation and nothing of Ariadne's: the
/// agent, the id it loads back by, the directory it ran in, when it last ran
/// and its first prompt as its title — and no goal, task, seat or status.
#[tokio::test]
async fn an_outside_row_carries_no_goal_task_seat_or_status() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(now), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "limit=1").await;

    let row = &page.sessions[0];
    assert_eq!(row.kind, SessionKind::Outside);
    assert_eq!(row.agent_id, "test-agent");
    assert_eq!(row.id, "s5");
    assert_eq!(row.internal_session_id.as_deref(), Some("s5"));
    assert_eq!(row.working_directory.as_deref(), Some("/other"));
    assert_eq!(row.title.as_deref(), Some("Write the docs"));
    assert_eq!(row.last_activity_at, Some(hours_before(now, 1)));
    assert_eq!(row.goal_id, None);
    assert_eq!(row.task_id, None);
    assert_eq!(row.seat, None);
    assert_eq!(row.status, None);
}

/// A cursor cut from one snapshot continues from the same row after a
/// refresh: it names the sort key of the last row, not an offset into the
/// snapshot it came from. A session newer than every row of the first page
/// appears at the agent in between, so an offset would slip by one and
/// answer `s4` again.
#[tokio::test]
async fn a_cursor_continues_from_the_same_row_after_a_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(now), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "limit=2").await;
    assert_eq!(ids(&first), ["s5", "s4"]);
    let cursor = first.next_cursor.clone().expect("a second page");

    let mut grown = five_sessions(now);
    grown.as_array_mut().unwrap().push(json!({
        "sessionId": "s6", "cwd": "/work/gamma", "title": "Newest of all",
        "updatedAt": hours_before(now, 0),
    }));
    stub.reprogram(listing_script(grown, None));
    let second = listing(&h, &format!("limit=2&refresh=true&cursor={cursor}")).await;

    assert_ne!(second.snapshot_at, first.snapshot_at);
    assert_eq!(stub.calls_of("session/list").len(), 2);
    assert_eq!(second.total, 6);
    assert_eq!(ids(&second), ["s3", "s2"]);
}

/// Five sessions at `limit=2` are three pages through `next_cursor`, newest
/// first, each saying `total=5`, the last with no cursor.
#[tokio::test]
async fn five_sessions_at_limit_two_are_three_pages_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "limit=2").await;
    assert_eq!(ids(&first), ["s5", "s4"]);
    assert_eq!(first.total, 5);
    let cursor = first.next_cursor.clone().expect("a second page");

    let second = listing(&h, &format!("limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&second), ["s3", "s2"]);
    assert_eq!(second.total, 5);
    let cursor = second.next_cursor.clone().expect("a third page");

    let third = listing(&h, &format!("limit=2&cursor={cursor}")).await;
    assert_eq!(ids(&third), ["s1"]);
    assert_eq!(third.total, 5);
    assert_eq!(third.next_cursor, None);
}

/// A cursor keeps a moment finer than a millisecond over a refresh: the next
/// page goes on at the row after the cursor's, even where that row shares its
/// millisecond and is only microseconds older. A cursor written to the
/// millisecond would read as the start of that millisecond and skip it.
#[tokio::test]
async fn a_cursor_keeps_a_moment_finer_than_a_millisecond() {
    let dir = tempfile::tempdir().unwrap();
    // On the second, so the two microsecond offsets below share a
    // millisecond whatever the clock reads.
    let base = Utc::now().with_nanosecond(0).unwrap() - TimeDelta::hours(1);
    let at = |micros: i64| {
        (base + TimeDelta::microseconds(micros)).to_rfc3339_opts(SecondsFormat::Micros, true)
    };
    let sessions = json!([
        {"sessionId": "newest", "cwd": "/work", "title": "Newest", "updatedAt": at(1500)},
        {"sessionId": "inside", "cwd": "/work", "title": "Inside", "updatedAt": at(1200)},
        {"sessionId": "oldest", "cwd": "/work", "title": "Oldest", "updatedAt": at(0)},
    ]);
    let stub = stub_acp_agent(dir.path(), listing_script(sessions, None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "limit=1").await;
    assert_eq!(ids(&first), ["newest"]);
    let cursor = first.next_cursor.clone().expect("a second page");

    let second = listing(&h, &format!("limit=1&refresh=true&cursor={cursor}")).await;

    assert_ne!(second.snapshot_at, first.snapshot_at);
    assert_eq!(ids(&second), ["inside"]);
}

/// `agent` keeps the sessions of that registry agent alone, of either kind.
#[tokio::test]
async fn agent_narrows_the_listing_to_one_agents_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (one_dir, other_dir) = (dir.path().join("one"), dir.path().join("other"));
    std::fs::create_dir_all(&one_dir).unwrap();
    std::fs::create_dir_all(&other_dir).unwrap();
    let now = Utc::now();
    let one = stub_acp_agent(&one_dir, listing_script(five_sessions(now), None));
    let other_sessions = json!([{"sessionId": "o1", "cwd": "/work/other", "title": "Other",
                                 "updatedAt": hours_before(now, 6)}]);
    let other = stub_acp_agent(&other_dir, listing_script(other_sessions, None));
    let h = harness()
        .home(home_with_agents(&[
            ("test-agent", &one.bin),
            ("other-agent", &other.bin),
        ]))
        .discover_agents()
        .await;
    discovery_accepted(&h, &one, "test-agent").await;
    discovery_accepted(&h, &other, "other-agent").await;
    let session = ariadne_session(&h, "Ship the UI", "other-agent").await;

    let all = listing(&h, "").await;
    assert_eq!(all.total, 7);
    let page = listing(&h, "agent=other-agent").await;

    assert_eq!(ids(&page), [session.id.as_str(), "o1"]);
    assert_eq!(page.total, 2);
}

/// `dir` keeps the sessions whose directory is the path or a directory under
/// it — not a sibling that merely shares its prefix.
#[tokio::test]
async fn dir_narrows_the_listing_to_a_path_and_what_is_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, "dir=/work/alpha").await;

    assert_eq!(ids(&page), ["s2", "s1"]);
    assert_eq!(page.total, 2);
}

/// `dir` reads an Ariadne session's worktree the same way.
#[tokio::test]
async fn dir_narrows_the_listing_by_an_ariadne_sessions_worktree() {
    let h = harness().await;
    let session = ariadne_session(&h, "Ship the UI", "stub").await;
    ariadne_session(&h, "Ship the API", "stub").await;

    let page = listing(&h, &format!("dir={}", h.at(&slug("Ship the UI")).display())).await;

    assert_eq!(ids(&page), [session.id.as_str()]);
}

/// `since` keeps the sessions last active at or after the moment.
#[tokio::test]
async fn since_narrows_the_listing_to_activity_at_or_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(now), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, &format!("since={}", hours_before(now, 3))).await;

    assert_eq!(ids(&page), ["s5", "s4", "s3"]);
}

/// `until` keeps the sessions last active at or before the moment.
#[tokio::test]
async fn until_narrows_the_listing_to_activity_at_or_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(now), None));
    let h = harness_with(&stub).await;

    let page = listing(&h, &format!("until={}", hours_before(now, 4))).await;

    assert_eq!(ids(&page), ["s2", "s1"]);
}

/// `q` keeps the sessions whose title contains the text, whatever the case of
/// either: an outside conversation's first prompt, and the title of the work
/// behind an Ariadne session.
#[tokio::test]
async fn q_narrows_the_listing_by_title_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;
    let session = ariadne_session(&h, "Fix the landing", "stub").await;
    ariadne_session(&h, "Ship the UI", "stub").await;

    let page = listing(&h, "q=FIX").await;

    assert_eq!(ids(&page), [session.id.as_str(), "s3", "s1"]);
    assert_eq!(
        titles(&page),
        ["Fix the landing", "fix the login", "Fix the flaky test"]
    );
}

/// `goal` keeps the sessions of one goal, and leaves out every conversation
/// that belongs to no goal at all.
#[tokio::test]
async fn goal_narrows_the_listing_to_one_goals_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;
    let cast = h.cast().await;
    let session = h.orchestrator_session(&cast.goal).await;
    ariadne_session(&h, "Ship the API", "stub").await;

    let page = listing(&h, &format!("goal={}", cast.goal.id)).await;

    assert_eq!(ids(&page), [session.id.as_str()]);
}

/// `task` keeps the sessions of one task.
#[tokio::test]
async fn task_narrows_the_listing_to_one_tasks_sessions() {
    let h = harness().await;
    let cast = h.cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.orchestrator_session(&cast.goal).await;

    let page = listing(&h, &format!("task={}", cast.task.id)).await;

    assert_eq!(ids(&page), [author.id.as_str()]);
    assert_eq!(only(&page).title.as_deref(), Some(cast.task.title.as_str()));
}

/// `seat` keeps the sessions of one seat.
#[tokio::test]
async fn seat_narrows_the_listing_to_one_seat() {
    let h = harness().await;
    let cast = h.cast().await;
    let author = h
        .session(&cast.goal, Some(&cast.task), Seat::Author, &cast.author.id)
        .await;
    h.orchestrator_session(&cast.goal).await;

    let page = listing(&h, "seat=author").await;

    assert_eq!(ids(&page), [author.id.as_str()]);
    assert_eq!(only(&page).seat, Some(Seat::Author));
}

/// `status` keeps the sessions in that status, ended ones included: a named
/// status is the live-only default made precisely.
#[tokio::test]
async fn status_narrows_the_listing_to_one_status() {
    let h = harness().await;
    let live = ariadne_session(&h, "Ship the UI", "stub").await;
    let ended = ariadne_session(&h, "Ship the API", "stub").await;
    h.set_status(&ended, SessionStatus::Exited).await;

    let page = listing(&h, "status=exited").await;
    assert_eq!(ids(&page), [ended.id.as_str()]);
    assert_eq!(only(&page).status, Some(SessionStatus::Exited));

    let live_page = listing(&h, "status=starting").await;
    assert_eq!(ids(&live_page), [live.id.as_str()]);
}

/// `attention` keeps the sessions somebody is waiting on.
#[tokio::test]
async fn attention_narrows_the_listing_to_the_sessions_waiting_on_somebody() {
    let h = harness().await;
    let waiting = ariadne_session(&h, "Ship the UI", "stub").await;
    ariadne_session(&h, "Ship the API", "stub").await;
    h.raise(&waiting, AttentionReason::WaitingInput).await;

    let page = listing(&h, "attention=true").await;

    assert_eq!(ids(&page), [waiting.id.as_str()]);
    assert_eq!(
        only(&page).attention_reason,
        Some(AttentionReason::WaitingInput)
    );
}

/// The default page holds the last seven days alone, and `all` widens it to
/// every session of either kind.
#[tokio::test]
async fn the_default_page_holds_the_last_seven_days_and_all_widens_it() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let mut sessions = five_sessions(now);
    sessions.as_array_mut().unwrap().push(json!({
        "sessionId": "old", "cwd": "/work/old", "title": "Last month",
        "updatedAt": hours_before(now, 24 * 30),
    }));
    let stub = stub_acp_agent(dir.path(), listing_script(sessions, None));
    let h = harness_with(&stub).await;
    let idle = ariadne_session(&h, "Ship the UI", "stub").await;
    h.idle_for(&idle, 8 * 24 * 60 * 60).await;

    let page = listing(&h, "").await;
    assert_eq!(ids(&page), ["s5", "s4", "s3", "s2", "s1"]);
    assert_eq!(page.total, 5);

    let all = listing(&h, "all=true").await;

    assert_eq!(
        ids(&all),
        ["s5", "s4", "s3", "s2", "s1", idle.id.as_str(), "old"]
    );
}

/// `all` lists the Ariadne sessions that have ended, which the live-only
/// default holds back.
#[tokio::test]
async fn all_lists_an_ended_ariadne_session() {
    let h = harness().await;
    let live = ariadne_session(&h, "Ship the UI", "stub").await;
    let ended = ariadne_session(&h, "Ship the API", "stub").await;
    h.set_status(&ended, SessionStatus::Exited).await;

    let page = listing(&h, "").await;
    assert_eq!(ids(&page), [live.id.as_str()]);

    let all = listing(&h, "all=true").await;

    assert_eq!(all.total, 2);
    assert!(ids(&all).contains(&ended.id.as_str()));
}

/// The snapshot serves a second request inside a minute without asking any
/// agent again; `refresh=true` asks them at once.
#[tokio::test]
async fn a_second_request_asks_no_agent_again_but_a_refresh_does() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;

    let first = listing(&h, "").await;
    assert_eq!(stub.calls_of("session/list").len(), 1);

    let second = listing(&h, "limit=2").await;
    assert_eq!(stub.calls_of("session/list").len(), 1);
    assert_eq!(second.snapshot_at, first.snapshot_at);

    listing(&h, "refresh=true").await;
    assert_eq!(stub.calls_of("session/list").len(), 2);
}

/// A page no outside session can be in — one kind of Ariadne's, or narrowed
/// by a goal, task, status, seat or attention — asks no agent for one.
#[tokio::test]
async fn a_page_with_no_room_for_an_outside_session_asks_no_agent() {
    let dir = tempfile::tempdir().unwrap();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(Utc::now()), None));
    let h = harness_with(&stub).await;
    let session = ariadne_session(&h, "Ship the UI", "stub").await;

    let ariadne = listing(&h, "kind=ariadne&refresh=true").await;
    assert_eq!(ids(&ariadne), [session.id.as_str()]);
    let goal = session.goal_id.clone().expect("the session has a goal");
    for query in [
        format!("goal={goal}"),
        "status=running".to_owned(),
        "seat=author".to_owned(),
        "attention=true".to_owned(),
    ] {
        listing(&h, &query).await;
    }
    assert!(stub.calls_of("session/list").is_empty());
}

/// A resumed conversation is in the page once, as the Ariadne session it
/// became: what a row already holds is subtracted from the outside half at
/// query time, without a new snapshot.
#[tokio::test]
async fn a_resumed_outside_session_is_listed_once_as_an_ariadne_session() {
    let dir = tempfile::tempdir().unwrap();
    let now = Utc::now();
    let stub = stub_acp_agent(dir.path(), listing_script(five_sessions(now), None));
    let h = harness_with(&stub).await;
    let mut setup = listing_script(five_sessions(now), None);
    setup["session_list"][2]["cwd"] = json!(dir.path().to_str().unwrap());
    stub.reprogram(setup);

    let before = listing(&h, "").await;
    assert!(ids(&before).contains(&"s3"));

    let resumed: Value = h
        .json(
            post_json(
                "/v1/outside-sessions/resume",
                json!({
                    "agent_id": "test-agent",
                    "internal_session_id": "s3",
                }),
            ),
            StatusCode::OK,
        )
        .await;
    stub.clear_messages();

    let after = listing(&h, "").await;

    let of_s3: Vec<&SessionEntryDto> = after
        .sessions
        .iter()
        .filter(|session| session.internal_session_id.as_deref() == Some("s3"))
        .collect();
    assert_eq!(of_s3.len(), 1, "{:?}", ids(&after));
    assert_eq!(of_s3[0].kind, SessionKind::Ariadne);
    assert_eq!(of_s3[0].id, resumed["id"].as_str().unwrap());
    // It keeps the first prompt it was listed with as its title.
    assert_eq!(of_s3[0].title.as_deref(), Some("fix the login"));
    let found = listing(&h, "kind=ariadne&q=LOGIN").await;
    assert_eq!(ids(&found), [of_s3[0].id.as_str()]);
    assert_eq!(after.total, 5);
    assert_eq!(after.snapshot_at, before.snapshot_at);
    assert!(stub.calls_of("session/list").is_empty());
}

/// A cursor the daemon did not write is refused as `invalid_cursor`.
#[tokio::test]
async fn an_unreadable_cursor_is_refused() {
    let h = harness().await;

    let error = h
        .error(
            get("/v1/sessions?cursor=not-a-cursor"),
            StatusCode::BAD_REQUEST,
        )
        .await;

    assert_eq!(error.error.code, "invalid_cursor");
}

/// The query parameters and the page DTO are in the OpenAPI document, and
/// the outside-session listing it replaces is gone from it.
#[tokio::test]
async fn the_query_and_the_page_are_in_the_openapi_document() {
    let h = harness().await;

    let doc: Value = h.get("/api-docs/openapi.json").await;

    let operation = &doc["paths"]["/v1/sessions"]["get"];
    let mut names: Vec<&str> = operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parameter| parameter["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "agent",
            "all",
            "attention",
            "cursor",
            "dir",
            "goal",
            "kind",
            "limit",
            "q",
            "refresh",
            "seat",
            "since",
            "status",
            "task",
            "until"
        ]
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/SessionPageDto"
    );
    let page = &doc["components"]["schemas"]["SessionPageDto"];
    let mut fields: Vec<&str> = page["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, ["next_cursor", "sessions", "snapshot_at", "total"]);
    assert!(doc["paths"]["/v1/outside-sessions"].is_null());
}
