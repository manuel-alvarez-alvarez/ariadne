//! Agent-event repository: what the ACP runtime reports each agent doing.
//!
//! These rows are most of a database that has run for months — 342 MB of
//! payload over 81,425 rows on the author's — so a payload is stored packed
//! and unpacked again here, and nowhere else: an [`AgentEvent`] carries the
//! JSON its reporter sent, whatever the row under it holds.

use std::io::{Read, Write};

use ariadne_core::id::new_id;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use crate::{AgentEvent, Change, Result, Store, now};

/// Deflate, which is what a payload packs under: 4.26x over 5,000 real
/// payloads, for 46 µs on the 4.2 KB average one. A row names its codec, so a
/// later build can pack a new row under another one and still read this one.
const DEFLATE: &str = "deflate";
/// The payload as it was reported. What a payload deflate cannot shrink —
/// anything short enough that the stream's own header costs more than it
/// saves — is stored as it came, so a row is never bigger for being packed.
const PLAIN: &str = "none";

/// The payload as it is stored, and the codec that names it. Deflate where
/// deflate is smaller, the text itself where it is not.
fn pack(payload: &str) -> (Vec<u8>, &'static str) {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    // Writing into a `Vec` cannot fail, so neither can the encoder over one.
    encoder
        .write_all(payload.as_bytes())
        .expect("deflate a Vec");
    let packed = encoder.finish().expect("finish a Vec");
    if packed.len() < payload.len() {
        (packed, DEFLATE)
    } else {
        (payload.as_bytes().to_vec(), PLAIN)
    }
}

/// The payload a row holds, read back. A codec this build does not know, a
/// stream it cannot inflate and bytes that are not UTF-8 are all one thing:
/// a row this build cannot read, reported where the column is read.
fn unpack(stored: Vec<u8>, codec: &str) -> sqlx::Result<String> {
    let bytes = match codec {
        DEFLATE => {
            let mut out = Vec::new();
            DeflateDecoder::new(&stored[..])
                .read_to_end(&mut out)
                .map_err(undecodable)?;
            out
        }
        PLAIN => stored,
        other => return Err(undecodable(format!("unknown payload codec {other:?}"))),
    };
    String::from_utf8(bytes).map_err(undecodable)
}

