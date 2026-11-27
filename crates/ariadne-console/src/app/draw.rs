//! The whole screen, drawn from the state: the transcript of turns, the
//! plan, the permission or the question above the input, the suggestion
//! list, the input box, the status row and the footer.

use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use ariadne_api::trajectory::{
    CompactionStatus, ContentBlockDto, MAIN_LANE, NoteSeverity, PermissionState, PermissionStepDto,
    QuestionState, StepDto, ToolCallStatus, TurnDto, TurnState,
};

use crate::markdown;
use crate::theme::{
    self, AGENT, ASK, DAEMON, DIM, FAIL, FRAME, PICKED, PROMPT, QUESTION, SPINNER, TOOL, USER,
};
use crate::trajectory::{kind, text};
use crate::wrap;

use super::{App, Detail, Item, LIVE_STEPS, Zone, detail, is_live, lane_of, status};

/// The marks of an item that is closed and of one that is open.
pub(super) const FOLDED: &str = "▸ ";
pub(super) const UNFOLDED: &str = "▾ ";
/// The marks of a turn or a step that was cancelled, and of a question.
const CANCELLED: &str = "⊘";
const WAITS: &str = "?";
const NOTE: &str = "·";
/// How far a step row is indented under its turn, and its body under it.
/// Each level of a subagent adds [`DEPTH_INDENT`].
const STEP_INDENT: &str = "    ";
const BODY_INDENT: &str = "      ";
const DEPTH_INDENT: &str = "  ";
/// The columns the kind of a step row is padded to.
const KIND_WIDTH: usize = 10;
/// The label in the rule over a pending permission.
const PERMISSION_LABEL: &str = "permission";

/// One line of the transcript: what it draws, the item a click on it opens,
/// and whether it is the line the cursor marks for that item.
struct Row {
    line: Line<'static>,
    item: Option<Item>,
    head: bool,
}

impl Row {
    fn of(line: Line<'static>, item: Option<&Item>) -> Self {
        Self {
            line,
            item: item.cloned(),
            head: false,
        }
    }

    fn head(line: Line<'static>, item: &Item) -> Self {
        Self {
            line,
            item: Some(item.clone()),
            head: true,
        }
    }
}

impl App {
    /// Draw the whole screen.
    pub fn render(&self, frame: &mut Draw) {
        let area = frame.area();
        let width = usize::from(area.width);
        let input = self.input.height(area.width).min(area.height);
        let footer = u16::from(area.height > input);
        let status_row = u16::from(area.height > input + footer);
        let below = input + footer + status_row;
        let most = (area.height.saturating_sub(below) / 2) as usize;
        let attention = self.attention_lines(width, most);
        let attention_rows = attention.len() as u16;
        let plan = match self.plan() {
            Some(plan) => {
                let most = match self.plan_open {
                    true => most.saturating_sub(attention.len()).max(1),
                    false => 1,
                };
                let room = usize::from(area.height.saturating_sub(below + attention_rows + 1));
                let (lines, scroll) = status::plan_lines(
                    plan,
                    self.plan_open,
                    self.plan_scroll.get(),
                    width,
                    most.min(room),
                );
                self.plan_scroll.set(scroll);
                lines
            }
            None => Vec::new(),
        };
        let plan_rows = plan.len() as u16;
        let room = area
            .height
            .saturating_sub(below + attention_rows + plan_rows)
            .saturating_sub(1);
        let suggested = self.suggestion_rows(room);
        let transcript = area
            .height
            .saturating_sub(below + attention_rows + plan_rows + suggested);

        let mut y = area.y;
        let mut next = |rows: u16| {
            let rect = Rect::new(area.x, y, area.width, rows);
            y += rows;
            rect
        };
        let transcript_area = next(transcript);
        let plan_area = next(plan_rows);
        let attention_area = next(attention_rows);
        let suggestions_area = next(suggested);
        let input_area = next(input);
        let status_area = next(status_row);
        let footer_area = next(footer);

        self.draw_transcript(frame, transcript_area);
        self.layout.borrow_mut().plan = plan_area;
        frame.render_widget(Paragraph::new(Text::from(plan)), plan_area);
        frame.render_widget(Paragraph::new(Text::from(attention)), attention_area);
        self.draw_suggestions(frame, suggestions_area);
        self.input
            .draw(frame, input_area, self.hint(), self.zone == Zone::Input);
        if let Some(view) = &self.view {
            frame.render_widget(
                Paragraph::new(status::status_line(
                    view.session(),
                    self.model.as_deref(),
                    width,
                )),
                status_area,
            );
        }
        frame.render_widget(Paragraph::new(self.footer(width)), footer_area);
    }

