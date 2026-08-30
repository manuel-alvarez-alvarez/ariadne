//! `ariadne events` — everything the daemon does, one line at a time.
//!
//! Two sources behind one vocabulary. The recorded half is `GET /v1/events`:
//! the agent events hooks reported, which is the only history there is. The
//! live half is `GET /v1/events/stream`, the daemon's domain events — every
//! goal, task, session, message and review as it changes, with the agent
//! events among them. So `ariadne events` prints what has happened and
//! `ariadne events -f` goes on printing what happens next, in the same shape:
//! `time · kind · subject · detail`.
//!
//! An agent event is spelled by its own kind (`stop`, `post_tool_use`) whether
//! it arrives from the history or from the stream, so `--kind stop` means one
//! thing in both halves and the two halves read as one list.

use anyhow::Result;
use serde::Serialize;

use ariadne_api::Page;
use ariadne_api::events::{AgentEventDto, EventListQuery};
use ariadne_api::messages::MessageDto;
use ariadne_api::sessions::SessionDto;
use ariadne_api::stream::{
    DeletedDto, DomainEvent, EventStreamQuery, TaskBranchDto, TaskUpdatedDto,
};
use ariadne_api::tasks::TaskDto;
use ariadne_client::{Client, SseEvent};

use super::attention::reason_label;
use super::follow::{self, Next};
use super::{query_path, recipient_label};
use crate::output::{Format, local_time, note, style, view};

/// How many recorded events the snapshot asks for. The daemon caps a page at
/// 200, and a tail wants the recent past rather than all of it.
const SNAPSHOT: i64 = 200;

/// How much of a message body or a title one line carries. A line of a stream
/// has no column to be cut to, but a paragraph pasted into a task description
/// would still be a screenful.
const DETAIL: usize = 100;

/// What `ariadne events` was asked for.
pub struct Filters {
    pub goal: Option<String>,
    pub task: Option<String>,
    pub session: Option<String>,
    pub kinds: Vec<String>,
}

impl Filters {
    /// Whether anything was asked for at all — which is what tells an empty
    /// system from a filter nothing matched.
    fn narrowed(&self) -> bool {
        self.goal.is_some()
            || self.task.is_some()
            || self.session.is_some()
            || !self.kinds.is_empty()
    }

    /// Whether a line passes the filters this command applies itself.
    ///
    /// `goal` and `task` are the stream's own parameters and are left to the
    /// daemon; the snapshot has no goal filter, so [`snapshot`] resolves that
    /// one into ids before it gets here.
    fn keeps(&self, line: &Line) -> bool {
        let kind = self.kinds.is_empty() || self.kinds.contains(&line.kind);
        let session = self
            .session
            .as_ref()
            .is_none_or(|s| line.session.as_deref() == Some(s));
        kind && session
    }
}

/// One event as a line, whichever half of the list it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Line {
    /// RFC 3339, as the daemon spells it.
    at: String,
    /// The event kind, in the daemon's own vocabulary.
    kind: String,
    /// What the event is about: the id of the entity it happened to.
    subject: String,
    /// The rest of it in a phrase, or empty where the kind and subject are the
    /// whole story (a deletion).
    detail: String,
    /// The session behind the event, where there is one — what `--session`
    /// narrows to, and never printed on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    /// The lifecycle status `detail` ends in `[brackets]` with, for `render`
    /// to paint — set only where `detail` was built from one (`task_line`,
    /// the goal arm of [`domain_line`], `session_line`), and skipped from
    /// json since `detail` already spells the same word.
    #[serde(skip)]
    status: Option<String>,
}

