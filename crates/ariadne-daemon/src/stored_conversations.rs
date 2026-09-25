//! The conversations an agent keeps on disk, read from its own files.
//!
//! ACP's `session/list` is one source of outside conversations, and this is
//! the other. Each adapter answers that call from an index of its own, and
//! every index holds less than the disk does: on 2026-09-25 claude-acp listed
//! 731 of 740 transcripts on this machine. The call also carries no model and
//! no tokens, which both lie in the files.
//!
//! [`StoredConversations`] is the contract one agent's reader answers: every
//! conversation that agent stored, and the model, effort, and tokens of one
//! of them by its internal session id. [`DiskConversations::new`] is where a
//! reader is registered, under the registry agent id it answers for;
//! `claude-acp`, `codex-acp` and `opencode-acp` have the only three today. A
//! reader answers for its own agent and for no other.
//!
//! OpenCode's `session/list` answers with the newest 100 root sessions of the
//! directory it runs in and no further page, so a conversation outside that
//! window is invisible to it. On 2026-09-25 its database held 243 root
//! sessions, 101 of them with messages, and not one of the 101 was in the
//! answer. [`OpencodeConversations`] reads that database directly instead,
//! opened read-only on every call: it never writes, locks or migrates a file
//! OpenCode itself may have open.
//!
//! A reader caches both reads by the file's path, size and modification time:
//! from its start as far as a listing needs, and from its figures source for
//! one row. So a listing reads no file it has read unchanged, and the figures
//! of a page are read from the files of that page's rows alone. The two reads
//! stand for one stamp, so a file read again for a page is read again for the
//! listing with it.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Sqlite, query_as};

use ariadne_api::sessions::{OutsideSessionDto, SessionEntryDto, SessionKind};
use ariadne_core::TokenUsage;

use crate::launcher::loose_title;
use crate::transcript::{TranscriptHomes, claude_request_usage};

/// The registry agent Claude Code's transcripts belong to.
const CLAUDE_AGENT_ID: &str = "claude-acp";

/// The registry agent Codex's rollouts belong to.
const CODEX_AGENT_ID: &str = "codex-acp";

/// The most bytes a page reads from the end of one Codex rollout.
const CODEX_FIGURES_TAIL_BYTES: u64 = 64 * 1024;

/// The registry agent OpenCode's database belongs to.
const OPENCODE_AGENT_ID: &str = "opencode-acp";

/// What one stored conversation ran on and what it spent.
///
/// The model is the agent's own name for it, without the registry agent id in
/// front: [`DiskConversations`] puts that there, because it knows which reader
/// answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Figures {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) usage: Option<TokenUsage>,
}

/// One agent's stored conversations, as its own files hold them.
///
/// A reader is registered in [`DiskConversations::new`] under the agent id it
/// answers for, and the registry names every row it hands back: a reader
/// leaves `agent_id` empty.
///
/// The daemon takes a snapshot before it cuts a page, so [`Self::figures`] is
/// only ever asked about a conversation [`Self::conversations`] has already
/// found, and answers `None` for anything else.
pub(crate) trait StoredConversations: Send + Sync {
    /// Every conversation the agent stored: one row each, with the working
    /// directory, the title and the last activity its file holds.
    fn conversations(&self) -> Vec<OutsideSessionDto>;

    /// What one conversation ran on and what it spent, or `None` where the
    /// agent stored no such conversation.
    fn figures(&self, internal_session_id: &str) -> Option<Figures>;
}

/// The disk readers of one daemon, by registry agent id.
pub(crate) struct DiskConversations {
    readers: HashMap<String, Box<dyn StoredConversations>>,
}

