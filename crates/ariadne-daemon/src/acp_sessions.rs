//! The session listing: the sessions Ariadne runs and the conversations the
//! ACP agents store themselves, in one page.
//!
//! The Ariadne half is read off the store at request time. The outside half
//! is served off one in-memory snapshot of every agent's stored sessions
//! ([`OutsideSessions`]), which a resume validates against and refreshes once
//! on a miss. Both halves are ordered by last activity and cut into one page.
//!
//! The outside half has two sources, merged by `(agent, internal id)`: what
//! each agent answered over `session/list`, and what it left on disk
//! ([`crate::stored_conversations`]). ACP is the first of them — a row it
//! answered with is listed however the disk reads, except an OpenCode row
//! absent from a readable database. The disk also holds the model and tokens.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use ariadne_api::agents::AcpAgentStatus;
use ariadne_api::sessions::{OutsideSessionDto, SessionKind, SessionPageDto, SessionPageQuery};
use ariadne_core::Seat;
use ariadne_core::models::agent_of;
use ariadne_store::{AgentSession, SessionFilter, Store, TaskFilter};

use crate::acp_discovery::AgentRegistry;
use crate::http::convert::{outside_entry, session_entry_of};
use crate::stored_conversations::{DiskConversations, DiskListing, fill_page};
use crate::transcript::TranscriptHomes;

/// How old the snapshot may be before a request takes it again.
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(60);

/// How far back the listing looks when no bound of the caller's own says
/// otherwise: a session nobody has touched in a week is history.
const DEFAULT_WINDOW_DAYS: i64 = 7;

/// The `(agent, internal id)` of every session an Ariadne row already holds.
///
/// The `model` column of a session carries `<agent>:<model>`, so the agent an
/// already-resumed session belongs to is read back off it rather than kept
/// anywhere else.
async fn adopted(store: &Store) -> Result<HashSet<(String, String)>> {
    Ok(store
        .list_sessions(SessionFilter::default())
        .await?
        .into_iter()
        .filter_map(|session| {
            let agent_id = agent_of(&session.model).to_string();
            session
                .internal_session_id
                .map(|internal| (agent_id, internal))
        })
        .collect())
}

fn session_key(session: &OutsideSessionDto) -> (String, String) {
    (
        session.agent_id.clone(),
        session.internal_session_id.clone(),
    )
}

/// Every outside session of every agent at one moment, resumed ones
/// included: what a row already holds is subtracted when a page is cut, so a
/// resume shows without a new snapshot.
pub(crate) struct Snapshot {
    sessions: Vec<OutsideSessionDto>,
    /// The moment, for the reply.
    taken_at: DateTime<Utc>,
    /// The same moment, monotonic, for the age.
    taken: Instant,
    /// When the agents were first asked for it, for a `refresh` that came
    /// while it was being taken.
    started: Instant,
}

impl Snapshot {
    /// Find one session still outside Ariadne by its agent and internal id.
    pub(crate) async fn find(
        &self,
        store: &Store,
        agent_id: &str,
        internal_session_id: &str,
    ) -> Result<Option<OutsideSessionDto>> {
        let known = adopted(store).await?;
        Ok(self
            .sessions
            .iter()
            .find(|session| {
                session.agent_id == agent_id
                    && session.internal_session_id == internal_session_id
                    && !known.contains(&session_key(session))
            })
            .cloned())
    }
}

/// The daemon's one snapshot of the outside sessions. Nothing of it reaches
/// the disk.
#[derive(Clone)]
pub struct OutsideSessions {
    /// The snapshot as it stands. It is locked only to read or to replace it,
    /// never while one is taken, so a refresh holds up no reader.
    snapshot: Arc<std::sync::Mutex<Option<Arc<Snapshot>>>>,
    /// Held while a snapshot is taken: requests that find it stale together
    /// wait here for the one refresh.
    refreshing: Arc<Mutex<()>>,
    pub(crate) resumes: Arc<Mutex<()>>,
    /// The second source: the conversations the agents left on disk.
    readers: Arc<DiskConversations>,
}

impl OutsideSessions {
    /// The listing of a daemon whose agents keep their conversations where its
    /// own environment says.
    pub fn from_env() -> Self {
        Self::with_transcripts(&TranscriptHomes::from_env())
    }

    /// The same, reading the conversations under `homes` rather than where the
    /// environment puts them.
    pub fn with_transcripts(homes: &TranscriptHomes) -> Self {
        Self {
            snapshot: Arc::default(),
            refreshing: Arc::default(),
            resumes: Arc::default(),
            readers: Arc::new(DiskConversations::new(homes)),
        }
    }