impl Line {
    /// `time · kind · subject · detail`, in local time, with an empty detail
    /// leaving no dangling separator behind it.
    ///
    /// `color` off is the plain line this command has always printed — the
    /// shape every fixture below still pins byte for byte. `color` on paints
    /// three of the four parts: the time dims to context, the kind turns
    /// bold in `TITLE`'s style (a kind is what the row is about, the same job
    /// a table's title column does — `HEADING` is for the section breaks a
    /// stream running past never has), and where `detail` ends in the
    /// lifecycle status the daemon just set, that word picks up its colour
    /// and its glyph together — never the glyph alone, since the plain line
    /// has never carried one and still must not.
    fn render(&self, color: bool) -> String {
        let mut out = format!(
            "{} · {} · {}",
            style::paint(color, style::META, &local_time(&self.at)),
            style::paint(color, style::TITLE, &self.kind),
            self.subject
        );
        if !self.detail.is_empty() {
            out.push_str(" · ");
            out.push_str(&self.painted_detail(color));
        }
        out
    }

    /// `detail`, with its `[status]` painted where there is one to paint and
    /// `color` says to. A word `style::status` does not recognise — an
    /// unfamiliar status, a transition's `from → to` — comes back unstyled
    /// and glyph-less, so it is written back exactly as it sat in `detail`.
    ///
    /// The bracket painted is the *last* one that matches, never the first: a
    /// goal or task title is free text a person wrote, and `titled` appends
    /// `[state]` after it rather than escaping what is already there, so a
    /// title that happens to end in the same words in brackets — `Investigate
    /// [under_review]`, status `under_review` — must not have its own bracket
    /// mistaken for the one the daemon actually set.
    fn painted_detail(&self, color: bool) -> String {
        let Some(word) = &self.status else {
            return self.detail.clone();
        };
        if !color {
            return self.detail.clone();
        }
        let bracket = format!("[{word}]");
        let Some(at) = self.detail.rfind(&bracket) else {
            return self.detail.clone();
        };
        let (style, glyph) = style::status(word);
        let glyphed = match glyph {
            Some(glyph) => format!("{glyph} {word}"),
            None => word.clone(),
        };
        let painted = style::paint(true, style, &glyphed);
        format!(
            "{}[{painted}]{}",
            &self.detail[..at],
            &self.detail[at + bracket.len()..]
        )
    }
}

pub async fn run(client: &Client, filters: Filters, follow_it: bool, format: Format) -> Result<()> {
    let recorded = snapshot(client, &filters).await?;
    let color = view().color;
    match format {
        Format::Json => {
            for line in &recorded {
                print_jsonl(line)?;
            }
        }
        Format::Table => {
            for line in &recorded {
                println!("{}", line.render(color));
            }
            // An empty list under a filter is not an empty system, and saying
            // so would send the reader looking for events that are right
            // there. Nothing at all is said when a follow is about to start:
            // the list is not over, it has not begun.
            if recorded.is_empty() && !follow_it {
                note(match filters.narrowed() {
                    true => "no recorded events match that filter",
                    false => "no events recorded yet",
                });
            }
        }
    }
    if !follow_it {
        return Ok(());
    }

    let path = query_path(
        "/v1/events/stream",
        &EventStreamQuery {
            goal: filters.goal.clone(),
            task: filters.task.clone(),
        },
    )?;
    follow::frames_reconnecting(client, &path, |frame| {
        for line in frame_lines(&frame) {
            if !filters.keeps(&line) {
                continue;
            }
            match format {
                Format::Json => print_jsonl(&line)?,
                Format::Table => println!("{}", line.render(color)),
            }
        }
        Ok(Next::Go)
    })
    .await
}

/// One line of JSON per event, which is what `--format json` means for
/// something that never ends: a pretty document could only be printed once the
/// last event had arrived, and there is no last event.
fn print_jsonl(line: &Line) -> Result<()> {
    println!("{}", serde_json::to_string(line)?);
    Ok(())
}

/// `GET /v1/events`' two query structs as the one query string it takes.
#[derive(Serialize)]
struct SnapshotQuery {
    #[serde(flatten)]
    filters: EventListQuery,
    #[serde(flatten)]
    page: Page,
}