    fn draw_transcript(&self, frame: &mut Draw, area: Rect) {
        let rows = self.rows(usize::from(area.width));
        let height = usize::from(area.height);
        let bottom = rows.len().saturating_sub(height);
        let mut top = self.scroll.get().unwrap_or(bottom).min(bottom);
        if self.reveal.replace(false)
            && let Some(cursor) = &self.cursor
            && let Some(at) = rows
                .iter()
                .position(|row| row.head && row.item.as_ref() == Some(cursor))
        {
            if at < top {
                top = at;
            } else if at >= top + height {
                top = at + 1 - height;
            }
            self.scroll.set((top < bottom).then_some(top));
        }
        let marked = (self.zone == Zone::Transcript)
            .then_some(self.cursor.as_ref())
            .flatten();
        let lines: Vec<Line<'static>> = rows
            .iter()
            .skip(top)
            .take(height)
            .map(|row| match row.head && row.item.as_ref() == marked {
                true => row.line.clone().patch_style(Modifier::REVERSED),
                false => row.line.clone(),
            })
            .collect();
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
        let mut layout = self.layout.borrow_mut();
        layout.area = area;
        layout.top = top;
        layout.total = rows.len();
        layout.items = rows.into_iter().map(|row| row.item).collect();
    }

    /// Every line of the transcript at `width`.
    fn rows(&self, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.turns().is_empty() && self.pending.is_empty() {
            rows.push(Row::of(
                Line::styled("no turn yet: what you type below is the first prompt", DIM),
                None,
            ));
        }
        for turn in self.turns() {
            self.turn_rows(turn, width, &mut rows);
        }
        for prompt in &self.pending {
            for (at, part) in wrap::wrap(prompt, width.saturating_sub(2))
                .into_iter()
                .enumerate()
            {
                let lead = if at == 0 { theme::USER_MARKER } else { "  " };
                rows.push(Row::of(
                    Line::from(vec![Span::styled(lead, USER), Span::styled(part, PROMPT)]),
                    None,
                ));
            }
            rows.push(Row::of(
                Line::styled(format!("  {}", theme::QUEUED), DIM),
                None,
            ));
        }
        for notice in &self.notices {
            for part in wrap::wrap(notice, width.saturating_sub(2)) {
                rows.push(Row::of(
                    Line::from(vec![
                        Span::styled(theme::ERROR_MARKER, FAIL),
                        Span::styled(part, FAIL),
                    ]),
                    None,
                ));
            }
        }
        rows
    }