impl DiskConversations {
    /// The readers a daemon runs with, reading under `homes`. Register an
    /// agent's reader here, and nowhere else.
    pub(crate) fn new(homes: &TranscriptHomes) -> Self {
        let claude = ClaudeConversations::new(homes.claude.clone());
        let codex = CodexConversations::new(homes.codex.clone());
        let opencode = OpencodeConversations::new(homes.opencode.join("opencode.db"));
        Self {
            readers: [
                (
                    CLAUDE_AGENT_ID.to_string(),
                    Box::new(claude) as Box<dyn StoredConversations>,
                ),
                (
                    CODEX_AGENT_ID.to_string(),
                    Box::new(codex) as Box<dyn StoredConversations>,
                ),
                (
                    OPENCODE_AGENT_ID.to_string(),
                    Box::new(opencode) as Box<dyn StoredConversations>,
                ),
            ]
            .into_iter()
            .collect(),
        }
    }

    /// Every conversation on disk of the agents `agents` names, each row under
    /// the agent id its reader is registered by. An agent with no reader, and
    /// an agent this daemon does not have, contribute nothing.
    pub(crate) fn conversations(&self, agents: &[String]) -> Vec<OutsideSessionDto> {
        agents
            .iter()
            .filter_map(|agent_id| Some((agent_id, self.readers.get(agent_id)?)))
            .flat_map(|(agent_id, reader)| {
                reader.conversations().into_iter().map(|mut session| {
                    session.agent_id = agent_id.clone();
                    session
                })
            })
            .collect()
    }

    /// The figures of the conversations `asked` names, by `(agent id, internal
    /// session id)` — the rows of one page, and no other file.
    fn figures_of(&self, asked: &[(String, String)]) -> HashMap<(String, String), Figures> {
        asked
            .iter()
            .filter_map(|key| {
                let figures = self.readers.get(&key.0)?.figures(&key.1)?;
                Some((key.clone(), figures))
            })
            .collect()
    }
}

/// Fill `model` and `usage` on the outside rows of one page, off the files of
/// those rows.
///
/// The reads happen on the blocking pool: a page holds up to 200 rows and one
/// transcript of a long conversation is megabytes, which is file work and not
/// the runtime's. A read that fails leaves its row as it was, so a page
/// answers without the figures rather than not at all.
pub(crate) async fn fill_page(readers: &Arc<DiskConversations>, page: &mut [SessionEntryDto]) {
    let asked: Vec<(String, String)> = page
        .iter()
        .filter(|entry| entry.kind == SessionKind::Outside)
        .filter_map(|entry| Some((entry.agent_id.clone(), entry.internal_session_id.clone()?)))
        .collect();
    if asked.is_empty() {
        return;
    }
    let readers = readers.clone();
    let read = tokio::task::spawn_blocking(move || readers.figures_of(&asked)).await;
    let figures = match read {
        Ok(figures) => figures,
        Err(error) => {
            tracing::warn!(error = %error, "reading the transcripts of a page failed");
            return;
        }
    };
    for entry in page
        .iter_mut()
        .filter(|entry| entry.kind == SessionKind::Outside)
    {
        let Some(internal) = entry.internal_session_id.clone() else {
            continue;
        };
        let Some(found) = figures.get(&(entry.agent_id.clone(), internal)) else {
            continue;
        };
        // The same spelling as every other row's: the agent, and after the
        // `:` the model it ran.
        entry.model = found
            .model
            .as_ref()
            .map(|model| format!("{}:{model}", entry.agent_id));
        entry.effort = found.effort.clone();
        entry.usage = found.usage.map(Into::into);
    }
}

/// Codex's rollouts: `<home>/sessions/**/rollout-*.jsonl`. The first line
/// names the session and working directory. The tail has the model, effort,
/// and running token total of its last turn.
struct CodexConversations {
    home: PathBuf,
    cache: Mutex<HashMap<PathBuf, CodexCached>>,
}

/// What one rollout held when it was last read.
struct CodexCached {
    stamp: Stamp,
    listed: CodexListed,
    figures: Option<Figures>,
}

/// What a Codex rollout says a listing needs.
struct CodexListed {
    internal_session_id: String,
    working_directory: String,
    has_turn: bool,
    last_activity_at: String,
}

impl CodexConversations {
    fn new(home: PathBuf) -> Self {
        Self {
            home,
            cache: Mutex::default(),
        }
    }
}