    /// The readers a page reads its rows' model and tokens with.
    pub(crate) fn readers(&self) -> &Arc<DiskConversations> {
        &self.readers
    }

    /// Take the first snapshot in the background, for a daemon whose
    /// discovery has just run. The refresh is held before this returns, so a
    /// request that comes while it runs waits for it rather than taking a
    /// second one.
    pub async fn warm(&self, registry: &AgentRegistry) {
        let refreshing = self.refreshing.clone().lock_owned().await;
        let (outside, registry) = (self.clone(), registry.clone());
        tokio::spawn(async move {
            let _refreshing = refreshing;
            outside.take(&registry).await;
        });
    }

    /// The snapshot as it stands, or a new one where there is none yet, where
    /// `refresh` asks for one, or where the current one is older than
    /// [`SNAPSHOT_MAX_AGE`]. Requests that find it stale together wait on
    /// one refresh, so each agent is asked once for the lot; a `refresh`
    /// takes only a snapshot started after it was asked.
    pub(crate) async fn snapshot(&self, registry: &AgentRegistry, refresh: bool) -> Arc<Snapshot> {
        let asked = Instant::now();
        let usable = |snapshot: &Arc<Snapshot>| {
            snapshot.taken.elapsed() < SNAPSHOT_MAX_AGE && (!refresh || snapshot.started >= asked)
        };
        if let Some(snapshot) = self.current().filter(usable) {
            return snapshot;
        }
        let _refreshing = self.refreshing.lock().await;
        // The refresh this request waited on may have answered it already.
        if let Some(snapshot) = self.current().filter(usable) {
            return snapshot;
        }
        self.take(registry).await
    }

    /// A new snapshot, over both sources at once, in place of the current one.
    async fn take(&self, registry: &AgentRegistry) -> Arc<Snapshot> {
        let started = Instant::now();
        let sessions = gather(registry.stored_sessions(), self.disk_sessions(registry)).await;
        let snapshot = Arc::new(Snapshot {
            sessions,
            taken_at: Utc::now(),
            taken: Instant::now(),
            started,
        });
        *self.snapshot.lock().expect("the snapshot lock") = Some(snapshot.clone());
        snapshot
    }

    fn current(&self) -> Option<Arc<Snapshot>> {
        self.snapshot.lock().expect("the snapshot lock").clone()
    }

    /// The conversations on disk of every agent this daemon has, read off the
    /// blocking pool: one listing walks an agent's whole transcript directory.
    ///
    /// An agent the last discovery did not find ready is not read at all: its
    /// conversations are none of this daemon's, and no row of one could be
    /// resumed.
    async fn disk_sessions(&self, registry: &AgentRegistry) -> DiskListing {
        let ready: Vec<String> = registry
            .agents()
            .await
            .into_iter()
            .filter(|agent| agent.status == AcpAgentStatus::Ready)
            .map(|agent| agent.id)
            .collect();
        let readers = self.readers.clone();
        match tokio::task::spawn_blocking(move || readers.conversations(&ready)).await {
            Ok(sessions) => sessions,
            Err(error) => {
                tracing::warn!(error = %error, "reading the conversations on disk failed");
                DiskListing {
                    sessions: Vec::new(),
                    opencode_readable: false,
                }
            }
        }
    }

    /// The snapshot as it stands, however old, or an empty one where none
    /// has been taken yet — for a page no outside session can be in, which
    /// has no reason to ask an agent anything, or to wait for a refresh.
    pub(crate) fn cached(&self) -> Arc<Snapshot> {
        self.current().unwrap_or_else(|| {
            Arc::new(Snapshot {
                sessions: Vec::new(),
                taken_at: Utc::now(),
                taken: Instant::now(),
                started: Instant::now(),
            })
        })
    }
}

/// Both sources, asked at the same time, as one: what the agents answer over
/// ACP and what they left on disk. Neither waits for the other.
async fn gather(
    listed: impl Future<Output = Vec<OutsideSessionDto>>,
    disk: impl Future<Output = DiskListing>,
) -> Vec<OutsideSessionDto> {
    let (listed, disk) = tokio::join!(listed, disk);
    merge(listed, disk)
}