    /// A turn: its prompt, one activity line, the step rows it shows, and
    /// its answer.
    fn turn_rows(&self, turn: &TurnDto, width: usize, rows: &mut Vec<Row>) {
        let item = Item::Turn(turn.id.clone());
        let open = self.open.contains(&item);
        let body = width.saturating_sub(2);
        match &turn.prompt {
            Some(prompt) if super::from_console(turn) => {
                let said = prompt.text.clone().unwrap_or_else(|| text(&prompt.content));
                for (at, part) in wrap::wrap(said.trim_end(), body).into_iter().enumerate() {
                    let lead = if at == 0 { theme::USER_MARKER } else { "  " };
                    rows.push(Row::of(
                        Line::from(vec![Span::styled(lead, USER), Span::styled(part, PROMPT)]),
                        Some(&item),
                    ));
                }
            }
            Some(prompt) => {
                // A daemon's prompt is its brief: one line of it.
                let said = prompt.text.clone().unwrap_or_else(|| text(&prompt.content));
                let source = prompt.source.as_deref().unwrap_or("daemon");
                let first = said.lines().next().unwrap_or_default().to_string();
                let lead = format!("» {source} ");
                let room = width.saturating_sub(wrap::width(&lead));
                rows.push(Row::of(
                    Line::from(vec![
                        Span::styled(lead, DAEMON),
                        Span::styled(wrap::clip(&first, room), DIM),
                    ]),
                    Some(&item),
                ));
            }
            None => rows.push(Row::of(
                Line::styled("» session · before the first prompt", DAEMON),
                Some(&item),
            )),
        }

        let (mark, style) = self.turn_mark(turn);
        let mut parts = vec![
            Span::raw("  "),
            Span::styled(if open { UNFOLDED } else { FOLDED }, DIM),
            Span::styled(format!("{mark} "), style),
        ];
        let mut said = crate::trajectory::activity(turn);
        if let Some(took) = took(&turn.started_at, turn.ended_at.as_deref()) {
            said.push_str(&format!("{}{took}", theme::SEPARATOR));
        }
        parts.push(Span::styled(
            wrap::clip(&said, width.saturating_sub(6)),
            DIM,
        ));
        rows.push(Row::head(Line::from(parts), &item));

        // Text appends update the message step between turn headers.
        let live_answer = if turn.state == TurnState::Running {
            turn.steps.iter().rev().find_map(|step| match step {
                StepDto::Message(message) if message.lane == MAIN_LANE => Some(message),
                _ => None,
            })
        } else {
            None
        };
        let is_answer_step = |step: &StepDto| {
            live_answer.is_some_and(|message| message.id == crate::trajectory::step_id(step))
        };
        let mut answer_in_step = false;

        // A warning note and a compaction show in their turn whether it is
        // open or not, read from the turn and not from its steps: an open
        // turn whose steps cannot be read shows them too.
        for step in turn.notices.iter().rev() {
            rows.push(Row::of(
                self.step_row(turn, step, 0, width, false),
                Some(&item),
            ));
        }
        if turn.notices_more > 0 {
            let more = match turn.notices_more {
                1 => "1 more notice".to_string(),
                more => format!("{more} more notices"),
            };
            rows.push(Row::of(
                Line::styled(format!("{STEP_INDENT}{more} in the steps of the turn"), DIM),
                Some(&item),
            ));
        }

        if open {
            if turn.earlier_steps > 0 && turn.state == TurnState::Running {
                rows.push(Row::of(
                    Line::styled(
                        format!("{STEP_INDENT}{} earlier steps", turn.earlier_steps),
                        DIM,
                    ),
                    Some(&item),
                ));
            }
            match self.shown_steps(turn) {
                Some(steps) => {
                    for (depth, step) in steps {
                        let step_item = Item::Step {
                            turn: turn.id.clone(),
                            step: crate::trajectory::step_id(step).to_string(),
                        };
                        let is_answer = is_answer_step(step);
                        rows.push(Row::head(
                            self.step_row(turn, step, depth, width, is_answer),
                            &step_item,
                        ));
                        if self.open.contains(&step_item) {
                            answer_in_step |= is_answer;
                            for line in self.open_step(step, &step_item, depth, width) {
                                rows.push(Row::of(line, Some(&step_item)));
                            }
                        }
                    }
                }
                None => rows.push(Row::of(
                    Line::styled(format!("{STEP_INDENT}reading the steps…"), DIM),
                    Some(&item),
                )),
            }
        } else {
            let noticed: Vec<&str> = turn
                .notices
                .iter()
                .map(crate::trajectory::step_id)
                .collect();
            if turn.state == TurnState::Running {
                let live: Vec<&StepDto> = turn
                    .steps
                    .iter()
                    .filter(|step| !noticed.contains(&crate::trajectory::step_id(step)))
                    .filter(|step| !answered_on_its_call(step, &turn.steps))
                    .collect();
                let skip = live.len().saturating_sub(LIVE_STEPS);
                for step in &live[skip..] {
                    rows.push(Row::of(
                        self.step_row(turn, step, 0, width, is_answer_step(step)),
                        Some(&item),
                    ));
                }
            } else {
                for step in &turn.attention {
                    if matches!(step, StepDto::Error(_)) {
                        rows.push(Row::of(
                            self.step_row(turn, step, 0, width, false),
                            Some(&item),
                        ));
                    }
                }
            }
        }

        let blocks = live_answer.map_or(turn.answer.as_slice(), |message| &message.content);
        let answer = text(blocks);
        if !answer_in_step && !answer.trim().is_empty() {
            for line in self.answer(&turn.id, blocks, body) {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                rows.push(Row::of(Line::from(spans).style(line.style), Some(&item)));
            }
        }
        rows.push(Row::of(Line::default(), None));
    }

