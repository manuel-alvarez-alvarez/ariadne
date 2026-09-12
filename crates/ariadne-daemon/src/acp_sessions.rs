//! Sessions an ACP agent stores itself, asked over the protocol: the ones
//! Ariadne did not start, which a task can adopt.
//!
//! The listing is served off one in-memory snapshot of every agent's stored
//! sessions ([`OutsideSessions`]), cut into filtered pages at request time;
//! adoption still asks the agents afresh ([`discover`]).

use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use ariadne_api::sessions::{OutsideSessionDto, OutsideSessionListQuery, OutsideSessionPageDto};
use ariadne_core::models::agent_of;
use ariadne_store::{SessionFilter, Store};

use crate::acp_discovery::AgentRegistry;

/// How old the snapshot may be before a request takes it again.
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(60);

/// The stored sessions of every registry agent that can list them, minus the
/// ones already bound to an Ariadne session row — asked of the agents now.
pub async fn discover(registry: &AgentRegistry, store: &Store) -> Result<Vec<OutsideSessionDto>> {
    let known = adopted(store).await?;
    let mut sessions = registry.stored_sessions().await;
    sessions.retain(|session| !known.contains(&session_key(session)));
    Ok(sessions)
}

/// The `(agent, internal id)` of every session an Ariadne row already holds.
///
/// The `model` column of a session carries `<agent>:<model>`, so the agent an
/// already-adopted session belongs to is read back off it rather than kept
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

/// Every outside session of every agent at one moment, adopted ones
/// included: what a row already holds is subtracted when a page is cut, so
/// an adoption shows without a new snapshot.
pub struct Snapshot {
    sessions: Vec<OutsideSessionDto>,
    /// The moment, for the reply.
    taken_at: DateTime<Utc>,
    /// The same moment, monotonic, for the age.
    taken: Instant,
}

/// The daemon's one snapshot of the outside sessions. Nothing of it reaches
/// the disk.
#[derive(Clone, Default)]
pub struct OutsideSessions {
    snapshot: Arc<Mutex<Option<Arc<Snapshot>>>>,
}

impl OutsideSessions {
    /// The snapshot as it stands, or a new one where there is none yet, where
    /// `refresh` asks for one, or where the current one is older than
    /// [`SNAPSHOT_MAX_AGE`]. Requests that find it stale together wait on
    /// one another, so each agent is asked once for the lot.
    pub async fn snapshot(&self, registry: &AgentRegistry, refresh: bool) -> Arc<Snapshot> {
        let mut current = self.snapshot.lock().await;
        if let Some(snapshot) = current.as_ref()
            && !refresh
            && snapshot.taken.elapsed() < SNAPSHOT_MAX_AGE
        {
            return snapshot.clone();
        }
        let snapshot = Arc::new(Snapshot {
            sessions: registry.stored_sessions().await,
            taken_at: Utc::now(),
            taken: Instant::now(),
        });
        *current = Some(snapshot.clone());
        snapshot
    }
}

/// Why a query cannot be answered: which value, and what is wrong with it.
#[derive(Debug)]
pub enum QueryError {
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
pub struct Filter {
    agent: Option<String>,
    dir: Option<PathBuf>,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    /// Lower-cased once, for the case-insensitive match.
    q: Option<String>,
    limit: usize,
    after: Option<SortKey>,
}

impl Filter {
    pub fn parse(query: &OutsideSessionListQuery) -> Result<Self, QueryError> {
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
        Ok(Self {
            agent: query.agent.clone(),
            dir,
            since: bound("since", &query.since)?,
            until: bound("until", &query.until)?,
            q: query.q.as_ref().map(|q| q.to_lowercase()),
            limit: query.limit(),
            after: after.as_ref().map(Cursor::key),
        })
    }

    fn matches(&self, session: &OutsideSessionDto) -> bool {
        let activity = moment(&session.last_activity_at);
        self.agent
            .as_ref()
            .is_none_or(|agent| &session.agent_id == agent)
            && self
                .dir
                .as_ref()
                .is_none_or(|dir| Path::new(&session.working_directory).starts_with(dir))
            && self
                .since
                .is_none_or(|since| activity.is_some_and(|at| at >= since))
            && self
                .until
                .is_none_or(|until| activity.is_some_and(|at| at <= until))
            && self
                .q
                .as_ref()
                .is_none_or(|q| session.first_prompt.to_lowercase().contains(q))
    }
}

/// One page of the snapshot: the sessions the filter keeps and no Ariadne row
/// holds, newest activity first, from just after the cursor's row.
///
/// The cursor is a keyset over the sort key, so a page cut from a newer
/// snapshot continues from the same row rather than the same offset.
pub async fn page(
    snapshot: &Snapshot,
    store: &Store,
    filter: &Filter,
) -> Result<OutsideSessionPageDto> {
    let known = adopted(store).await?;
    let mut rows: Vec<(SortKey, &OutsideSessionDto)> = snapshot
        .sessions
        .iter()
        .filter(|session| !known.contains(&session_key(session)) && filter.matches(session))
        .map(|session| (sort_key(session), session))
        .collect();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    let total = rows.len();
    let start = match &filter.after {
        Some(after) => rows.partition_point(|(key, _)| key <= after),
        None => 0,
    };
    let page: Vec<&OutsideSessionDto> = rows[start..]
        .iter()
        .take(filter.limit)
        .map(|(_, session)| *session)
        .collect();
    let next_cursor = (start + page.len() < total)
        .then(|| page.last().map(|last| Cursor::of(last).encode()))
        .flatten();
    Ok(OutsideSessionPageDto {
        sessions: page.into_iter().cloned().collect(),
        next_cursor,
        total,
        snapshot_at: snapshot
            .taken_at
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    })
}

/// The order of the listing: last activity, newest first — a session whose
/// activity cannot be read comes last — then agent id, then internal id.
type SortKey = (Reverse<Option<DateTime<Utc>>>, String, String);

fn sort_key(session: &OutsideSessionDto) -> SortKey {
    (
        Reverse(moment(&session.last_activity_at)),
        session.agent_id.clone(),
        session.internal_session_id.clone(),
    )
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
    internal_session_id: String,
}

impl Cursor {
    fn of(session: &OutsideSessionDto) -> Self {
        Self {
            last_activity_at: session.last_activity_at.clone(),
            agent_id: session.agent_id.clone(),
            internal_session_id: session.internal_session_id.clone(),
        }
    }

    fn key(&self) -> SortKey {
        (
            Reverse(moment(&self.last_activity_at)),
            self.agent_id.clone(),
            self.internal_session_id.clone(),
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