/// The recorded events, filtered as asked.
async fn snapshot(client: &Client, filters: &Filters) -> Result<Vec<Line>> {
    let path = query_path(
        "/v1/events",
        &SnapshotQuery {
            filters: EventListQuery {
                session: filters.session.clone(),
                task: filters.task.clone(),
            },
            page: Page {
                after: None,
                limit: Some(SNAPSHOT),
            },
        },
    )?;
    let events: Vec<AgentEventDto> = client.get_json(&path).await?;
    // `GET /v1/events` takes no goal, so a goal filter is resolved into the
    // ids that belong to it and applied here. Only the snapshot needs this:
    // the stream filters by goal itself.
    let scope = match &filters.goal {
        Some(goal) => Some(GoalScope::fetch(client, goal).await?),
        None => None,
    };
    Ok(events
        .iter()
        .map(agent_line)
        .filter(|line| scope.as_ref().is_none_or(|s| s.holds(line)))
        .filter(|line| filters.keeps(line))
        .collect())
}

/// The tasks and sessions of one goal: what makes a recorded agent event that
/// goal's, since an agent event names only its session and its task.
struct GoalScope {
    tasks: Vec<String>,
    sessions: Vec<String>,
}

impl GoalScope {
    async fn fetch(client: &Client, goal: &str) -> Result<Self> {
        let tasks: Vec<TaskDto> = client.get_json(&format!("/v1/tasks?goal={goal}")).await?;
        let sessions: Vec<SessionDto> = client
            .get_json(&format!("/v1/sessions?goal={goal}"))
            .await?;
        Ok(Self {
            tasks: tasks.into_iter().map(|t| t.id).collect(),
            sessions: sessions.into_iter().map(|s| s.id).collect(),
        })
    }

    fn holds(&self, line: &Line) -> bool {
        let session = line.session.as_ref();
        self.tasks.contains(&line.subject)
            || session.is_some_and(|s| self.sessions.contains(s))
            || self.sessions.contains(&line.subject)
    }
}

/// The lines one stream frame is worth: one for a domain event, none for the
/// control frames, which describe the stream rather than the daemon's work.
fn frame_lines(frame: &SseEvent) -> Vec<Line> {
    match frame.event.as_str() {
        // The heartbeat only says the daemon is still there, which a tail can
        // see for itself.
        "heartbeat" => vec![],
        // Events were dropped for this connection and it is about to close;
        // `frames_reconnecting` picks the stream up again.
        "resync" => {
            note("the daemon dropped events for this connection — some are missing");
            vec![]
        }
        _ => domain_event(frame)
            .as_ref()
            .map(domain_line)
            .into_iter()
            .collect(),
    }
}

/// A frame put back together as the tagged union the wire splits in two: the
/// kind is the SSE event name, the payload is the bare DTO.
///
/// A frame this build cannot read — a kind added to the daemon since — is
/// dropped rather than printed half-understood: `--format json` promises
/// objects of one shape, and a tail that guessed would be inventing lines.
fn domain_event(frame: &SseEvent) -> Option<DomainEvent> {
    let data: serde_json::Value = serde_json::from_str(&frame.data).ok()?;
    serde_json::from_value(serde_json::json!({"event": frame.event, "data": data})).ok()
}

/// One recorded agent event as a line: its own kind, the session that reported
/// it, and which agent that was.
fn agent_line(e: &AgentEventDto) -> Line {
    Line {
        at: e.created_at.clone(),
        kind: e.kind.clone(),
        subject: e
            .session_id
            .clone()
            .or_else(|| e.task_id.clone())
            .unwrap_or_else(|| "-".into()),
        detail: e.agent_kind.clone().unwrap_or_default(),
        session: e.session_id.clone(),
        status: None,
    }
}