impl StoredConversations for CodexConversations {
    fn conversations(&self) -> Vec<OutsideSessionDto> {
        let mut cache = self.cache.lock().expect("the rollout cache lock");
        let mut sessions = Vec::new();
        let mut found = HashSet::new();
        for (path, stamp) in codex_rollouts(&self.home) {
            let fresh = cache.get(&path).is_some_and(|cached| cached.stamp == stamp);
            if !fresh {
                cache.insert(
                    path.clone(),
                    CodexCached {
                        stamp,
                        listed: read_codex_listed(&path, &stamp),
                        figures: None,
                    },
                );
            }
            let cached = &cache[&path];
            if cached.listed.has_turn && !cached.listed.internal_session_id.is_empty() {
                sessions.push(OutsideSessionDto {
                    agent_id: String::new(),
                    internal_session_id: cached.listed.internal_session_id.clone(),
                    working_directory: cached.listed.working_directory.clone(),
                    last_activity_at: cached.listed.last_activity_at.clone(),
                    first_prompt: String::new(),
                });
            }
            found.insert(path);
        }
        cache.retain(|path, _| found.contains(path));
        sessions
    }

    fn figures(&self, internal_session_id: &str) -> Option<Figures> {
        let mut cache = self.cache.lock().expect("the rollout cache lock");
        let (path, cached) = cache
            .iter_mut()
            .find(|(_, cached)| cached.listed.internal_session_id == internal_session_id)?;
        if let Some(stamp) = stamp_of(path).filter(|stamp| *stamp != cached.stamp) {
            cached.listed = read_codex_listed(path, &stamp);
            cached.stamp = stamp;
            cached.figures = None;
        }
        if cached.figures.is_none() {
            cached.figures = Some(read_codex_figures(path));
        }
        cached.figures.clone()
    }
}

/// Every Codex rollout under `<home>/sessions`, wherever its dated directory
/// puts it.
fn codex_rollouts(home: &Path) -> Vec<(PathBuf, Stamp)> {
    let mut found = Vec::new();
    let mut directories = vec![home.join("sessions")];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file()
                && path.extension().and_then(OsStr::to_str) == Some("jsonl")
                && path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.starts_with("rollout-"))
                && let Ok(metadata) = entry.metadata()
            {
                found.push((path, stamp_of_metadata(metadata)));
            }
        }
    }
    found
}