    /// A turn's answer as markdown, drawn once for each width and content.
    fn answer(&self, id: &str, blocks: &[ContentBlockDto], width: usize) -> Vec<Line<'static>> {
        // The key holds what the lines show: the text, and the line of each
        // block that is not text, so a new name of an image draws again.
        let answer: String = blocks
            .iter()
            .map(|block| detail::media(block).unwrap_or_else(|| text(std::slice::from_ref(block))))
            .collect::<Vec<_>>()
            .join("\u{0}");
        let mut answers = self.answers.borrow_mut();
        if let Some((drawn_width, drawn, lines)) = answers.get(id)
            && *drawn_width == width
            && *drawn == answer
        {
            return lines.clone();
        }
        let lines = content_lines(blocks, width);
        answers.insert(id.to_string(), (width, answer, lines.clone()));
        lines
    }

    /// What an open step shows under its row: a tool call its detail by
    /// its kind, a subagent its task, and every other step its whole text.
    fn open_step(
        &self,
        step: &StepDto,
        item: &Item,
        depth: usize,
        width: usize,
    ) -> Vec<Line<'static>> {
        // A step's body carries the indent of a body; a subagent's steps
        // add one level for each subagent they are under.
        let lead = DEPTH_INDENT.repeat(depth);
        let room = width.saturating_sub(wrap::width(&lead));
        let indent = |line: Line<'static>| {
            let mut spans = vec![Span::raw(lead.clone())];
            spans.extend(line.spans);
            Line::from(spans).style(line.style)
        };
        let body = |text: &str| indent(Line::styled(format!("{BODY_INDENT}{text}"), DIM));
        let room_of_body = room.saturating_sub(BODY_INDENT.len());
        match step {
            StepDto::ToolCall(call) => match self.details.get(&call.id) {
                Some(Detail::Read(read)) => {
                    let raw = self.raw.contains(&call.id);
                    let whole = self.whole.contains(item);
                    // The detail lines carry four of the six columns of a
                    // body.
                    let gap = BODY_INDENT.len() - 4;
                    detail::lines(read, raw, whole, room.saturating_sub(gap))
                        .into_iter()
                        .map(|line| {
                            let mut spans = vec![Span::raw(" ".repeat(gap))];
                            spans.extend(line.spans);
                            indent(Line::from(spans).style(line.style))
                        })
                        .collect()
                }
                Some(Detail::Failed(reason)) => {
                    let mut lines: Vec<Line<'static>> =
                        step_body(step, room).into_iter().map(indent).collect();
                    lines.push(body(&wrap::clip(reason, room_of_body)));
                    lines
                }
                None => vec![body("reading the detail…")],
            },
            StepDto::Subagent(subagent) => match &subagent.task {
                Some(task) => wrap::wrap(task, room_of_body)
                    .iter()
                    .map(|part| body(part))
                    .collect(),
                None => Vec::new(),
            },
            _ => step_body(step, room).into_iter().map(indent).collect(),
        }
    }

    fn turn_mark(&self, turn: &TurnDto) -> (&'static str, Style) {
        match turn.state {
            TurnState::Running => (SPINNER[self.tick % SPINNER.len()], TOOL),
            TurnState::Completed => (theme::CALL_DONE, AGENT),
            TurnState::Failed => (theme::CALL_FAILED, FAIL),
            TurnState::Cancelled | TurnState::Interrupted => (CANCELLED, DIM),
        }
    }

    /// One step: its status, its kind, its subject and how long it took.
    /// A call carries the mark of the permission that was answered for it,
    /// and a subagent its task, its state and its step count.
    fn step_row(
        &self,
        turn: &TurnDto,
        step: &StepDto,
        depth: usize,
        width: usize,
        is_answer: bool,
    ) -> Line<'static> {
        let (mark, style) = step_mark(step);
        let label = step_kind(step);
        let kind_style = match step {
            StepDto::ToolCall(_) => TOOL,
            _ => DIM,
        };
        let indent = format!("{STEP_INDENT}{}", DEPTH_INDENT.repeat(depth));
        let lead = format!("{indent}{mark} ");
        let kind = format!("{label:<KIND_WIDTH$} ");
        let mut tail = Vec::new();
        let took = match step {
            StepDto::ToolCall(call) => {
                if let Some(answered) = call
                    .permission
                    .as_deref()
                    .and_then(|id| self.permission_of(turn, id))
                    .and_then(permission_mark)
                {
                    tail.push(Span::styled(format!("  {answered}"), ASK));
                }
                took(&call.started_at, call.ended_at.as_deref())
            }
            StepDto::Subagent(subagent) => took(&subagent.started_at, subagent.ended_at.as_deref()),
            _ => None,
        };
        if let StepDto::Subagent(subagent) = step {
            let mut said = Vec::new();
            if let Some(state) = &subagent.state {
                said.push(state.clone());
            }
            if let Some(took) = &took {
                said.push(took.clone());
            }
            said.push(match subagent.activity.steps {
                1 => "1 step".to_string(),
                steps => format!("{steps} steps"),
            });
            tail.push(Span::styled(
                format!("  {}", said.join(theme::SEPARATOR)),
                DIM,
            ));
        } else if let Some(took) = took {
            tail.push(Span::styled(format!("  {took}"), DIM));
        }
        let tail_width: usize = tail.iter().map(|span| wrap::width(&span.content)).sum();
        let room = width.saturating_sub(wrap::width(&lead) + wrap::width(&kind) + tail_width);
        let subject = if is_answer {
            "Turn answer".into()
        } else {
            match step {
                StepDto::Subagent(subagent) => match &subagent.task {
                    Some(task) => format!("{} · {}", subagent.title, task.trim()),
                    None => subagent.title.clone(),
                },
                // Under its subagent, a step needs no name of its lane.
                _ if depth > 0 => bare_subject(step),
                _ => subject(step),
            }
        };
        let first = subject.lines().next().unwrap_or_default();
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(format!("{mark} "), style),
            Span::styled(kind, kind_style),
            Span::raw(wrap::clip(first, room)),
        ];
        spans.extend(tail);
        Line::from(spans)
    }

    /// The pending permission or the open question, above the input.
    fn attention_lines(&self, width: usize, most: usize) -> Vec<Line<'static>> {
        if let Some(permission) = self.permission() {
            return permission_lines(permission, self.picked.1, width, most);
        }
        self.form_lines(width, most).unwrap_or_default()
    }

    /// The keys of the zone that has them, and the state of the link.
    fn footer(&self, width: usize) -> Line<'static> {
        let mut said = Vec::new();
        if !self.live {
            said.push("reconnecting");
        }
        if self.armed {
            said.push("ctrl-c again to leave");
        }
        let keys = match self.zone {
            Zone::Transcript => {
                "↑↓ move · enter open · ← close · r raw · y copy · home/end · ctrl-t plan · tab input"
            }
            Zone::Input if self.permission().is_some() => {
                "↑↓ pick · enter answer · tab transcript · ctrl-c twice leave"
            }
            Zone::Input if self.question().is_some() => {
                "↑↓ pick · enter answer · tab next · esc decline · ctrl-c twice leave"
            }
            Zone::Input if self.running() => {
                "enter send · tab transcript · pgup/pgdn scroll · ctrl-o open · ctrl-t plan · esc cancel · ctrl-c twice leave"
            }
            Zone::Input => {
                "enter send · tab transcript · pgup/pgdn scroll · ctrl-o open · ctrl-t plan · ctrl-c twice leave"
            }
        };
        said.push(keys);
        Line::styled(
            wrap::clip(&format!(" {}", said.join(theme::SEPARATOR)), width),
            DIM,
        )
    }
}