/// One domain event as a line.
fn domain_line(event: &DomainEvent) -> Line {
    let kind = event.kind().to_string();
    match event {
        DomainEvent::GoalCreated(g) | DomainEvent::GoalUpdated(g) => Line {
            at: g.updated_at.clone(),
            kind,
            subject: g.id.clone(),
            detail: titled(&g.title, g.status.as_str()),
            session: None,
            status: Some(g.status.as_str().to_string()),
        },
        DomainEvent::TaskCreated(t) => task_line(kind, t, t.status.as_str().to_string()),
        DomainEvent::TaskUpdated(TaskUpdatedDto { task, transition }) => task_line(
            kind,
            task,
            match transition {
                Some(t) => format!("{} → {}", t.from_status, t.to_status),
                None => task.status.as_str().to_string(),
            },
        ),
        DomainEvent::TaskBranchUpdated(TaskBranchDto {
            task_id,
            branch,
            head,
            ..
        }) => Line {
            at: now(),
            kind,
            subject: task_id.clone(),
            detail: format!("{branch} @ {}", short(head)),
            session: None,
            status: None,
        },
        DomainEvent::MessageCreated(m) => message_line(kind, m),
        DomainEvent::ReviewCreated(r) => Line {
            at: r.created_at.clone(),
            kind,
            subject: r.task_id.clone(),
            detail: format!("round {} {}", r.round, r.verdict.as_str()),
            session: r.session_id.clone(),
            status: None,
        },
        DomainEvent::SessionCreated(s) | DomainEvent::SessionUpdated(s) => session_line(kind, s),
        DomainEvent::AgentEvent(e) => agent_line(e),
        DomainEvent::ProfileCreated(p) | DomainEvent::ProfileUpdated(p) => Line {
            at: now(),
            kind,
            subject: p.id.clone(),
            detail: format!("{} ({})", p.name, p.role.as_str()),
            session: None,
            status: None,
        },
        DomainEvent::RepositoryCreated(r) | DomainEvent::RepositoryUpdated(r) => Line {
            at: now(),
            kind,
            subject: r.id.clone(),
            detail: r.path.clone(),
            session: None,
            status: None,
        },
        DomainEvent::GoalDeleted(DeletedDto { id })
        | DomainEvent::ProfileDeleted(DeletedDto { id })
        | DomainEvent::RepositoryDeleted(DeletedDto { id }) => Line {
            at: now(),
            kind,
            subject: id.clone(),
            detail: String::new(),
            session: None,
            status: None,
        },
    }
}

fn task_line(kind: String, t: &TaskDto, state: String) -> Line {
    Line {
        at: t.updated_at.clone(),
        kind,
        subject: t.id.clone(),
        detail: titled(&t.title, &state),
        session: None,
        status: Some(state),
    }
}

fn message_line(kind: String, m: &MessageDto) -> Line {
    let to = match &m.recipient {
        Some(recipient) => format!(" → {}", recipient_label(recipient)),
        None => String::new(),
    };
    Line {
        at: m.created_at.clone(),
        kind,
        subject: m.task_id.clone().unwrap_or_else(|| m.goal_id.clone()),
        detail: brief(&format!("{}{to}: {}", m.author_role.as_str(), m.body)),
        session: m.author_session_id.clone(),
        status: None,
    }
}

fn session_line(kind: String, s: &SessionDto) -> Line {
    let mut detail = format!(
        "{} {} [{}]",
        s.role.as_str(),
        s.agent_kind.as_str(),
        s.status.as_str()
    );
    // Why the daemon wants a person for it, when it does: the one thing about
    // a session that is not in its status.
    if let Some(reason) = s.attention_reason {
        detail.push_str(" · ");
        detail.push_str(reason_label(reason));
    }
    Line {
        at: s
            .ended_at
            .clone()
            .or_else(|| s.last_activity_at.clone())
            .unwrap_or_else(|| s.created_at.clone()),
        kind,
        subject: s.id.clone(),
        detail,
        session: Some(s.id.clone()),
        status: Some(s.status.as_str().to_string()),
    }
}

/// `title [state]`: what a goal or a task is called, and where it now stands.
fn titled(title: &str, state: &str) -> String {
    format!("{} [{state}]", brief(title))
}

/// The first line of the head of a commit sha, which is how a commit is
/// named anywhere it is not being fetched.
fn short(sha: &str) -> String {
    sha.chars().take(12).collect()
}

/// Free text as one line of at most [`DETAIL`] characters. Counted in
/// characters, like every other cut in the CLI, so an accent cannot be split
/// in half.
fn brief(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= DETAIL {
        return flat;
    }
    flat.chars().take(DETAIL - 1).chain(['…']).collect()
}