/// The two sources as one: every session `listed` over ACP, and every one on
/// disk that is not among them, by `(agent, internal id)`.
///
/// ACP rows stand except OpenCode rows absent from a readable database.
/// The agent's fields stand; only a missing title comes from the disk.
fn merge(listed: Vec<OutsideSessionDto>, disk: DiskListing) -> Vec<OutsideSessionDto> {
    let opencode_ids: HashSet<&str> = disk
        .sessions
        .iter()
        .filter(|session| session.agent_id == "opencode-acp")
        .map(|session| session.internal_session_id.as_str())
        .collect();
    let mut sessions: Vec<_> = listed
        .into_iter()
        .filter(|session| {
            session.agent_id != "opencode-acp"
                || !disk.opencode_readable
                || opencode_ids.contains(session.internal_session_id.as_str())
        })
        .collect();
    let mut at: HashMap<(String, String), usize> = sessions
        .iter()
        .enumerate()
        .map(|(at, session)| (session_key(session), at))
        .collect();
    for session in disk.sessions {
        match at.get(&session_key(&session)) {
            Some(&at) => {
                let row = &mut sessions[at];
                if row.first_prompt.trim().is_empty() {
                    row.first_prompt = session.first_prompt;
                }
            }
            None => {
                at.insert(session_key(&session), sessions.len());
                sessions.push(session);
            }
        }
    }
    sessions
}

/// Why a query cannot be answered: which value, and what is wrong with it.
#[derive(Debug)]
pub(crate) enum QueryError {
    /// A cursor this daemon did not write.
    InvalidCursor,
    /// A filter whose value cannot be read.
    InvalidFilter(String),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursor => write!(f, "the cursor is not one this daemon wrote"),
            Self::InvalidFilter(message) => write!(f, "{message}"),
        }
    }
}

/// The query of one listing request, read once: every value parsed, and the
/// page it asks for.
pub(crate) struct Filter {
    kind: Option<SessionKind>,
    agent: Option<String>,
    goal: Option<String>,
    task: Option<String>,
    status: Option<ariadne_core::SessionStatus>,
    seat: Option<Seat>,
    attention: bool,
    dir: Option<PathBuf>,
    /// The lower bound on activity: the caller's own, or the default window.
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    /// Lower-cased once, for the case-insensitive match.
    q: Option<String>,
    all: bool,
    limit: usize,
    after: Option<SortKey>,
}

impl Filter {
    pub(crate) fn parse(query: &SessionPageQuery) -> Result<Self, QueryError> {
        let bound = |name: &str, value: &Option<String>| match value {
            Some(value) => moment(value).map(Some).ok_or_else(|| {
                QueryError::InvalidFilter(format!("{name} is not RFC 3339: {value}"))
            }),
            None => Ok(None),
        };
        let dir = match &query.dir {
            Some(dir) if Path::new(dir).is_absolute() => Some(PathBuf::from(dir)),
            Some(dir) => {
                return Err(QueryError::InvalidFilter(format!(
                    "dir is not an absolute path: {dir}"
                )));
            }
            None => None,
        };
        let after = match &query.cursor {
            Some(cursor) => Some(Cursor::decode(cursor).ok_or(QueryError::InvalidCursor)?),
            None => None,
        };
        let all = query.all.unwrap_or(false);
        let since = bound("since", &query.since)?;
        let until = bound("until", &query.until)?;
        // The window the listing defaults to. A bound of the caller's own,
        // either way round, is the window they asked for, and `all` is none.
        let window = (!all && since.is_none() && until.is_none())
            .then(|| Utc::now() - TimeDelta::days(DEFAULT_WINDOW_DAYS));
        Ok(Self {
            kind: query.kind,
            agent: query.agent.clone(),
            goal: query.goal.clone(),
            task: query.task.clone(),
            status: query.status,
            seat: query.seat,
            attention: query.attention.unwrap_or(false),
            dir,
            since: since.or(window),
            until,
            q: query.q.as_ref().map(|q| q.to_lowercase()),
            all,
            limit: query.limit(),
            after: after.as_ref().map(Cursor::key),
        })
    }

    /// Whether the bounds keep a session last active then. A session whose
    /// activity cannot be read is outside every bound: nothing places it.
    fn within(&self, activity: Option<DateTime<Utc>>) -> bool {
        self.since
            .is_none_or(|since| activity.is_some_and(|at| at >= since))
            && self
                .until
                .is_none_or(|until| activity.is_some_and(|at| at <= until))
    }

    fn matches_agent(&self, agent_id: &str) -> bool {
        self.agent.as_ref().is_none_or(|agent| agent == agent_id)
    }