/// Read the rollout's first line. A rollout that wrote more than its session
/// metadata has started a turn; no-turn rollouts contain that one line alone.
fn read_codex_listed(path: &Path, stamp: &Stamp) -> CodexListed {
    let mut listed = CodexListed {
        internal_session_id: String::new(),
        working_directory: String::new(),
        has_turn: false,
        last_activity_at: stamp.modified.map(moment).unwrap_or_default(),
    };
    let Ok(file) = File::open(path) else {
        return listed;
    };
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let Ok(meta_bytes) = reader.read_line(&mut line) else {
        return listed;
    };
    let Ok(meta) = serde_json::from_str::<Value>(line.trim_end()) else {
        return listed;
    };
    if meta.get("type").and_then(Value::as_str) != Some("session_meta")
        || meta.pointer("/payload/source").and_then(Value::as_str) == Some("subagent")
    {
        return listed;
    }
    listed.internal_session_id = meta
        .pointer("/payload/session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    listed.working_directory = meta
        .pointer("/payload/cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    listed.has_turn = stamp.size > meta_bytes as u64;
    listed
}

/// What one rollout ran on and spent, from its bounded tail. The token count
/// is cumulative, so its final line is the conversation total.
fn read_codex_figures(path: &Path) -> Figures {
    let mut figures = Figures::default();
    for line in codex_tail(path, CODEX_FIGURES_TAIL_BYTES) {
        match line.get("type").and_then(Value::as_str) {
            Some("turn_context") => {
                figures.model = line
                    .pointer("/payload/model")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                figures.effort = line
                    .pointer("/payload/effort")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Some("event_msg")
                if line.pointer("/payload/type").and_then(Value::as_str) == Some("token_count") =>
            {
                let Some(total) = line.pointer("/payload/info/total_token_usage") else {
                    continue;
                };
                let count = |key: &str| total.get(key).and_then(Value::as_u64).unwrap_or(0);
                figures.usage = Some(TokenUsage {
                    input_tokens: count("input_tokens"),
                    cached_input_tokens: count("cached_input_tokens"),
                    output_tokens: count("output_tokens"),
                });
            }
            _ => {}
        }
    }
    figures
}

/// The complete JSON lines from the file's final `bytes`. A rollout can grow
/// to tens of megabytes, but a page needs only the last turn's figures.
fn codex_tail(path: &Path, bytes: u64) -> Vec<Value> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(size) = file.metadata().map(|metadata| metadata.len()) else {
        return Vec::new();
    };
    let start = size.saturating_sub(bytes);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut tail = vec![0; usize::try_from(size - start).unwrap_or_default()];
    if file.read_exact(&mut tail).is_err() {
        return Vec::new();
    }
    tail.split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

/// Claude Code's transcripts: `<home>/projects/<slug>/<session id>.jsonl`,
/// one JSON object per line, where the slug is the working directory the
/// conversation ran in with every character other than an ASCII letter or
/// digit made a `-`. The slug is lossy, so the directory is read off the
/// transcript rather than out of its own path.
struct ClaudeConversations {
    /// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    home: PathBuf,
    /// What each transcript held when it was last read, by internal session
    /// id — which is the stem of its file.
    cache: Mutex<HashMap<String, Cached>>,
}

/// One transcript as it was last read.
struct Cached {
    path: PathBuf,
    /// What the file stood at when the reads below were made. A file that
    /// still stands there has nothing in it that has not been read.
    stamp: Stamp,
    /// What the file says as far as a listing reads it.
    listed: Listed,
    /// What the whole file says, for a row of a page: read when a page first
    /// asks for it, and never on a listing.
    figures: Option<Figures>,
}

/// What a file is read by: its size and its modification time.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: Option<SystemTime>,
}

/// What a listing shows of one conversation.
struct Listed {
    working_directory: String,
    title: String,
    /// Whether the transcript holds a turn at all. One that holds none is a
    /// conversation nobody had, and a listing leaves it out.
    has_turn: bool,
    last_activity_at: String,
}

impl ClaudeConversations {
    fn new(home: PathBuf) -> Self {
        Self {
            home,
            cache: Mutex::default(),
        }
    }
}

impl StoredConversations for ClaudeConversations {
    fn conversations(&self) -> Vec<OutsideSessionDto> {
        let mut cache = self.cache.lock().expect("the transcript cache lock");
        let mut sessions = Vec::new();
        let mut found: HashSet<String> = HashSet::new();
        for (id, path, stamp) in transcripts(&self.home) {
            let fresh = cache
                .get(&id)
                .is_some_and(|cached| cached.path == path && cached.stamp == stamp);
            if !fresh {
                cache.insert(
                    id.clone(),
                    Cached {
                        listed: read_listed(&path, &stamp),
                        path,
                        stamp,
                        figures: None,
                    },
                );
            }
            let cached = &cache[&id];
            if cached.listed.has_turn {
                sessions.push(OutsideSessionDto {
                    agent_id: String::new(),
                    internal_session_id: id.clone(),
                    working_directory: cached.listed.working_directory.clone(),
                    last_activity_at: cached.listed.last_activity_at.clone(),
                    first_prompt: cached.listed.title.clone(),
                });
            }
            found.insert(id);
        }
        // A conversation whose file is gone is gone: nothing keeps its reads.
        cache.retain(|id, _| found.contains(id));
        sessions
    }

    fn figures(&self, internal_session_id: &str) -> Option<Figures> {
        let mut cache = self.cache.lock().expect("the transcript cache lock");
        let cached = cache.get_mut(internal_session_id)?;
        // A file that has grown since the listing is read again whole: what it
        // holds now is what the page answers. What the listing shows is read
        // again with it, because the stamp stands for both reads — one moved on
        // without the other would leave the next listing showing what this file
        // said before, and calling it fresh.
        if let Some(stamp) = stamp_of(&cached.path).filter(|stamp| *stamp != cached.stamp) {
            cached.listed = read_listed(&cached.path, &stamp);
            cached.stamp = stamp;
            cached.figures = None;
        }
        if cached.figures.is_none() {
            cached.figures = Some(read_figures(&cached.path));
        }
        cached.figures.clone()
    }
}

/// Every transcript under `<home>/projects`: the internal session id its file
/// is named after, that file, and the stamp it stands at. The subagent files
/// of a conversation are under a directory of its own name, so they are no
/// conversation of this listing.
fn transcripts(home: &Path) -> Vec<(String, PathBuf, Stamp)> {
    let mut found = Vec::new();
    let Ok(projects) = std::fs::read_dir(home.join("projects")) else {
        return found;
    };
    for project in projects.flatten() {
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(OsStr::to_str) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            let Some(stamp) = file
                .metadata()
                .ok()
                .filter(std::fs::Metadata::is_file)
                .map(stamp_of_metadata)
            else {
                continue;
            };
            found.push((id.to_string(), path, stamp));
        }
    }
    found
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(stamp_of_metadata)
}

fn stamp_of_metadata(metadata: std::fs::Metadata) -> Stamp {
    Stamp {
        size: metadata.len(),
        modified: metadata.modified().ok(),
    }
}

/// What a listing shows of one transcript, read from the start of it: the
/// directory of its first turn, the title it records, and whether it holds a
/// turn at all. The last activity is the file's modification time, which is the
/// moment it last wrote anything.
///
/// Nothing here is read on a budget, so nothing is lost to one: the read stops
/// as soon as all three are known, and goes to the end of the file where they
/// are not. Of the 737 transcripts on this machine on 2026-09-25, half are read
/// inside 660 bytes and the deepest title stands at 358 KiB; the nine that
/// record no title of their own are read whole, the largest of them 1.1 MB, and
/// they go by the first line of their first prompt.
///
/// This is the listing's read alone. What a conversation ran on and what it
/// spent are [`read_figures`], for the rows of one page and no others.
fn read_listed(path: &Path, stamp: &Stamp) -> Listed {
    let mut listed = Listed {
        working_directory: String::new(),
        title: String::new(),
        has_turn: false,
        last_activity_at: stamp.modified.map(moment).unwrap_or_default(),
    };
    let mut prompt = String::new();
    for line in json_lines(path) {
        let kind = line.get("type").and_then(Value::as_str).unwrap_or_default();
        if kind == "ai-title"
            && listed.title.is_empty()
            && let Some(title) = line.get("aiTitle").and_then(Value::as_str)
        {
            listed.title = loose_title(title).unwrap_or_default();
        }
        if kind == "user" || kind == "assistant" {
            listed.has_turn = true;
            if listed.working_directory.is_empty()
                && let Some(cwd) = line.get("cwd").and_then(Value::as_str)
            {
                listed.working_directory = cwd.to_string();
            }
            if prompt.is_empty()
                && kind == "user"
                && let Some(text) = user_text(&line)
            {
                prompt = loose_title(&text).unwrap_or_default();
            }
        }
        if listed.has_turn && !listed.working_directory.is_empty() && !listed.title.is_empty() {
            break;
        }
    }
    if listed.title.is_empty() {
        listed.title = prompt;
    }
    listed
}

/// What one conversation ran on and what it spent, read off the whole file.
///
/// One model request is written over as many lines as it has content blocks,
/// each carrying the same `message.usage`, so the requests are counted once
/// each by `message.id` — a total that counts the lines instead is about three
/// times the truth. The model is the last one the conversation ran, which is
/// the one it is on now.
fn read_figures(path: &Path) -> Figures {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Figures::default();
    };
    let mut model = None;
    let mut requests: HashMap<String, TokenUsage> = HashMap::new();
    for line in lines(&text) {
        if line.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(named) = line.pointer("/message/model").and_then(Value::as_str) {
            model = Some(named.to_string());
        }
        let (Some(id), Some(usage)) = (
            line.pointer("/message/id").and_then(Value::as_str),
            line.pointer("/message/usage"),
        ) else {
            continue;
        };
        requests.insert(id.to_string(), claude_request_usage(usage));
    }
    Figures {
        model,
        effort: None,
        usage: (!requests.is_empty()).then(|| requests.values().sum()),
    }
}