/// The status mark of a step and its style.
fn step_mark(step: &StepDto) -> (&'static str, Style) {
    let done = (theme::CALL_DONE, AGENT);
    let failed = (theme::CALL_FAILED, FAIL);
    let cancelled = (CANCELLED, DIM);
    let running = (theme::CALL_RUNNING, TOOL);
    match step {
        StepDto::ToolCall(call) => match call.status {
            ToolCallStatus::Pending => (theme::CALL_PENDING, TOOL),
            ToolCallStatus::Running => running,
            ToolCallStatus::Completed => done,
            ToolCallStatus::Failed => failed,
            ToolCallStatus::Cancelled | ToolCallStatus::Interrupted => cancelled,
        },
        StepDto::Message(_) | StepDto::Thought(_) if is_live(step) => running,
        StepDto::Message(_) => done,
        StepDto::Thought(_) => (NOTE, DIM),
        StepDto::Permission(permission) => match permission.state {
            PermissionState::Pending => (WAITS, ASK),
            PermissionState::Answered => done,
            PermissionState::Cancelled => cancelled,
        },
        StepDto::Question(question) => match question.state {
            QuestionState::Open => (WAITS, ASK),
            QuestionState::Answered => done,
            QuestionState::Declined | QuestionState::Cancelled => cancelled,
        },
        StepDto::Note(note) => match note.severity {
            NoteSeverity::Info => (NOTE, DIM),
            NoteSeverity::Warning => ("!", TOOL),
        },
        StepDto::Error(_) => failed,
        StepDto::Subagent(subagent) => match subagent.state.as_deref() {
            Some("completed") => done,
            Some("failed") => failed,
            Some("cancelled") => cancelled,
            _ if is_live(step) => running,
            _ => done,
        },
        StepDto::Compaction(compaction) => match compaction.status {
            CompactionStatus::Running => running,
            CompactionStatus::Completed => done,
            CompactionStatus::Failed => failed,
        },
    }
}