    fn matches_dir(&self, directory: Option<&str>) -> bool {
        self.dir.as_ref().is_none_or(|dir| {
            directory.is_some_and(|directory| Path::new(directory).starts_with(dir))
        })
    }

    fn matches_title(&self, title: Option<&str>) -> bool {
        self.q
            .as_ref()
            .is_none_or(|q| title.is_some_and(|title| title.to_lowercase().contains(q)))
    }

    fn takes_ariadne(&self) -> bool {
        self.kind != Some(SessionKind::Outside)
    }

    /// Whether an outside session can be in this page at all. It carries no
    /// goal, task, seat, status or attention, so a filter over one of those
    /// leaves none of them.
    pub(crate) fn takes_outside(&self) -> bool {
        self.kind != Some(SessionKind::Ariadne)
            && self.goal.is_none()
            && self.task.is_none()
            && self.status.is_none()
            && self.seat.is_none()
            && !self.attention
    }

    /// Only live sessions, which is what the listing is for. A named status
    /// is that choice made precisely, and `all` is the history asked for.
    fn live_only(&self) -> bool {
        !self.all && self.status.is_none()
    }
}

/// The titles behind the Ariadne sessions: the task's own, the goal's for
/// an orchestrator, which no task staffs, or a loose session's own. Two
/// listings, rather than a lookup for every row.
struct Titles {
    tasks: HashMap<String, String>,
    goals: HashMap<String, String>,
}

impl Titles {
    async fn load(store: &Store) -> Result<Self> {
        Ok(Self {
            tasks: store
                .list_tasks(TaskFilter::default())
                .await?
                .into_iter()
                .map(|task| (task.id, task.title))
                .collect(),
            goals: store
                .list_goals(&[])
                .await?
                .into_iter()
                .map(|goal| (goal.id, goal.title))
                .collect(),
        })
    }

    fn of(&self, session: &AgentSession) -> Option<String> {
        session
            .task_id
            .as_ref()
            .and_then(|id| self.tasks.get(id))
            .or_else(|| session.goal_id.as_ref().and_then(|id| self.goals.get(id)))
            .cloned()
            .or_else(|| session.title.clone())
    }
}

/// One row of the listing before the page is cut: what it takes to build the
/// entry, once the page is known.
enum Row<'a> {
    /// Boxed: a session row dwarfs the reference beside it, and most of a
    /// listing is rows the page leaves behind.
    Ariadne(Box<AgentSession>, Option<String>),
    Outside(&'a OutsideSessionDto),
}

/// When a session was last active, for the order and the window: a row
/// nothing has been heard from is as old as the row itself.
fn activity_of(session: &AgentSession) -> String {
    session
        .last_activity_at
        .clone()
        .unwrap_or_else(|| session.created_at.clone())
}

/// The Ariadne half: the rows the store narrows, and the filters it does not
/// hold.
async fn ariadne_rows<'a>(store: &Store, filter: &Filter) -> Result<Vec<(SortKey, Row<'a>)>> {
    if !filter.takes_ariadne() {
        return Ok(Vec::new());
    }
    let sessions = store
        .list_sessions(SessionFilter {
            goal_id: filter.goal.clone(),
            task_id: filter.task.clone(),
            status: filter.status,
            live_only: filter.live_only(),
            attention_only: filter.attention,
        })
        .await?;
    let titles = Titles::load(store).await?;
    let mut rows = Vec::new();
    for session in sessions {
        let title = titles.of(&session);
        let agent_id = agent_of(&session.model).to_string();
        let activity = activity_of(&session);
        let kept = filter.matches_agent(&agent_id)
            && filter.seat.is_none_or(|seat| session.seat() == Some(seat))
            && filter.matches_dir(session.worktree_path.as_deref())
            && filter.within(moment(&activity))
            && filter.matches_title(title.as_deref());
        if kept {
            let key = sort_key(&activity, agent_id, session.id.clone());
            rows.push((key, Row::Ariadne(Box::new(session), title)));
        }
    }
    Ok(rows)
}

/// The outside half: the snapshot's sessions the filter keeps and no Ariadne
/// row holds.
async fn outside_rows<'a>(
    snapshot: &'a Snapshot,
    store: &Store,
    filter: &Filter,
) -> Result<Vec<(SortKey, Row<'a>)>> {
    if !filter.takes_outside() {
        return Ok(Vec::new());
    }
    let known = adopted(store).await?;
    Ok(snapshot
        .sessions
        .iter()
        .filter(|session| {
            !known.contains(&session_key(session))
                && filter.matches_agent(&session.agent_id)
                && filter.matches_dir(Some(&session.working_directory))
                && filter.within(moment(&session.last_activity_at))
                && filter.matches_title(Some(&session.first_prompt))
        })
        .map(|session| {
            let key = sort_key(
                &session.last_activity_at,
                session.agent_id.clone(),
                session.internal_session_id.clone(),
            );
            (key, Row::Outside(session))
        })
        .collect())
}