/// The text of one user turn: `message.content` is the prompt itself, or the
/// blocks it was written in. A turn that carries only a tool result holds no
/// text of a person's.
fn user_text(line: &Value) -> Option<String> {
    let content = line.pointer("/message/content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let text: Vec<&str> = content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();
    (!text.is_empty()).then(|| text.join("\n"))
}

/// Every line of a file that is an object, one at a time — so a reader stops
/// where it has seen enough, without holding the whole file in memory.
fn json_lines(path: &Path) -> impl Iterator<Item = Value> {
    let mut reader = File::open(path).ok().map(BufReader::new);
    let mut line = String::new();
    std::iter::from_fn(move || {
        let reader = reader.as_mut()?;
        loop {
            line.clear();
            if matches!(reader.read_line(&mut line), Ok(0) | Err(_)) {
                return None;
            }
            if let Ok(value) = serde_json::from_str(line.trim_end()) {
                return Some(value);
            }
        }
    })
}

/// Every line of a transcript that is an object; one that is not is a line
/// still being written, or a line of something else entirely.
fn lines(text: &str) -> impl Iterator<Item = Value> {
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
}

/// OpenCode's own database, one SQLite file at `<home>/opencode.db`: the
/// `session` table carries one row per conversation, keyed by `id`, with
/// `parent_id` set for a subagent's session and null for a conversation of
/// its own; `message` carries the turns, keyed by `session_id`.
///
/// Nothing here is cached: every call opens the file fresh and read-only, so
/// a listing never answers older than OpenCode's own database and never
/// holds a connection open against a file OpenCode itself may be writing.
struct OpencodeConversations {
    /// `<home>/opencode.db`.
    db: PathBuf,
}

impl OpencodeConversations {
    fn new(db: PathBuf) -> Self {
        Self { db }
    }
}

/// A root session with a message, as a listing shows it.
#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    directory: Option<String>,
    title: Option<String>,
    time_updated: Option<i64>,
}