/// The kind a step row names: a tool call by the kind of its tool.
fn step_kind(step: &StepDto) -> String {
    match step {
        StepDto::ToolCall(call) => call.kind.clone().unwrap_or_else(|| "tool".to_string()),
        other => kind(other).to_string(),
    }
}

/// What a step is about, whole, after the lane it is in where that is not
/// the session's own: the row shows its first line.
fn subject(step: &StepDto) -> String {
    let said = bare_subject(step);
    match lane_of(step) {
        MAIN_LANE => said,
        lane => format!("{lane}: {said}"),
    }
}

/// What a step is about, whole.
fn bare_subject(step: &StepDto) -> String {
    let said = match step {
        StepDto::Message(message) => text(&message.content),
        StepDto::Thought(thought) => text(&thought.content),
        StepDto::ToolCall(call) => match call.subject.as_deref() {
            Some(subject) if subject != call.title => format!("{} · {subject}", call.title),
            _ => call.title.clone(),
        },
        StepDto::Permission(permission) => permission.question.clone(),
        StepDto::Question(question) => question.question.clone(),
        StepDto::Note(note) => note.text.clone(),
        StepDto::Error(error) => error.text.clone(),
        StepDto::Subagent(subagent) => subagent.title.clone(),
        StepDto::Compaction(compaction) => match compaction.status {
            CompactionStatus::Running => "compacting the context".to_string(),
            CompactionStatus::Completed => "context compacted".to_string(),
            CompactionStatus::Failed => "the compaction failed".to_string(),
        },
    };
    said.trim().to_string()
}

/// The mark an answered permission puts on the row of its call: the option
/// chosen, and who chose it where that was not a person at the console.
fn permission_mark(permission: &PermissionStepDto) -> Option<String> {
    match permission.state {
        PermissionState::Pending => None,
        PermissionState::Cancelled => Some(format!("{CANCELLED} permission cancelled")),
        PermissionState::Answered => {
            let answer = permission.answer.as_ref()?;
            let allowed = permission
                .options
                .iter()
                .find(|option| Some(&option.id) == answer.option_id.as_ref())
                .and_then(|option| option.kind.as_deref())
                .is_none_or(|kind| kind.starts_with("allow"));
            let mark = if allowed {
                theme::CALL_DONE
            } else {
                theme::CALL_FAILED
            };
            Some(match answer.decided_by.as_deref() {
                Some(by) if by != "console" => format!("{mark} {} · by {by}", answer.label),
                _ => format!("{mark} {}", answer.label),
            })
        }
    }
}

/// Whether a step is a permission that was answered and whose call is in
/// `steps`: the call's row carries it.
fn answered_on_its_call(step: &StepDto, steps: &[StepDto]) -> bool {
    let StepDto::Permission(permission) = step else {
        return false;
    };
    permission.state != PermissionState::Pending
        && steps.iter().any(|other| {
            matches!(other, StepDto::ToolCall(call) if call.permission.as_deref() == Some(permission.id.as_str()))
        })
}

/// Content blocks as lines: the text as markdown, and a block that is not
/// text as one line with its type and its name.
fn content_lines(blocks: &[ContentBlockDto], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut said = String::new();
    let flush = |said: &mut String, lines: &mut Vec<Line<'static>>| {
        if !said.trim().is_empty() {
            lines.extend(markdown::render(said.trim_end(), width));
        }
        said.clear();
    };
    for block in blocks {
        match detail::media(block) {
            Some(media) => {
                flush(&mut said, &mut lines);
                lines.push(Line::styled(wrap::clip(&media, width), DIM));
            }
            None => said.push_str(&text(std::slice::from_ref(block))),
        }
    }
    flush(&mut said, &mut lines);
    lines
}