/// When a live event with nothing of its own to go by happened: now.
///
/// Most DTOs carry the stamp of the change that produced them, and those are
/// used. The few that do not — a deletion, a branch that moved — are stamped
/// with their arrival, which for a stream being watched is the same instant.
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::goals::GoalDto;
    use ariadne_api::tasks::TaskTransitionDto;
    use ariadne_core::{AttentionReason, AuthorRole, RecipientKind, SessionStatus, TaskStatus};

    use crate::commands::fixtures;

    /// A fixed stamp, so a line reads the same wherever it is rendered from.
    const AT: &str = "2026-08-18T11:00:00Z";

    fn rendered(line: &Line) -> String {
        line.render(false)
            .replace(&local_time(AT), "<time>")
            .replace(&local_time(&line.at), "<time>")
    }

    fn frame(event: &str, data: serde_json::Value) -> SseEvent {
        SseEvent {
            event: event.into(),
            data: data.to_string(),
            id: Some("01FRAME".into()),
        }
    }

    fn goal() -> GoalDto {
        GoalDto {
            updated_at: AT.into(),
            ..fixtures::goal("01GOAL", "Ship the board")
        }
    }

    fn task() -> TaskDto {
        TaskDto {
            title: "Wire the screen".into(),
            status: TaskStatus::UnderReview,
            updated_at: AT.into(),
            ..fixtures::task("01TASK", "01GOAL")
        }
    }

    fn session() -> SessionDto {
        SessionDto {
            status: SessionStatus::Running,
            last_activity_at: Some(AT.into()),
            ..fixtures::session("01SESS", "01GOAL", Some("01TASK"))
        }
    }

    /// The shape of every line: when, what kind, what it happened to, and the
    /// one phrase that says the rest.
    #[test]
    fn an_event_reads_as_time_kind_subject_and_detail() {
        assert_eq!(
            rendered(&domain_line(&DomainEvent::GoalCreated(goal()))),
            "<time> · goal_created · 01GOAL · Ship the board [active]"
        );
        assert_eq!(
            rendered(&domain_line(&DomainEvent::TaskCreated(task()))),
            "<time> · task_created · 01TASK · Wire the screen [under_review]"
        );
    }

    /// With colour on, the kind turns bold and the lifecycle status inside
    /// the detail's brackets carries its colour and its glyph together; with
    /// colour off nothing changes at all — no escapes, no glyph — which is
    /// the same plain line every other fixture in this module pins.
    #[test]
    fn a_status_detail_is_painted_only_when_colour_is_on() {
        let line = domain_line(&DomainEvent::TaskCreated(task()));
        assert_eq!(
            rendered(&line),
            "<time> · task_created · 01TASK · Wire the screen [under_review]"
        );
        assert!(!line.render(false).contains('\u{1b}'));

        let painted = line.render(true);
        let (sty, glyph) = style::status("under_review");
        assert!(
            painted.contains(&style::paint(true, style::TITLE, "task_created")),
            "{painted}"
        );
        assert!(
            painted.contains(&style::paint(
                true,
                sty,
                &format!("{} under_review", glyph.expect("under_review has a glyph"))
            )),
            "{painted}"
        );
    }

    /// A title is free text a person wrote, and can end in something that
    /// reads exactly like the status in brackets. The bracket `titled`
    /// actually appended — always the rightmost one — is what gets painted;
    /// the title's own is left exactly as it came, in the plain render and
    /// in the painted one alike.
    #[test]
    fn a_title_that_echoes_the_status_does_not_steal_the_paint() {
        let echoing = TaskDto {
            title: "Investigate [under_review]".into(),
            ..task()
        };
        let line = domain_line(&DomainEvent::TaskCreated(echoing));
        assert_eq!(
            rendered(&line),
            "<time> · task_created · 01TASK · Investigate [under_review] [under_review]"
        );

        let painted = line.render(true);
        let (sty, glyph) = style::status("under_review");
        let glyphed = style::paint(
            true,
            sty,
            &format!("{} under_review", glyph.expect("under_review has a glyph")),
        );
        assert!(painted.ends_with(&format!("[{glyphed}]")), "{painted}");
        assert!(
            painted.contains("Investigate [under_review] ["),
            "the title's own bracket is untouched: {painted}"
        );
    }

    /// A transition is the interesting half of a task update, so it takes the
    /// place of the status a plain edit would have shown.
    #[test]
    fn a_task_update_says_which_way_it_moved() {
        let moved = DomainEvent::TaskUpdated(TaskUpdatedDto {
            task: task(),
            transition: Some(TaskTransitionDto {
                id: "01TR".into(),
                from_status: "in_progress".into(),
                to_status: "under_review".into(),
                actor: "engineer".into(),
                reason: None,
                created_at: AT.into(),
            }),
        });
        assert_eq!(
            rendered(&domain_line(&moved)),
            "<time> · task_updated · 01TASK · Wire the screen [in_progress → under_review]"
        );

        let edited = DomainEvent::TaskUpdated(TaskUpdatedDto {
            task: task(),
            transition: None,
        });
        assert_eq!(
            rendered(&domain_line(&edited)),
            "<time> · task_updated · 01TASK · Wire the screen [under_review]"
        );
    }

    /// A session line carries what a session is — role, agent, status — plus
    /// the one thing that is not in its status: whether it wants a person.
    #[test]
    fn a_session_line_carries_its_attention_beside_its_status() {
        assert_eq!(
            rendered(&domain_line(&DomainEvent::SessionUpdated(session()))),
            "<time> · session_updated · 01SESS · engineer claude_code [running]"
        );
        let waiting = SessionDto {
            attention_reason: Some(AttentionReason::WaitingInput),
            ..session()
        };
        assert_eq!(
            rendered(&domain_line(&DomainEvent::SessionUpdated(waiting))),
            "<time> · session_updated · 01SESS · engineer claude_code [running] · waiting for input"
        );
    }

    /// A message is who said it, to whom, and what — the same three things
    /// `task messages` prints, on one line.
    #[test]
    fn a_message_line_names_its_author_and_its_addressee() {
        let mut message = MessageDto {
            id: "01MSG".into(),
            goal_id: "01GOAL".into(),
            task_id: Some("01TASK".into()),
            author_role: AuthorRole::Engineer,
            author_session_id: Some("01SESS".into()),
            recipient: None,
            body: "rebased onto main".into(),
            created_at: AT.into(),
        };
        assert_eq!(
            rendered(&domain_line(&DomainEvent::MessageCreated(message.clone()))),
            "<time> · message_created · 01TASK · engineer: rebased onto main"
        );
        message.recipient = Some(ariadne_api::messages::MessageRecipientDto {
            kind: RecipientKind::User,
            profile_id: None,
            profile_name: None,
        });
        assert_eq!(
            rendered(&domain_line(&DomainEvent::MessageCreated(message))),
            "<time> · message_created · 01TASK · engineer → user: rebased onto main"
        );
    }

    /// A deletion is its kind and its id; there is nothing left to describe,
    /// and a trailing separator would say there was.
    #[test]
    fn a_line_with_nothing_to_add_ends_at_its_subject() {
        let line = domain_line(&DomainEvent::GoalDeleted(DeletedDto {
            id: "01GOAL".into(),
        }));
        assert_eq!(rendered(&line), "<time> · goal_deleted · 01GOAL");
    }

    /// A recorded agent event and the same event arriving live read
    /// identically — kind, session and agent — so the history and the tail are
    /// one list and `--kind stop` means one thing in both.
    #[test]
    fn an_agent_event_reads_the_same_recorded_as_it_does_live() {
        let event = AgentEventDto {
            id: "01EV".into(),
            session_id: Some("01SESS".into()),
            task_id: Some("01TASK".into()),
            agent_kind: Some("claude_code".into()),
            kind: "stop".into(),
            payload: serde_json::json!({}),
            created_at: AT.into(),
        };
        let expected = "<time> · stop · 01SESS · claude_code";
        assert_eq!(rendered(&agent_line(&event)), expected);
        assert_eq!(
            rendered(&domain_line(&DomainEvent::AgentEvent(event))),
            expected
        );
    }

    /// A paragraph pasted into a description is still one line, cut in
    /// characters and marked as cut.
    #[test]
    fn free_text_is_flattened_to_one_line_and_cut() {
        assert_eq!(brief("two\nlines"), "two lines");
        let long = "á".repeat(DETAIL + 10);
        let cut = brief(&long);
        assert_eq!(cut.chars().count(), DETAIL);
        assert!(cut.ends_with('…'), "{cut}");
    }

    /// The wire splits the union in two; a frame is only a line once both
    /// halves are put back together, and a kind this build does not know is
    /// dropped rather than half-read.
    #[test]
    fn a_frame_becomes_the_event_its_two_halves_describe() {
        let created = frame("goal_created", serde_json::to_value(goal()).unwrap());
        assert_eq!(
            frame_lines(&created)
                .iter()
                .map(rendered)
                .collect::<Vec<_>>(),
            ["<time> · goal_created · 01GOAL · Ship the board [active]"]
        );
        assert!(frame_lines(&frame("goal_reticulated", serde_json::json!({}))).is_empty());
    }

    /// The control frames describe the connection, not the daemon's work: a
    /// heartbeat is not an event, and neither is being told to resync.
    #[test]
    fn the_control_frames_are_not_events() {
        let beat = frame("heartbeat", serde_json::json!({"version": "0.4.0"}));
        assert!(frame_lines(&beat).is_empty());
        assert!(frame_lines(&frame("resync", serde_json::json!({"missed": 3}))).is_empty());
    }

    fn line(kind: &str, session: Option<&str>) -> Line {
        Line {
            at: AT.into(),
            kind: kind.into(),
            subject: "01X".into(),
            detail: String::new(),
            session: session.map(str::to_owned),
            status: None,
        }
    }

    /// `--kind` is a list of alternatives and `--session` narrows to one
    /// agent; an event with no session at all is not that agent's.
    #[test]
    fn the_filters_this_command_applies_itself_narrow_and_never_widen() {
        let all = Filters {
            goal: None,
            task: None,
            session: None,
            kinds: vec![],
        };
        assert!(all.keeps(&line("stop", None)));

        let kinds = Filters {
            kinds: vec!["stop".into(), "task_updated".into()],
            ..all
        };
        assert!(kinds.keeps(&line("stop", None)));
        assert!(kinds.keeps(&line("task_updated", None)));
        assert!(!kinds.keeps(&line("session_updated", None)));

        let session = Filters {
            goal: None,
            task: None,
            session: Some("01SESS".into()),
            kinds: vec![],
        };
        assert!(session.keeps(&line("stop", Some("01SESS"))));
        assert!(!session.keeps(&line("stop", Some("01OTHER"))));
        assert!(!session.keeps(&line("goal_created", None)));
    }

    /// An empty answer means one thing when nothing was asked for and another
    /// when something was: "no events yet" under a filter would send a reader
    /// looking for events that are right there.
    #[test]
    fn a_filter_is_what_tells_an_empty_answer_from_an_empty_system() {
        let none = Filters {
            goal: None,
            task: None,
            session: None,
            kinds: vec![],
        };
        assert!(!none.narrowed());
        let by_kind = Filters {
            kinds: vec!["stop".into()],
            ..none
        };
        assert!(by_kind.narrowed());
        let by_goal = Filters {
            goal: Some("01GOAL".into()),
            ..by_kind
        };
        assert!(by_goal.narrowed());
    }

    /// `--format json` is one object per line — a stream has no last event to
    /// close a document after — and the object is the four fields of the line,
    /// with the session behind it where there is one.
    #[test]
    fn json_output_is_one_object_per_event() {
        let goal = serde_json::to_string(&domain_line(&DomainEvent::GoalCreated(goal()))).unwrap();
        assert!(!goal.contains('\n'), "{goal}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&goal).unwrap(),
            serde_json::json!({
                "at": AT,
                "kind": "goal_created",
                "subject": "01GOAL",
                "detail": "Ship the board [active]",
            })
        );
        let live = domain_line(&DomainEvent::SessionUpdated(session()));
        assert_eq!(
            serde_json::to_value(live).unwrap()["session"],
            serde_json::json!("01SESS")
        );
    }
}
