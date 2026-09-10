//! Sessions a supported CLI recorded before Ariadne started managing them.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::sqlite::SqliteConnectOptions;

use ariadne_api::sessions::OutsideSessionDto;
use ariadne_core::AgentKind;
use ariadne_store::{SessionFilter, Store};

/// Find sessions in all CLI stores that have no Ariadne session row.
pub async fn discover(store: &Store, home: &Path) -> Result<Vec<OutsideSessionDto>> {
    let known: HashSet<_> = store
        .list_sessions(SessionFilter::default())
        .await?
        .into_iter()
        .filter_map(|session| {
            let agent_kind = session.agent_kind();
            session.internal_session_id.map(|id| (agent_kind, id))
        })
        .collect();
    discover_in(home, &known).await
}

/// Find sessions under `home`. This is public so the fixture tests exercise
/// the same scanner as the REST endpoint without reading a user's stores.
pub async fn discover_in(
    home: &Path,
    known: &HashSet<(AgentKind, String)>,
) -> Result<Vec<OutsideSessionDto>> {
    let mut sessions = Vec::new();
    sessions.extend(claude_sessions(&home.join(".claude/projects"))?);
    sessions.extend(codex_sessions(&home.join(".codex/sessions"))?);
    sessions.extend(opencode_sessions(&home.join(".local/share/opencode/opencode.db")).await?);
    sessions.retain(|session| {
        !known.contains(&(session.agent_kind, session.internal_session_id.clone()))
    });
    sessions.sort_by(|left, right| right.last_activity_at.cmp(&left.last_activity_at));
    Ok(sessions)
}

fn claude_sessions(root: &Path) -> Result<Vec<OutsideSessionDto>> {
    transcript_files(root)
        .into_iter()
        .filter_map(|path| claude_session(&path).transpose())
        .collect()
}

fn claude_session(path: &Path) -> Result<Option<OutsideSessionDto>> {
    let text = fs::read_to_string(path)?;
    let mut id = None;
    let mut cwd = None;
    let mut prompt = None;
    let mut last = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        id = id.or_else(|| string(&value, "sessionId"));
        cwd = cwd.or_else(|| string(&value, "cwd"));
        last = latest(last, string(&value, "timestamp"));
        if prompt.is_none()
            && value.get("type").and_then(|v| v.as_str()) == Some("user")
            && value.pointer("/message/role").and_then(|v| v.as_str()) == Some("user")
        {
            prompt = message_text(value.pointer("/message/content"));
        }
    }
    Ok(record(
        AgentKind::ClaudeCode,
        id.or_else(|| stem(path)),
        cwd,
        last.or_else(|| modified_at(path)),
        prompt,
    ))
}

fn codex_sessions(root: &Path) -> Result<Vec<OutsideSessionDto>> {
    transcript_files(root)
        .into_iter()
        .filter_map(|path| codex_session(&path).transpose())
        .collect()
}

fn codex_session(path: &Path) -> Result<Option<OutsideSessionDto>> {
    let text = fs::read_to_string(path)?;
    let mut id = None;
    let mut cwd = None;
    let mut prompt = None;
    let mut last = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        last = latest(last, string(&value, "timestamp"));
        if value.get("type").and_then(|v| v.as_str()) == Some("session_meta") {
            id = id.or_else(|| value.pointer("/payload/id").and_then(value_string));
            cwd = cwd.or_else(|| value.pointer("/payload/cwd").and_then(value_string));
        }
        let message = match value.get("type").and_then(|v| v.as_str()) {
            Some("message") => Some(&value),
            Some("response_item") => value.get("payload"),
            _ => None,
        };
        if prompt.is_none()
            && message
                .and_then(|message| message.get("role"))
                .and_then(|v| v.as_str())
                == Some("user")
        {
            prompt = message.and_then(|message| message_text(message.get("content")));
        }
    }
    Ok(record(
        AgentKind::Codex,
        id.or_else(|| stem(path)),
        cwd,
        last.or_else(|| modified_at(path)),
        prompt,
    ))
}

async fn opencode_sessions(path: &Path) -> Result<Vec<OutsideSessionDto>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let rows: Vec<(String, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT s.id, s.directory, s.time_updated, (
             SELECT p.data FROM message m JOIN part p ON p.message_id = m.id
              WHERE m.session_id = s.id
                AND json_extract(m.data, '$.role') = 'user'
                AND json_extract(p.data, '$.type') = 'text'
              ORDER BY m.time_created, p.time_created LIMIT 1
         ) FROM session s",
    )
    .fetch_all(&pool)
    .await?;
    pool.close().await;
    Ok(rows
        .into_iter()
        .filter_map(|(id, directory, updated, first_part)| {
            let prompt = first_part
                .as_deref()
                .and_then(|part| serde_json::from_str::<serde_json::Value>(part).ok())
                .and_then(|part| string(&part, "text"));
            record(
                AgentKind::Opencode,
                Some(id),
                Some(directory),
                Utc.timestamp_millis_opt(updated)
                    .single()
                    .map(|time| time.to_rfc3339()),
                prompt,
            )
        })
        .collect())
}

fn transcript_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit(root, &mut files);
    files
}

fn visit(path: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn record(
    agent_kind: AgentKind,
    id: Option<String>,
    cwd: Option<String>,
    last_activity_at: Option<String>,
    first_prompt: Option<String>,
) -> Option<OutsideSessionDto> {
    id.zip(cwd).zip(last_activity_at).zip(first_prompt).map(
        |(((internal_session_id, working_directory), last_activity_at), first_prompt)| {
            OutsideSessionDto {
                agent_kind,
                internal_session_id,
                working_directory,
                last_activity_at,
                first_prompt,
            }
        },
    )
}

fn string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(value_string)
}

fn value_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(ToOwned::to_owned)
}

fn message_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Array(parts)) => parts.iter().find_map(|part| {
            (part.get("type").and_then(|v| v.as_str()) == Some("input_text"))
                .then(|| string(part, "text"))
                .flatten()
        }),
        _ => None,
    }
}

fn latest(current: Option<String>, candidate: Option<String>) -> Option<String> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (current, candidate) => current.or(candidate),
    }
}

fn modified_at(path: &Path) -> Option<String> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(time).to_rfc3339())
}

fn stem(path: &Path) -> Option<String> {
    path.file_stem()?
        .to_str()?
        .strip_prefix("rollout-")
        .map(ToOwned::to_owned)
}