/// What one session ran on and what it spent, off the `session` row alone.
#[derive(sqlx::FromRow)]
struct FiguresRow {
    model: Option<String>,
    tokens_input: Option<i64>,
    tokens_output: Option<i64>,
}

impl StoredConversations for OpencodeConversations {
    fn conversations(&self) -> Vec<OutsideSessionDto> {
        tokio::runtime::Handle::current().block_on(async {
            let Some(pool) = opencode_connect(&self.db).await else {
                return Vec::new();
            };
            let rows: Vec<SessionRow> = query_as(
                "SELECT id, directory, title, time_updated FROM session \
                 WHERE parent_id IS NULL \
                   AND EXISTS (SELECT 1 FROM message WHERE message.session_id = session.id)",
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
            rows.into_iter()
                .map(|row| OutsideSessionDto {
                    agent_id: String::new(),
                    internal_session_id: row.id,
                    working_directory: row.directory.unwrap_or_default(),
                    last_activity_at: row.time_updated.map(opencode_moment).unwrap_or_default(),
                    first_prompt: row.title.unwrap_or_default(),
                })
                .collect()
        })
    }

    fn figures(&self, internal_session_id: &str) -> Option<Figures> {
        tokio::runtime::Handle::current().block_on(async {
            let pool = opencode_connect(&self.db).await?;
            let found: Option<FiguresRow> =
                query_as("SELECT model, tokens_input, tokens_output FROM session WHERE id = ?")
                    .bind(internal_session_id)
                    .fetch_optional(&pool)
                    .await
                    .ok()?;
            let row = found?;
            Some(Figures {
                model: row.model.as_deref().and_then(opencode_model),
                effort: None,
                usage: Some(TokenUsage {
                    input_tokens: u64::try_from(row.tokens_input.unwrap_or_default())
                        .unwrap_or_default(),
                    cached_input_tokens: 0,
                    output_tokens: u64::try_from(row.tokens_output.unwrap_or_default())
                        .unwrap_or_default(),
                }),
            })
        })
    }
}

/// A read-only pool of one connection to `db`, or `None` where it is missing
/// or this build cannot open it: OpenCode may hold it open at the same
/// moment, so a listing never writes to it, never waits on its lock and
/// never runs a migration over it — a session this daemon cannot read is a
/// session it lists none of, rather than one it fails the whole page for.
async fn opencode_connect(db: &Path) -> Option<sqlx::Pool<Sqlite>> {
    if !db.is_file() {
        return None;
    }
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(db)
                .create_if_missing(false)
                .read_only(true),
        )
        .await
        .inspect_err(|error| {
            tracing::warn!(
                error = %error,
                db = %db.display(),
                "opening the opencode database failed",
            );
        })
        .ok()
}