fn undecodable(source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> sqlx::Error {
    sqlx::Error::ColumnDecode {
        index: "payload".into(),
        source: source.into(),
    }
}

/// Every read of an event goes through here, so every read unpacks.
impl sqlx::FromRow<'_, SqliteRow> for AgentEvent {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let stored: Vec<u8> = row.try_get("payload")?;
        let codec: String = row.try_get("payload_codec")?;
        Ok(Self {
            id: row.try_get("id")?,
            session_id: row.try_get("session_id")?,
            task_id: row.try_get("task_id")?,
            kind: row.try_get("kind")?,
            payload: unpack(stored, &codec)?,
            created_at: row.try_get("created_at")?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewAgentEvent {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Which end of the recorded events a page is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EventOrder {
    /// Forward from the oldest, which is what a sweep with `after` walks.
    #[default]
    Asc,
    /// Back from the newest, which is what a snapshot of the recent past
    /// wants.
    Desc,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    /// The goal an event belongs to, through its session or through its task.
    pub goal_id: Option<String>,
    /// Only events with an id above this one.
    pub after: Option<String>,
    /// Only events with an id below this one, which is how a descending page
    /// walks further back.
    pub before: Option<String>,
    pub order: EventOrder,
    pub limit: i64,
}

/// What `create_event` runs, whole: one statement, which is what keeps the
/// lock over it short.
const CREATE_EVENT: &str =
    "INSERT INTO agent_events (id, session_id, task_id, kind, payload, payload_codec, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)
     RETURNING *";

impl Store {
    pub async fn create_event(&self, new: NewAgentEvent) -> Result<AgentEvent> {
        // Packed before the lock: `event_order` also holds up every live
        // console chunk of every session, so it covers the one statement and
        // nothing else.
        let (payload, codec) = pack(&new.payload.to_string());
        let created_at = now();
        // The id is taken and the event published under one lock, so two
        // writers racing on the write pool cannot publish a higher id before
        // a lower one: the order on the change channel is the order of ids.
        let _order = self.event_order().lock().await;
        let id = new_id();
        // One statement: the row is written and read back by the same
        // `RETURNING`, so the write pool is asked once and the read pool not
        // at all.
        let event = sqlx::query_as::<_, AgentEvent>(CREATE_EVENT)
            .bind(&id)
            .bind(&new.session_id)
            .bind(&new.task_id)
            .bind(&new.kind)
            .bind(payload)
            .bind(codec)
            .bind(created_at)
            .fetch_one(self.w())
            .await?;
        self.publish(Change::AgentEventCreated(event.clone()));
        Ok(event)
    }

    /// The events of a session with an id above `after`, in order: what a
    /// console stream committed to the store but not yet published on the
    /// bus when a live event overtook it (008). `None` is every event.
    pub async fn list_session_events_after(
        &self,
        session_id: &str,
        after: Option<&str>,
    ) -> Result<Vec<AgentEvent>> {
        Ok(sqlx::query_as::<_, AgentEvent>(
            "SELECT * FROM agent_events WHERE session_id = ? AND id > ? ORDER BY id",
        )
        .bind(session_id)
        .bind(after.unwrap_or(""))
        .fetch_all(self.r())
        .await?)
    }

    /// Every event a session has produced, in order: the whole transcript an
    /// console replays from, where `list_events`'s page cap would
    /// truncate a long conversation.
    pub async fn list_session_events(&self, session_id: &str) -> Result<Vec<AgentEvent>> {
        Ok(sqlx::query_as::<_, AgentEvent>(
            "SELECT * FROM agent_events WHERE session_id = ? ORDER BY id",
        )
        .bind(session_id)
        .fetch_all(self.r())
        .await?)
    }

    /// A page of recorded events, narrowed by whichever filters were set.
    ///
    /// The query is assembled here rather than by `Filtered`, which builds one
    /// `SELECT * FROM <table>` and binds each clause once: the goal reaches an
    /// event through two other tables and binds its id twice. Every fragment
    /// below is a literal and every value is bound, which is what makes the
    /// assembled string safe to assert.
    pub async fn list_events(&self, filter: EventFilter) -> Result<Vec<AgentEvent>> {
        let limit = match filter.limit {
            n if n <= 0 => 50,
            n => n.min(200),
        };
        let mut sql = String::from("SELECT * FROM agent_events WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();
        // A cursor is bound only where it is set: `id < ''` matches no row,
        // so an unset `before` written as an empty string would answer
        // nothing at all.
        if let Some(after) = filter.after {
            sql.push_str(" AND id > ?");
            binds.push(after);
        }
        if let Some(before) = filter.before {
            sql.push_str(" AND id < ?");
            binds.push(before);
        }
        if let Some(session_id) = filter.session_id {
            sql.push_str(" AND session_id = ?");
            binds.push(session_id);
        }
        if let Some(task_id) = filter.task_id {
            sql.push_str(" AND task_id = ?");
            binds.push(task_id);
        }
        // `agent_events` holds no goal of its own: an event belongs to the
        // goal of the session that reported it, or of the task it was on.
        // Both are read, because an event can carry a task and no session.
        if let Some(goal_id) = filter.goal_id {
            sql.push_str(
                " AND (session_id IN (SELECT id FROM agent_sessions WHERE goal_id = ?)
                    OR task_id IN (SELECT id FROM tasks WHERE goal_id = ?))",
            );
            binds.push(goal_id.clone());
            binds.push(goal_id);
        }
        sql.push_str(match filter.order {
            EventOrder::Asc => " ORDER BY id LIMIT ?",
            EventOrder::Desc => " ORDER BY id DESC LIMIT ?",
        });
        let mut q = sqlx::query_as::<_, AgentEvent>(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            q = q.bind(bind);
        }
        Ok(q.bind(limit).fetch_all(self.r()).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One statement, on the write pool alone: the text is a single
    /// `INSERT … RETURNING`, and with the read pool closed the write still
    /// answers with the row it wrote. A second round trip, to either pool,
    /// would be a second wait under `event_order`, which every live console
    /// chunk of every session waits behind.
    #[tokio::test]
    async fn an_event_is_written_and_read_back_by_one_statement() {
        assert_eq!(CREATE_EVENT.matches("INSERT").count(), 1);
        assert!(CREATE_EVENT.contains("RETURNING"));
        assert!(!CREATE_EVENT.contains("SELECT"));
        assert!(!CREATE_EVENT.contains(';'), "one statement, not two");

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        store.read.close().await;

        let event = store
            .create_event(NewAgentEvent {
                session_id: None,
                task_id: None,
                kind: "stop".into(),
                payload: serde_json::json!({"stop_reason": "end_turn"}),
            })
            .await
            .expect("the write pool answers it alone");

        assert_eq!(event.kind, "stop");
        assert_eq!(event.payload, r#"{"stop_reason":"end_turn"}"#);
    }

    /// A row this build cannot read is reported, not guessed at: a codec a
    /// later build wrote reaches this one as a decode error on the column.
    #[test]
    fn a_codec_this_build_does_not_know_is_a_decode_error() {
        let error = unpack(b"whatever".to_vec(), "brotli").unwrap_err();
        assert!(
            matches!(&error, sqlx::Error::ColumnDecode { index, .. } if index == "payload"),
            "{error}"
        );
    }
}