/// An open step, in place under its row: what it says whole, as the log
/// prints it.
fn step_body(step: &StepDto, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(BODY_INDENT.len());
    let indent = |line: Line<'static>| {
        let mut spans = vec![Span::raw(BODY_INDENT)];
        spans.extend(line.spans);
        Line::from(spans).style(line.style)
    };
    match step {
        StepDto::Message(message) => content_lines(&message.content, room)
            .into_iter()
            .map(indent)
            .collect(),
        StepDto::Thought(thought) => markdown::render(text(&thought.content).trim_end(), room)
            .into_iter()
            .map(|line| indent(line.patch_style(DIM)))
            .collect(),
        _ => {
            let printed = crate::trajectory::Printer::new(chrono::Utc).step(step);
            let mut said = vec![subject(step)];
            said.extend(
                printed
                    .lines()
                    .skip(1)
                    .map(|line| line.strip_prefix(STEP_INDENT).unwrap_or(line).to_string()),
            );
            said.iter()
                .flat_map(|line| wrap::wrap(line, room))
                .map(|part| indent(Line::styled(part, DIM)))
                .collect()
        }
    }
}

/// A pending permission between two rules: the question, the call it
/// asks about, why the AI permission model had a part, and the options,
/// numbered, the pick marked.
fn permission_lines(
    permission: &PermissionStepDto,
    picked: usize,
    width: usize,
    most: usize,
) -> Vec<Line<'static>> {
    let width = width.max(8);
    let lead = format!("{} ", theme::RULE);
    let rest = width.saturating_sub(wrap::width(&lead) + PERMISSION_LABEL.len() + 1);
    let mut lines = vec![Line::from(vec![
        Span::styled(lead, FRAME),
        Span::styled(PERMISSION_LABEL, ASK),
        Span::styled(format!(" {}", theme::RULE.repeat(rest)), FRAME),
    ])];
    let mut asked = permission.question.clone();
    if let Some(by) = &permission.subagent {
        asked.push_str(&format!(" · asked by subagent {}", by.title));
    }
    for part in wrap::wrap(&asked, width.saturating_sub(2)) {
        lines.push(Line::from(vec![
            Span::styled(theme::OPTION_PICKED, ASK),
            Span::styled(part, QUESTION),
        ]));
    }
    let mut call = permission.tool.title.clone();
    if let Some(subject) = permission
        .tool
        .subject
        .as_deref()
        .filter(|subject| *subject != call)
    {
        call.push_str(&format!(" · {subject}"));
    }
    for part in wrap::wrap(&call, width.saturating_sub(2)) {
        lines.push(Line::from(vec![Span::raw("  "), Span::styled(part, TOOL)]));
    }
    if let Some(ai) = &permission.ai {
        for part in wrap::wrap(ai, width.saturating_sub(2)) {
            lines.push(Line::from(vec![Span::raw("  "), Span::styled(part, DIM)]));
        }
    }
    let mut selected = None;
    for (at, option) in permission.options.iter().enumerate() {
        let chosen = at == picked;
        if chosen {
            selected = Some(lines.len());
        }
        let (mark, style) = match chosen {
            true => (theme::OPTION_PICKED, PICKED),
            false => (theme::OPTION_IDLE, Style::new()),
        };
        let said = format!("{}. {}", at + 1, option.name);
        for (row, part) in wrap::wrap(&said, width.saturating_sub(4))
            .into_iter()
            .enumerate()
        {
            let lead = if row == 0 { mark } else { "  " };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(lead, style),
                Span::styled(part, style),
            ]));
        }
    }
    lines.push(Line::styled(theme::RULE.repeat(width), FRAME));
    let tail = lines.len().saturating_sub(most);
    let start = selected.map_or(tail, |row| tail.min(row));
    lines.into_iter().skip(start).take(most).collect()
}

/// How long something took from `started` to `ended`, or until now while it
/// runs.
fn took(started: &str, ended: Option<&str>) -> Option<String> {
    let started = chrono::DateTime::parse_from_rfc3339(started).ok()?;
    let ended = match ended {
        Some(ended) => chrono::DateTime::parse_from_rfc3339(ended).ok()?,
        None => chrono::Utc::now().fixed_offset(),
    };
    let seconds = (ended - started).num_seconds().max(0);
    Some(match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    })
}
