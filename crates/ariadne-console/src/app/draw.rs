//! The whole screen, drawn from the state: the bar of the tabs, the
//! transcript of turns or the steps of a subagent, the plan, the permission or the question above the input, the suggestion
//! list, the input box and the bottom row, and over them the list of keys.

use ratatui::Frame as Draw;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};

use ariadne_api::trajectory::{
    CompactionStatus, ContentBlockDto, MAIN_LANE, NoteSeverity, PermissionState, PermissionStepDto,
    QuestionState, StepDto, SubagentStepDto, ToolCallStatus, TurnDto, TurnState,
};

use crate::markdown;
use crate::theme::{
    self, ADDED, AGENT, ASK, DAEMON, DIM, FAIL, FRAME, PICKED, PROMPT, QUESTION, REMOVED, SPINNER,
    TOOL, USER,
};
use crate::trajectory::{kind, text};
use crate::wrap;

use super::{
    App, Detail, Entry, Item, LIVE_STEPS, Zone, detail, grouped, is_live, lane_of, status,
};

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
        let whole = frame.area();
        let width = usize::from(whole.width);
        let bar = self.tab_bar(width).filter(|_| whole.height > 1);
        let bar_rows = u16::from(bar.is_some());
        let area = Rect::new(
            whole.x,
            whole.y + bar_rows,
            whole.width,
            whole.height - bar_rows,
        );
        let input = self.input.height(area.width).min(area.height);
        let bottom = u16::from(area.height > input);
        let below = input + bottom;
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
                let (lines, scroll, shown) = status::plan_lines(
                    plan,
                    self.plan_open,
                    self.plan_scroll.get(),
                    width,
                    most.min(room),
                );
                self.plan_scroll.set(scroll);
                self.plan_shown.set(shown);
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
        let bottom_area = next(bottom);

        if let Some(bar) = bar {
            frame.render_widget(
                Paragraph::new(bar),
                Rect::new(whole.x, whole.y, whole.width, 1),
            );
        }
        self.draw_transcript(frame, transcript_area);
        self.layout.borrow_mut().plan = plan_area;
        frame.render_widget(Paragraph::new(Text::from(plan)), plan_area);
        frame.render_widget(Paragraph::new(Text::from(attention)), attention_area);
        self.draw_suggestions(frame, suggestions_area);
        self.input.draw(
            frame,
            input_area,
            self.hint(),
            self.zone == Zone::Input && !self.keys_open,
        );
        frame.render_widget(Paragraph::new(self.bottom_row(width)), bottom_area);
        if self.keys_open {
            self.draw_keys(frame, whole);
        }
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
                true => cursor_line(&row.line, usize::from(area.width)),
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

    /// Every line of the transcript at `width`: the turns in main, and
    /// the steps of the subagent in its tab.
    fn rows(&self, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        if let Some(subagent) = self.shown_subagent() {
            self.tab_rows(subagent, width, &mut rows);
            self.notice_rows(width, &mut rows);
            return rows;
        }
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
        self.notice_rows(width, &mut rows);
        rows
    }

    /// What the console says about a post that did not arrive, in every
    /// tab.
    fn notice_rows(&self, width: usize, rows: &mut Vec<Row>) {
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
    }

    /// A subagent's tab: its task, then a row for each of its steps, as an
    /// open turn of main shows them.
    fn tab_rows(&self, subagent: &SubagentStepDto, width: usize, rows: &mut Vec<Row>) {
        if let Some(task) = subagent.task.as_deref().map(str::trim) {
            for part in wrap::wrap(task, width.saturating_sub(2)) {
                rows.push(Row::of(Line::styled(format!("  {part}"), DIM), None));
            }
        }
        let entries = self.tab_entries(subagent);
        if entries.is_empty() {
            let lane = (subagent.turn_id.clone(), subagent.subagent_id.clone());
            let said = match self.lanes.contains_key(&lane) {
                true => "no step yet",
                false => "reading the steps…",
            };
            rows.push(Row::of(
                Line::styled(format!("{STEP_INDENT}{said}"), DIM),
                None,
            ));
        }
        self.entry_rows(
            &subagent.turn_id,
            entries,
            width,
            &|_| false,
            &mut false,
            rows,
        );
        rows.push(Row::of(Line::default(), None));
    }

    /// The rows of `entries` of the turn `turn`: each step, and each group
    /// with its steps under it while it is open.
    fn entry_rows(
        &self,
        turn: &str,
        entries: Vec<Entry<'_>>,
        width: usize,
        is_answer_step: &dyn Fn(&StepDto) -> bool,
        answer_in_step: &mut bool,
        rows: &mut Vec<Row>,
    ) {
        for entry in entries {
            match entry {
                Entry::Step(depth, step) => {
                    self.push_step_rows(
                        turn,
                        step,
                        depth,
                        width,
                        is_answer_step(step),
                        answer_in_step,
                        rows,
                    );
                }
                Entry::Group(group) => {
                    let group_item = Entry::Group(group.clone()).item(turn);
                    rows.push(Row::head(self.group_row(&group, width), &group_item));
                    if self.open.contains(&group_item) {
                        for step in group {
                            self.push_step_rows(turn, step, 1, width, false, answer_in_step, rows);
                        }
                    }
                }
            }
        }
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
                // A daemon's prompt is its brief: one line of it, and the
                // whole of it as markdown once opened.
                let said = prompt.text.clone().unwrap_or_else(|| text(&prompt.content));
                let source = prompt.source.as_deref().unwrap_or("daemon");
                let first = prompt_line(&said);
                let lead = format!("» {source} ");
                let room = width.saturating_sub(wrap::width(&lead));
                let at = Item::Prompt(turn.id.clone());
                rows.push(Row::head(
                    Line::from(vec![
                        Span::styled(lead, DAEMON),
                        Span::styled(wrap::clip(&first, room), DIM),
                    ]),
                    &at,
                ));
                if self.open.contains(&at) {
                    for line in markdown::render(said.trim_end(), body) {
                        let mut spans = vec![Span::raw("  ")];
                        spans.extend(line.spans);
                        rows.push(Row::of(Line::from(spans).style(line.style), Some(&at)));
                    }
                }
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
        let took = took(&turn.started_at, turn.ended_at.as_deref());
        // The mark says the state of the turn, so its line has no state word.
        let said = crate::trajectory::counts_line(turn, took);
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
            let notice = Item::Step {
                turn: turn.id.clone(),
                step: crate::trajectory::step_id(step).to_string(),
            };
            rows.push(Row::head(
                self.step_row(&turn.id, step, 0, width, false),
                &notice,
            ));
            if self.open.contains(&notice) {
                for line in self.open_step(step, &notice, 0, width) {
                    rows.push(Row::of(line, Some(&notice)));
                }
            }
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
            match self.entries(turn) {
                Some(entries) => self.entry_rows(
                    &turn.id,
                    entries,
                    width,
                    &is_answer_step,
                    &mut answer_in_step,
                    rows,
                ),
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
                    .filter(|step| lane_of(step) == MAIN_LANE || !self.has_tab(lane_of(step)))
                    .filter(|step| !answered_on_its_call(step, &turn.steps))
                    .collect();
                let skip = live.len().saturating_sub(LIVE_STEPS);
                let entries = grouped(live[skip..].iter().map(|step| (0, *step)).collect());
                for entry in &entries {
                    let line = match entry {
                        Entry::Step(_, step) => {
                            self.step_row(&turn.id, step, 0, width, is_answer_step(step))
                        }
                        Entry::Group(group) => self.group_row(group, width),
                    };
                    rows.push(Row::of(line, Some(&item)));
                }
            } else {
                for step in &turn.attention {
                    if matches!(step, StepDto::Error(_)) {
                        rows.push(Row::of(
                            self.step_row(&turn.id, step, 0, width, false),
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
                    // The command shows in its detail only where its row
                    // cut it.
                    let cut = self.row_cut(item.turn(), step, depth, width);
                    let mut lines: Vec<Line<'static>> =
                        detail::lines(read, raw, whole, room.saturating_sub(gap), cut)
                            .into_iter()
                            .map(|line| {
                                let mut spans = vec![Span::raw(" ".repeat(gap))];
                                spans.extend(line.spans);
                                indent(Line::from(spans).style(line.style))
                            })
                            .collect();
                    if let Some(permission) = call
                        .permission
                        .as_deref()
                        .and_then(|id| self.permission_of(item.turn(), id))
                    {
                        for said in permission_detail(permission) {
                            for part in wrap::wrap(&said, room_of_body) {
                                lines.push(body(&part));
                            }
                        }
                    }
                    lines
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

    /// The row of a step, and under it what it shows while it is open.
    #[allow(clippy::too_many_arguments)]
    fn push_step_rows(
        &self,
        turn: &str,
        step: &StepDto,
        depth: usize,
        width: usize,
        is_answer: bool,
        answer_in_step: &mut bool,
        rows: &mut Vec<Row>,
    ) {
        let step_item = Item::Step {
            turn: turn.to_string(),
            step: crate::trajectory::step_id(step).to_string(),
        };
        rows.push(Row::head(
            self.step_row(turn, step, depth, width, is_answer),
            &step_item,
        ));
        if self.open.contains(&step_item) {
            *answer_in_step |= is_answer;
            for line in self.open_step(step, &step_item, depth, width) {
                rows.push(Row::of(line, Some(&step_item)));
            }
        }
    }

    /// A group of read and search calls: its state mark, the icon of its
    /// first call and the subject of each call, cut to the width.
    fn group_row(&self, group: &[&StepDto], width: usize) -> Line<'static> {
        let state = |step: &&StepDto| match step {
            StepDto::ToolCall(call) => call.status,
            _ => ToolCallStatus::Completed,
        };
        let (mark, style) = if group
            .iter()
            .any(|step| state(step) == ToolCallStatus::Running)
        {
            (theme::CALL_RUNNING, TOOL)
        } else if group
            .iter()
            .any(|step| state(step) == ToolCallStatus::Failed)
        {
            (theme::CALL_FAILED, FAIL)
        } else {
            (theme::CALL_DONE, AGENT)
        };
        // One glyph has one meaning: a group of one kind has the icon of
        // that kind, and a mixed group has the glyph of a group.
        fn kind_of<'a>(step: &&'a StepDto) -> Option<&'a str> {
            match step {
                StepDto::ToolCall(call) => call.kind.as_deref(),
                _ => None,
            }
        }
        let icon = if group.iter().all(|step| kind_of(step) == kind_of(&group[0])) {
            theme::kind_icon(kind_of(&group[0]))
        } else {
            theme::GROUP
        };
        let subjects: Vec<String> = group.iter().map(|step| call_subject(step)).collect();
        let lead = STEP_INDENT.to_string();
        let room =
            width.saturating_sub(wrap::width(&lead) + wrap::width(mark) + wrap::width(icon) + 2);
        Line::from(vec![
            Span::raw(lead),
            Span::styled(format!("{mark} "), style),
            Span::styled(format!("{icon} "), TOOL),
            Span::raw(wrap::clip(&subjects.join(", "), room)),
        ])
    }

    fn turn_mark(&self, turn: &TurnDto) -> (&'static str, Style) {
        match turn.state {
            TurnState::Running => (SPINNER[self.tick % SPINNER.len()], TOOL),
            TurnState::Completed => (theme::CALL_DONE, AGENT),
            TurnState::Failed => (theme::CALL_FAILED, FAIL),
            TurnState::Cancelled | TurnState::Interrupted => (CANCELLED, DIM),
        }
    }

    /// One step: its status, its kind and its subject, then the tail in one
    /// column at the right edge: the permission mark, the diff counts and
    /// how long it took. A subagent is its summary row: its name, its step
    /// count and its time.
    fn step_row(
        &self,
        turn: &str,
        step: &StepDto,
        depth: usize,
        width: usize,
        is_answer: bool,
    ) -> Line<'static> {
        self.step_parts(turn, step, depth, width, is_answer).0
    }

    /// Whether the row of a step cut its subject to fit the width.
    pub(super) fn row_cut(&self, turn: &str, step: &StepDto, depth: usize, width: usize) -> bool {
        self.step_parts(turn, step, depth, width, false).1
    }

    /// The row of a step, and whether it cut the subject.
    fn step_parts(
        &self,
        turn: &str,
        step: &StepDto,
        depth: usize,
        width: usize,
        is_answer: bool,
    ) -> (Line<'static>, bool) {
        let (mark, style) = step_mark(step);
        // A tool call shows the icon of its kind; every other step names its
        // kind.
        let (label, kind_style) = match step {
            StepDto::ToolCall(call) => (theme::kind_icon(call.kind.as_deref()).to_string(), TOOL),
            StepDto::Subagent(_) => (theme::SUBAGENT.to_string(), TOOL),
            other => (format!("{:<KIND_WIDTH$}", step_kind(other)), DIM),
        };
        let indent = format!("{STEP_INDENT}{}", DEPTH_INDENT.repeat(depth));
        let lead = format!("{indent}{mark} ");
        let kind = format!("{label} ");
        let mut tail: Vec<Span<'static>> = Vec::new();
        let put = |tail: &mut Vec<Span<'static>>, span: Span<'static>| {
            if !tail.is_empty() {
                tail.push(Span::raw(" "));
            }
            tail.push(span);
        };
        match step {
            StepDto::ToolCall(call) => {
                if let Some(answered) = call
                    .permission
                    .as_deref()
                    .and_then(|id| self.permission_of(turn, id))
                    .and_then(permission_mark)
                {
                    put(&mut tail, answered);
                }
                if call.kind.as_deref() == Some("edit")
                    && let Some(stat) = &call.diff_stat
                {
                    put(&mut tail, Span::styled(format!("+{}", stat.added), ADDED));
                    tail.push(Span::raw(" "));
                    tail.push(Span::styled(format!("-{}", stat.removed), REMOVED));
                }
                if let Some(took) = took(&call.started_at, call.ended_at.as_deref()) {
                    put(&mut tail, Span::styled(took, DIM));
                }
            }
            StepDto::Subagent(subagent) => {
                let mut said = vec![match subagent.activity.steps {
                    1 => "1 step".to_string(),
                    steps => format!("{steps} steps"),
                }];
                said.extend(took(&subagent.started_at, subagent.ended_at.as_deref()));
                put(&mut tail, Span::styled(said.join(theme::SEPARATOR), DIM));
            }
            _ => {}
        }
        let tail_width: usize = tail.iter().map(|span| wrap::width(&span.content)).sum();
        let gap = usize::from(tail_width > 0);
        let room = width.saturating_sub(wrap::width(&lead) + wrap::width(&kind) + tail_width + gap);
        let subject = if is_answer {
            "Turn answer".into()
        } else {
            match step {
                // In its subagent's tab, a step needs no name of its lane.
                _ if depth > 0 || self.tab.as_deref() == Some(lane_of(step)) => bare_subject(step),
                _ => subject(step),
            }
        };
        let first = subject.lines().next().unwrap_or_default();
        let shown = wrap::clip(first, room);
        // A title of many lines lost its later lines to the first-line subject.
        let title_cut =
            matches!(step, StepDto::ToolCall(call) if call.title.trim().lines().nth(1).is_some());
        let cut = title_cut || subject.lines().nth(1).is_some() || shown != first;
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(format!("{mark} "), style),
            Span::styled(kind, kind_style),
        ];
        let pad = room.saturating_sub(wrap::width(&shown));
        spans.push(Span::raw(shown));
        if tail_width > 0 {
            spans.push(Span::raw(" ".repeat(pad + gap)));
            spans.extend(tail);
        }
        (Line::from(spans), cut)
    }

    /// The pending permission or the open question, above the input.
    fn attention_lines(&self, width: usize, most: usize) -> Vec<Line<'static>> {
        if let Some(permission) = self.permission() {
            return permission_lines(permission, self.picked.1, width, most);
        }
        self.form_lines(width, most).unwrap_or_default()
    }

    /// The row under the input box: the state of the session and the hint
    /// for the keys, or the state of the link while it needs saying.
    fn bottom_row(&self, width: usize) -> Line<'static> {
        let mut said = Vec::new();
        if !self.live {
            said.push("reconnecting");
        }
        if self.armed {
            said.push("ctrl-c again to leave");
        }
        let said = (!said.is_empty()).then(|| said.join(theme::SEPARATOR));
        status::bottom_row(
            self.view.as_ref().map(|view| view.session()),
            self.model.as_deref(),
            self.name.as_deref(),
            said.as_deref(),
            width,
        )
    }

    /// The list of every key, over the screen, in a group for each zone. The
    /// groups stand in as many columns as the width holds, and the screen
    /// shows no text cursor under them.
    fn draw_keys(&self, frame: &mut Draw, area: Rect) {
        let groups = KEYS
            .iter()
            .map(|(zone, keys)| {
                let mut lines = vec![Line::styled(format!(" {zone}"), QUESTION)];
                lines.extend(keys.iter().map(|(key, what)| {
                    Line::from(vec![
                        Span::styled(format!("{KEY_INDENT}{key:<KEY_WIDTH$}"), PICKED),
                        Span::raw(what.to_string()),
                    ])
                }));
                lines
            })
            .collect::<Vec<_>>();
        let wide = KEYS
            .iter()
            .flat_map(|(_, keys)| keys.iter())
            .map(|(_, what)| wrap::width(what))
            .max()
            .unwrap_or(0)
            + KEY_INDENT.len()
            + KEY_WIDTH
            + KEYS_GUTTER;
        let across = (usize::from(area.width) / wide).max(1);
        let columns = key_columns(groups, across);
        let height = columns.iter().map(Vec::len).max().unwrap_or(0);
        let lines = (0..height)
            .map(|row| {
                let mut spans = Vec::new();
                for (at, column) in columns.iter().enumerate() {
                    let cell = column.get(row);
                    let used = cell.map_or(0, |line| line.width());
                    if let Some(line) = cell {
                        spans.extend(line.spans.iter().cloned());
                    }
                    if at + 1 < columns.len() {
                        spans.push(Span::raw(" ".repeat(wide - used)));
                    }
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>();
        // Everything above the bottom row, so that no line of the screen
        // under it shows through. The first row names how to close the list
        // and stays; the keys under it scroll where they do not all fit.
        let over = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
        frame.render_widget(Clear, over);
        let room = usize::from(over.height.saturating_sub(1));
        let from = self.keys_scroll.get().min(lines.len().saturating_sub(room));
        self.keys_scroll.set(from);
        let mut shown = vec![Line::styled(
            " keys · ? or esc closes · ↑↓ pgup pgdn scroll",
            DIM,
        )];
        shown.extend(lines.into_iter().skip(from).take(room));
        frame.render_widget(Paragraph::new(Text::from(shown)), over);
    }
}

/// The indent of a key, the columns it is padded to, and the gap between
/// two columns of the list of keys.
const KEY_INDENT: &str = "   ";
const KEY_WIDTH: usize = 14;
const KEYS_GUTTER: usize = 2;

/// The height of the groups that stand in one column, a blank line between
/// two.
fn column_height(column: &[Vec<Line<'static>>]) -> usize {
    column.iter().map(Vec::len).sum::<usize>() + column.len().saturating_sub(1)
}

/// Pack `groups` in order into as many columns as `across` allows, each
/// group whole in one column and a blank line between two in a column, as
/// short as the order allows. A packing with fewer columns than fit is split
/// at group boundaries until it fills them, which never makes it taller.
fn key_columns(groups: Vec<Vec<Line<'static>>>, across: usize) -> Vec<Vec<Line<'static>>> {
    let total = groups.iter().map(Vec::len).sum::<usize>() + groups.len().saturating_sub(1);
    let tallest = groups.iter().map(Vec::len).max().unwrap_or(0);
    let want = across.min(groups.len());
    for limit in tallest..=total {
        let mut columns: Vec<Vec<Vec<Line<'static>>>> = Vec::new();
        for group in &groups {
            match columns.last_mut() {
                Some(column) if column_height(column) + 1 + group.len() <= limit => {
                    column.push(group.clone());
                }
                _ => columns.push(vec![group.clone()]),
            }
        }
        if columns.len() > across {
            continue;
        }
        while columns.len() < want {
            let Some(at) = (0..columns.len())
                .filter(|&at| columns[at].len() > 1)
                .max_by_key(|&at| column_height(&columns[at]))
            else {
                break;
            };
            let column = &columns[at];
            let split = (1..column.len())
                .min_by_key(|&k| column_height(&column[..k]).max(column_height(&column[k..])))
                .unwrap_or(1);
            let tail = columns[at].split_off(split);
            columns.insert(at + 1, tail);
        }
        return columns
            .into_iter()
            .map(|column| {
                let mut lines = Vec::new();
                for (at, group) in column.into_iter().enumerate() {
                    if at > 0 {
                        lines.push(Line::raw(""));
                    }
                    lines.extend(group);
                }
                lines
            })
            .collect();
    }
    vec![]
}

/// Every key of the console, by zone, for the list that `?` opens.
pub(super) const KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "Input",
        &[
            ("enter", "send the prompt"),
            ("tab", "to the transcript; complete a pick"),
            ("esc", "cancel the running turn"),
            ("↑ ↓", "box rows, then prompt history"),
            ("/", "open the slash commands"),
            ("↑ ↓ tab enter", "pick and complete a slash command"),
        ],
    ),
    (
        "Input box",
        &[
            ("shift-enter", "new line"),
            ("alt-enter", "new line"),
            ("ctrl-j", "new line"),
            ("\\ enter", "new line after a backslash"),
            ("← →", "move one character"),
            ("alt-← alt-→", "move one word"),
            ("alt-b alt-f", "move one word"),
            ("home ctrl-a", "start of the line"),
            ("end ctrl-e", "end of the line"),
            ("backspace", "delete before the cursor"),
            ("delete", "delete at the cursor"),
            ("ctrl-u", "delete to the start of the line"),
            ("ctrl-k", "delete to the end of the line"),
            ("ctrl-w", "delete the word before the cursor"),
        ],
    ),
    (
        "Permission",
        &[
            ("↑ ↓", "pick an option"),
            ("1-9", "pick the option of that number"),
            ("enter", "answer with the picked option"),
        ],
    ),
    (
        "Question",
        &[
            ("↑ ↓", "pick a choice"),
            ("1-9", "pick a choice; answer a single one"),
            ("space", "check a multi-select choice"),
            ("enter", "answer and advance"),
            ("tab ← →", "move between questions, empty box"),
            ("esc", "decline the question"),
        ],
    ),
    (
        "Transcript",
        &[
            ("↑ ↓ j k", "move between turns and steps"),
            ("enter →", "open the item in place"),
            ("←", "close the item, or go to its turn"),
            ("r", "show the raw input and output of a call"),
            ("y", "copy the item"),
            ("home end", "go to the first or the newest turn"),
            ("tab esc", "give the keys back to the input"),
            ("click", "open or close a turn or a step"),
        ],
    ),
    (
        "Tabs",
        &[
            ("ctrl-n", "the next tab"),
            ("ctrl-p", "the tab before"),
            ("enter", "on a subagent row, open its tab"),
            ("esc", "in a subagent tab, back to main"),
        ],
    ),
    (
        "Everywhere",
        &[
            ("pgup pgdn", "scroll the transcript"),
            ("wheel", "scroll the transcript or the plan"),
            ("ctrl-o", "open or close the newest turn"),
            ("ctrl-t", "open or close the plan"),
            ("click plan", "open or close the plan"),
            ("ctrl-c ctrl-c", "leave"),
            ("ctrl-d", "leave at once"),
            ("?", "open this list, in an empty box"),
        ],
    ),
];

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
        StepDto::Subagent(subagent) => subagent_mark(subagent),
        StepDto::Compaction(compaction) => match compaction.status {
            CompactionStatus::Running => running,
            CompactionStatus::Completed => done,
            CompactionStatus::Failed => failed,
        },
    }
}