/// One page of both halves, newest activity first, from just after the
/// cursor's row.
///
/// The cursor is a keyset over the sort key, so a page cut from a newer
/// snapshot continues from the same row rather than the same offset.
pub(crate) async fn page(
    snapshot: &Snapshot,
    readers: &Arc<DiskConversations>,
    store: &Store,
    filter: &Filter,
) -> Result<SessionPageDto> {
    let mut rows = ariadne_rows(store, filter).await?;
    rows.extend(outside_rows(snapshot, store, filter).await?);
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let total = rows.len();
    let start = match &filter.after {
        Some(after) => rows.partition_point(|(key, _)| key <= after),
        None => 0,
    };
    let page = &rows[start..];
    let page = &page[..page.len().min(filter.limit)];
    let next_cursor = (start + page.len() < total)
        .then(|| page.last().map(|(key, row)| Cursor::of(key, row).encode()))
        .flatten();
    let mut sessions = Vec::with_capacity(page.len());
    for (_, row) in page {
        sessions.push(match row {
            Row::Ariadne(session, title) => {
                session_entry_of(store, (**session).clone(), title.clone()).await?
            }
            Row::Outside(outside) => outside_entry(outside),
        });
    }
    // The figures of this page's rows, and of no other file: what a
    // conversation ran on and what it spent are on disk, not in `session/list`.
    fill_page(readers, &mut sessions).await;
    Ok(SessionPageDto {
        sessions,
        next_cursor,
        total,
        snapshot_at: snapshot
            .taken_at
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    })
}

/// The order of the listing: last activity, newest first — a session whose
/// activity cannot be read comes last — then agent id, then session id.
type SortKey = (Reverse<Option<DateTime<Utc>>>, String, String);

fn sort_key(activity: &str, agent_id: String, id: String) -> SortKey {
    (Reverse(moment(activity)), agent_id, id)
}

fn moment(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// The sort key of the last row of a page, as the client carries it back:
/// its fields under a base64url coat, which is all that makes it opaque.
#[derive(Serialize, Deserialize)]
struct Cursor {
    last_activity_at: String,
    agent_id: String,
    id: String,
}

impl Cursor {
    /// The moment comes off the row, word for word, rather than off the key:
    /// an agent that dates a session finer than a millisecond would lose that
    /// much of it to being formatted again, and the next page would skip the
    /// rows inside the millisecond it landed in.
    fn of(key: &SortKey, row: &Row<'_>) -> Self {
        Self {
            last_activity_at: match row {
                Row::Ariadne(session, _) => activity_of(session),
                Row::Outside(outside) => outside.last_activity_at.clone(),
            },
            agent_id: key.1.clone(),
            id: key.2.clone(),
        }
    }

    fn key(&self) -> SortKey {
        sort_key(
            &self.last_activity_at,
            self.agent_id.clone(),
            self.id.clone(),
        )
    }

    fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("a cursor serializes"))
    }

    fn decode(cursor: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(cursor).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str) -> OutsideSessionDto {
        OutsideSessionDto {
            agent_id: "agent".into(),
            internal_session_id: id.into(),
            working_directory: "/work".into(),
            last_activity_at: "2026-01-01T00:00:00.000Z".into(),
            first_prompt: id.into(),
        }
    }

    /// The two sources are asked at the same time: each waits at a barrier
    /// only the other can lift, so one asked after the other never ends.
    #[tokio::test]
    async fn the_two_sources_are_asked_at_the_same_time() {
        let barrier = tokio::sync::Barrier::new(2);
        let source = |id: &'static str| {
            let barrier = &barrier;
            async move {
                barrier.wait().await;
                vec![row(id)]
            }
        };

        let sessions = tokio::time::timeout(
            Duration::from_secs(30),
            gather(source("listed"), async {
                DiskListing {
                    sessions: source("disk").await,
                    opencode_readable: false,
                }
            }),
        )
        .await
        .expect("both sources to answer");

        let ids: Vec<&str> = sessions
            .iter()
            .map(|session| session.internal_session_id.as_str())
            .collect();
        assert_eq!(ids, ["listed", "disk"]);
    }
}