/// The model of one OpenCode session, off the `model` column's JSON:
/// `{"id": "...", "providerID": "..."}`. Spelled `<providerID>/<id>` — the
/// same way every other OpenCode model is — or `id` alone where the column
/// carries no provider.
fn opencode_model(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    let id = value.get("id").and_then(Value::as_str)?;
    match value.get("providerID").and_then(Value::as_str) {
        Some(provider) if !provider.is_empty() => Some(format!("{provider}/{id}")),
        _ => Some(id.to_string()),
    }
}

/// `time_updated`, epoch milliseconds, as the RFC 3339 every other row's
/// `last_activity_at` is written in.
fn opencode_moment(millis: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn moment(at: SystemTime) -> String {
    DateTime::<Utc>::from(at).to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{CODEX_FIGURES_TAIL_BYTES, codex_tail, opencode_connect, read_codex_figures};

    use serde_json::json;

    /// The figures of a large rollout come from its bounded tail, rather than
    /// the megabytes before its last turn.
    #[test]
    fn a_large_rollouts_figures_read_only_its_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-large.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            "x".repeat((CODEX_FIGURES_TAIL_BYTES * 2) as usize)
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type": "turn_context", "payload": {
                "model": "gpt-5.2-codex", "effort": "high",
            }})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({"type": "event_msg", "payload": {
                "type": "token_count", "info": {"total_token_usage": {
                    "input_tokens": 900, "cached_input_tokens": 700, "output_tokens": 200,
                }},
            }})
        )
        .unwrap();

        let tail = codex_tail(&path, CODEX_FIGURES_TAIL_BYTES);
        assert_eq!(tail.len(), 2);
        let figures = read_codex_figures(&path);
        assert_eq!(figures.model.as_deref(), Some("gpt-5.2-codex"));
        assert_eq!(figures.effort.as_deref(), Some("high"));
        assert_eq!(
            figures.usage.expect("the final token count").input_tokens,
            900
        );
    }

    /// [`opencode_connect`] opens `SQLITE_OPEN_READONLY`: a write over the
    /// connection it hands back is refused, on an ordinary writable database,
    /// with nothing about the file or its directory stopping a write of its
    /// own. A test that instead took away the file's or the directory's write
    /// permission would prove nothing — SQLite itself falls back to a
    /// read-only open when a read-write one is denied, so a connection asked
    /// for read-write and a connection asked for read-only behave alike
    /// there, and the flag between them goes untested.
    #[tokio::test]
    async fn the_connection_refuses_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let setup = sqlx::sqlite::SqlitePoolOptions::new()
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&db)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query("CREATE TABLE session (id TEXT)")
            .execute(&setup)
            .await
            .unwrap();
        setup.close().await;

        let reader = opencode_connect(&db).await.expect("opens the database");
        let wrote = sqlx::query("INSERT INTO session (id) VALUES ('x')")
            .execute(&reader)
            .await;

        assert!(
            wrote.is_err(),
            "a connection opened read-only must refuse a write"
        );
    }
}