/// The state mark of a subagent and its style, on its row and its tab.
pub(super) fn subagent_mark(subagent: &SubagentStepDto) -> (&'static str, Style) {
    match subagent.state.as_deref() {
        Some("completed") => (theme::CALL_DONE, AGENT),
        Some("failed") => (theme::CALL_FAILED, FAIL),
        Some("cancelled") => (CANCELLED, DIM),
        _ if subagent.ended_at.is_none() => (theme::CALL_RUNNING, TOOL),
        _ => (theme::CALL_DONE, AGENT),
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

/// What a tool call is about, short: its subject, else its title.
fn call_subject(step: &StepDto) -> String {
    match step {
        StepDto::ToolCall(call) => call.subject.clone().unwrap_or_else(|| call.title.clone()),
        other => bare_subject(other),
    }
    .lines()
    .next()
    .unwrap_or_default()
    .trim()
    .to_string()
}

/// What a step is about, whole.
fn bare_subject(step: &StepDto) -> String {
    let said = match step {
        StepDto::Message(message) => text(&message.content),
        StepDto::Thought(thought) => text(&thought.content),
        StepDto::ToolCall(call) => {
            let title = call.title.lines().next().unwrap_or_default().trim();
            match call.subject.as_deref().map(str::trim) {
                Some(subject) if !subject.is_empty() && !title.contains(subject) => {
                    format!("{title} · {subject}")
                }
                _ => title.to_string(),
            }
        }
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

/// The mark an answered permission puts on the row of its call. A call
/// that `auto` or a learned permission allowed has none: it is the rule. An
/// allow by the AI or by the user is a dim mark, and a rejection a red one.
fn permission_mark(permission: &PermissionStepDto) -> Option<Span<'static>> {
    match permission.state {
        PermissionState::Pending => None,
        PermissionState::Cancelled => Some(Span::styled(CANCELLED, DIM)),
        PermissionState::Answered => {
            let answer = permission.answer.as_ref()?;
            if allowed(permission) {
                match answer.decided_by.as_deref() {
                    Some("auto" | "learned") => None,
                    _ => Some(Span::styled(theme::CALL_DONE, DIM)),
                }
            } else {
                Some(Span::styled(theme::CALL_FAILED, FAIL))
            }
        }
    }
}

/// Whether the option an answered permission chose allows the call.
fn allowed(permission: &PermissionStepDto) -> bool {
    let Some(answer) = permission.answer.as_ref() else {
        return false;
    };
    permission
        .options
        .iter()
        .find(|option| Some(&option.id) == answer.option_id.as_ref())
        .and_then(|option| option.kind.as_deref())
        .is_none_or(|kind| kind.starts_with("allow"))
}

/// The answer of a permission and who decided, one line for each decider:
/// the answer, then the AI that weighed it and the learned rule that held.
pub(super) fn permission_detail(permission: &PermissionStepDto) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(answer) = &permission.answer {
        let by = answer.decided_by.as_deref().unwrap_or("console");
        lines.push(format!("permission: {} · by {by}", answer.label));
    }
    if let Some(ai) = &permission.ai {
        lines.push(format!("ai: {ai}"));
    }
    if let Some(rule) = &permission.learned {
        lines.push(format!("learned: {rule}"));
    }
    lines
}

/// The first line of a prompt that has text, without its Markdown block
/// mark: a heading, a quote, a list bullet or a number.
fn prompt_line(said: &str) -> String {
    for line in said.lines() {
        let mut line = line.trim();
        loop {
            let before = line;
            line = line.trim_start_matches(['#', '>']).trim_start();
            for bullet in ["- ", "* ", "+ "] {
                line = line.strip_prefix(bullet).unwrap_or(line).trim_start();
            }
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            if digits > 0
                && let Some(rest) = line[digits..]
                    .strip_prefix(". ")
                    .or_else(|| line[digits..].strip_prefix(") "))
            {
                line = rest.trim_start();
            }
            if line == before {
                break;
            }
        }
        if !line.is_empty() && !line.starts_with("```") && !line.chars().all(|c| "-*_=".contains(c))
        {
            return line.to_string();
        }
    }
    String::new()
}

/// The line at the cursor: its text in one reversed style across the whole
/// width, with no colour of its own.
fn cursor_line(line: &Line<'static>, width: usize) -> Line<'static> {
    let mut said: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let pad = width.saturating_sub(wrap::width(&said));
    said.push_str(&" ".repeat(pad));
    Line::styled(said, Style::new().add_modifier(Modifier::REVERSED))
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
            // The printer writes the first line of the step after its kind,
            // and the rest of its text under it.
            let subject = subject(step);
            let mut said = vec![subject.lines().next().unwrap_or_default().to_string()];
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
