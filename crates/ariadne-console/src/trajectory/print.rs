//! Plain text of turns and steps: no colour and no terminal.
//!
//! There are two forms. A block form, for `ariadne session logs`: a turn
//! is its prompt, its steps or its attention, one activity line and its
//! answer. A line form, for a piped `ariadne attach`: one `kind · summary`
//! line for each item.

use std::fmt::Display;

use ariadne_api::trajectory::{
    ActivityDto, CompactionStatus, ContentBlockDto, MAIN_LANE, NoteSeverity, PermissionState,
    PermissionStepDto, QuestionState, StepDto, ToolCallStatus, TurnDto, TurnState,
};
use chrono::{DateTime, TimeZone};

/// The text of content blocks. A block that is not text reads as a short
/// placeholder on a line of its own.
pub fn text(blocks: &[ContentBlockDto]) -> String {
    let mut out = String::new();
    for block in blocks {
        let placeholder = match block {
            ContentBlockDto::Text { text } => {
                out.push_str(text);
                continue;
            }
            ContentBlockDto::Resource {
                text: Some(text), ..
            } => {
                out.push_str(text);
                continue;
            }
            ContentBlockDto::Image { .. } => "[image]".to_string(),
            ContentBlockDto::Audio { .. } => "[audio]".to_string(),
            ContentBlockDto::Resource { uri, .. } => format!("[resource {uri}]"),
            ContentBlockDto::ResourceLink { uri, name, .. } => format!("[link {name} {uri}]"),
        };
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&placeholder);
    }
    out
}

/// The kind of a step, as its `type` names it on the wire.
pub fn kind(step: &StepDto) -> &'static str {
    match step {
        StepDto::Message(_) => "message",
        StepDto::Thought(_) => "thought",
        StepDto::ToolCall(_) => "tool_call",
        StepDto::Permission(_) => "permission",
        StepDto::Question(_) => "question",
        StepDto::Note(_) => "note",
        StepDto::Error(_) => "error",
        StepDto::Subagent(_) => "subagent",
        StepDto::Compaction(_) => "compaction",
    }
}

/// Whether a step is ready to print: text that no longer streams, a call
/// or a subagent or a compaction that ended, and every permission, question,
/// note and error as it comes. A permission and a question print again
/// when they close, by [`Printer::answered`].
pub fn settled(step: &StepDto) -> bool {
    match step {
        StepDto::Message(step) => !step.streaming,
        StepDto::Thought(step) => !step.streaming,
        StepDto::ToolCall(step) => !matches!(
            step.status,
            ToolCallStatus::Pending | ToolCallStatus::Running
        ),
        StepDto::Subagent(step) => {
            step.ended_at.is_some()
                || matches!(
                    step.state.as_deref(),
                    Some("completed" | "failed" | "cancelled")
                )
        }
        StepDto::Compaction(step) => step.status != CompactionStatus::Running,
        StepDto::Permission(_) | StepDto::Question(_) | StepDto::Note(_) | StepDto::Error(_) => {
            true
        }
    }
}

/// Writes turns and steps, with each time in `zone`.
#[derive(Debug, Clone)]
pub struct Printer<Tz> {
    zone: Tz,
}

impl<Tz> Printer<Tz>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    pub fn new(zone: Tz) -> Self {
        Self { zone }
    }

    /// A whole turn: its prompt, then `steps` where given and its attention
    /// otherwise, then its activity and its answer.
    pub fn turn(&self, turn: &TurnDto, steps: Option<&[StepDto]>) -> String {
        let mut out = self.opened(turn);
        match steps {
            Some(steps) => {
                for step in steps {
                    out.push_str(&self.step(step));
                }
            }
            None => {
                for step in oldest_first(&turn.attention) {
                    out.push_str(&self.step(step));
                }
            }
        }
        out.push_str(&self.closed(turn, steps.is_some()));
        out
    }

    /// The head of a turn: its prompt.
    pub fn opened(&self, turn: &TurnDto) -> String {
        let at = self.clock(&turn.started_at);
        match &turn.prompt {
            Some(prompt) => {
                let label = match prompt.source.as_deref() {
                    Some(source) if source != "console" => format!("USER/{source}"),
                    _ => "USER".to_string(),
                };
                labelled(&at, &label, &text(&prompt.content))
            }
            None => labelled(&at, "SESSION", "before the first prompt"),
        }
    }

    /// The end of a turn: its activity, the count of the attention steps
    /// it does not show, and its answer.
    pub fn closed(&self, turn: &TurnDto, shows_steps: bool) -> String {
        let mut out = format!("  {}\n", activity(turn));
        if !shows_steps && turn.attention_more > 0 {
            out.push_str(&format!(
                "  {} more attention {}: --steps shows {}\n",
                turn.attention_more,
                plural(turn.attention_more, "step", "steps"),
                plural(turn.attention_more, "it", "them"),
            ));
        }
        let answer = text(&turn.answer);
        if !answer.trim().is_empty() {
            let at = self.clock(turn.ended_at.as_deref().unwrap_or(&turn.started_at));
            out.push_str(&labelled(&at, "AGENT", &answer));
        }
        out.push('\n');
        out
    }

    /// One step under its turn: its time, its kind and what it says.
    pub fn step(&self, step: &StepDto) -> String {
        let (head, rest) = body(step);
        let mut out = format!(
            "  {}  {}  {head}\n",
            self.clock(super::step_at(step)),
            kind(step)
        );
        for line in rest {
            out.push_str("    ");
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    /// How a permission or a question closed, or the diagnosis an error got, under the step printed while
    /// it waited. None while it waits, and for every other step.
    pub fn answered(&self, step: &StepDto) -> Option<String> {
        closing(step).map(|line| format!("    {line}\n"))
    }

    /// The line that names a turn, over a step printed after the turn
    /// closed: its time and the start of its prompt.
    pub fn owner(&self, turn: &TurnDto) -> String {
        let prompt = match &turn.prompt {
            Some(prompt) => collapsed(&text(&prompt.content)),
            None => "before the first prompt".to_string(),
        };
        let prompt = match prompt.char_indices().nth(60) {
            Some((at, _)) => format!("{}…", &prompt[..at]),
            None => prompt,
        };
        format!(
            "  in the turn of {}: {prompt}\n",
            self.clock(&turn.started_at)
        )
    }

    /// The line form of a turn's prompt.
    pub fn opened_line(&self, turn: &TurnDto) -> String {
        match &turn.prompt {
            Some(prompt) => format!("prompt · {}\n", collapsed(&text(&prompt.content))),
            None => "session · before the first prompt\n".to_string(),
        }
    }

    /// The line form of a step. A permission that waits adds its options,
    /// numbered, one in each line.
    pub fn step_line(&self, step: &StepDto) -> String {
        // Text is whole on its line. The other steps keep their head line:
        // a preview and the options are not what the line says.
        let (head, rest) = body(step);
        let head = match step {
            StepDto::Message(_) | StepDto::Thought(_) | StepDto::Note(_) => std::iter::once(head)
                .chain(rest)
                .collect::<Vec<_>>()
                .join(" "),
            _ => head,
        };
        let mut out = format!("{} · {}\n", kind(step), collapsed(&head));
        if let StepDto::Permission(permission) = step
            && permission.state == PermissionState::Pending
        {
            out.push_str(&options(permission));
        }
        out
    }

    /// The line form of how a permission or a question closed.
    pub fn answered_line(&self, step: &StepDto) -> Option<String> {
        closing(step).map(|line| format!("{} · {}\n", kind(step), collapsed(&line)))
    }

    /// The line form of a turn's end. `with_answer` adds the answer, for a
    /// turn whose steps were not printed.
    pub fn closed_line(&self, turn: &TurnDto, with_answer: bool) -> String {
        let mut out = format!("turn · {}\n", activity(turn));
        let answer = text(&turn.answer);
        if with_answer && !answer.trim().is_empty() {
            out.push_str(&format!("answer · {}\n", collapsed(&answer)));
        }
        out
    }

    fn clock(&self, timestamp: &str) -> String {
        DateTime::parse_from_rfc3339(timestamp)
            .map(|at| at.with_timezone(&self.zone).format("%H:%M:%S").to_string())
            .unwrap_or_else(|_| timestamp.to_string())
    }
}

/// The numbered options of a permission, one in each line.
fn options(permission: &PermissionStepDto) -> String {
    permission
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("  {}. {}\n", index + 1, option.name))
        .collect()
}

/// The state of a turn and what it did, counted, in one line.
pub(crate) fn activity(turn: &TurnDto) -> String {
    let state = match turn.state {
        TurnState::Running => "running".to_string(),
        TurnState::Completed => match turn.stop_reason.as_deref() {
            Some(reason) if reason != "end_turn" => format!("completed ({reason})"),
            _ => "completed".to_string(),
        },
        TurnState::Cancelled => "cancelled".to_string(),
        TurnState::Failed => "failed".to_string(),
        TurnState::Interrupted => "interrupted".to_string(),
    };
    let mut parts = vec![state];
    parts.extend(counts(&turn.activity));
    parts.join(" · ")
}

/// The counts of a turn, with no word for its state.
pub(crate) fn counts_line(turn: &TurnDto) -> String {
    counts(&turn.activity).join(" · ")
}

fn counts(activity: &ActivityDto) -> Vec<String> {
    [
        (activity.steps, "step", "steps"),
        (activity.tool_calls, "tool call", "tool calls"),
        (activity.files_changed, "file changed", "files changed"),
        (activity.failed, "failed", "failed"),
        (activity.cancelled, "cancelled", "cancelled"),
        (activity.thoughts, "thought", "thoughts"),
        (activity.subagents, "subagent", "subagents"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, many)| format!("{count} {}", plural(count, one, many)))
    .collect()
}

fn plural<'a>(count: u64, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// The first line of a step, and the lines under it.
fn body(step: &StepDto) -> (String, Vec<String>) {
    let (head, rest) = match step {
        StepDto::Message(step) => split(&text(&step.content)),
        StepDto::Thought(step) => split(&text(&step.content)),
        StepDto::ToolCall(call) => {
            let mut head = call.title.clone();
            if let Some(subject) = call.subject.as_deref().filter(|subject| *subject != head) {
                head.push_str(&format!(" · {subject}"));
            }
            head.push_str(&format!(" · {}", tool_status(call.status)));
            if let Some(diff) = call.diff_stat {
                head.push_str(&format!(
                    " · {} {} +{} -{}",
                    diff.files,
                    plural(diff.files, "file", "files"),
                    diff.added,
                    diff.removed
                ));
            }
            let rest = call
                .preview
                .as_deref()
                .map(|preview| preview.lines().map(str::to_string).collect())
                .unwrap_or_default();
            (head, rest)
        }
        StepDto::Permission(permission) => {
            let mut rest = Vec::new();
            if let Some(ai) = &permission.ai {
                rest.push(ai.clone());
            }
            match permission.state {
                PermissionState::Pending => rest.extend(
                    permission
                        .options
                        .iter()
                        .enumerate()
                        .map(|(index, option)| format!("{}. {}", index + 1, option.name)),
                ),
                _ => rest.extend(closing(step)),
            }
            (permission.question.clone(), rest)
        }
        StepDto::Question(question) => {
            let mut rest = Vec::new();
            if !question.hint.is_empty() {
                rest.push(question.hint.clone());
            }
            if question.state == QuestionState::Open {
                for (index, option) in question.options.iter().enumerate() {
                    let mut line = format!("{}. {}", index + 1, option.label);
                    if let Some(description) = &option.description {
                        line.push_str(&format!(" — {description}"));
                    }
                    rest.push(line);
                }
            } else {
                rest.extend(closing(step));
            }
            (question.question.clone(), rest)
        }
        StepDto::Note(note) => {
            let (head, rest) = split(&note.text);
            match note.severity {
                NoteSeverity::Info => (head, rest),
                NoteSeverity::Warning => (format!("warning: {head}"), rest),
            }
        }
        StepDto::Error(error) => {
            let (head, mut rest) = split(&error.text);
            if let Some(diagnosis) = &error.diagnosis {
                rest.push(format!("diagnosis: {diagnosis}"));
            }
            (head, rest)
        }
        StepDto::Subagent(subagent) => {
            let mut head = subagent.title.clone();
            if let Some(state) = &subagent.state {
                head.push_str(&format!(" · {state}"));
            }
            let counts = counts(&subagent.activity);
            if !counts.is_empty() {
                head.push_str(&format!(" · {}", counts.join(" · ")));
            }
            (head, subagent.task.iter().cloned().collect())
        }
        StepDto::Compaction(compaction) => {
            let status = match compaction.status {
                CompactionStatus::Running => "running",
                CompactionStatus::Completed => "completed",
                CompactionStatus::Failed => "failed",
            };
            let mut rest: Vec<String> = text(&compaction.summary)
                .lines()
                .map(str::to_string)
                .collect();
            rest.extend(compaction.error.clone());
            (status.to_string(), rest)
        }
    };
    match lane(step) {
        MAIN_LANE => (head, rest),
        lane => (format!("{lane}: {head}"), rest),
    }
}

/// How a closed permission or question ended, in one line.
fn closing(step: &StepDto) -> Option<String> {
    match step {
        StepDto::Permission(permission) => match permission.state {
            PermissionState::Pending => None,
            PermissionState::Cancelled => Some("cancelled".to_string()),
            PermissionState::Answered => Some(match &permission.answer {
                Some(answer) => match answer.decided_by.as_deref() {
                    Some(by) => format!("answer: {} ({by})", answer.label),
                    None => format!("answer: {}", answer.label),
                },
                None => "answered".to_string(),
            }),
        },
        StepDto::Question(question) => match question.state {
            QuestionState::Open => None,
            QuestionState::Answered => Some(format!(
                "answer: {}",
                question.answer.as_deref().unwrap_or_default()
            )),
            QuestionState::Declined => Some("declined".to_string()),
            QuestionState::Cancelled => Some("cancelled".to_string()),
        },
        // The diagnosis of an error comes after the error: it closes it.
        StepDto::Error(error) => error
            .diagnosis
            .as_ref()
            .map(|diagnosis| format!("diagnosis: {diagnosis}")),
        _ => None,
    }
}

fn tool_status(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::Running => "running",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        ToolCallStatus::Cancelled => "cancelled",
        ToolCallStatus::Interrupted => "interrupted",
    }
}

fn split(text: &str) -> (String, Vec<String>) {
    let mut lines = text.trim_end_matches('\n').lines().map(str::to_string);
    (lines.next().unwrap_or_default(), lines.collect())
}

/// A text on one line, its runs of white space as one space.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A label after a time, the text after it, and the lines of the text
/// under its first line.
fn labelled(at: &str, label: &str, text: &str) -> String {
    let (head, rest) = split(text);
    let mut out = format!("{at}  {label}  {head}\n");
    let indent = " ".repeat(at.chars().count() + 2 + label.chars().count() + 2);
    for line in rest {
        out.push_str(&indent);
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Attention holds the waiting steps and then the errors, each newest
/// first. A turn shows them in the order they came.
fn oldest_first(steps: &[StepDto]) -> Vec<&StepDto> {
    let mut steps: Vec<_> = steps.iter().collect();
    steps.sort_by(|a, b| {
        super::step_at(a)
            .cmp(super::step_at(b))
            .then_with(|| super::step_id(a).cmp(super::step_id(b)))
    });
    steps
}

fn lane(step: &StepDto) -> &str {
    match step {
        StepDto::Message(step) => &step.lane,
        StepDto::Thought(step) => &step.lane,
        StepDto::ToolCall(step) => &step.lane,
        StepDto::Permission(step) => &step.lane,
        StepDto::Question(step) => &step.lane,
        StepDto::Note(step) => &step.lane,
        StepDto::Error(step) => &step.lane,
        StepDto::Subagent(step) => &step.lane,
        StepDto::Compaction(step) => &step.lane,
    }
}

#[cfg(test)]
mod tests {
    use ariadne_api::trajectory::TurnDto;
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::trajectory::fixtures::{step, turn};

    fn typed_turn(value: serde_json::Value) -> TurnDto {
        serde_json::from_value(value).unwrap()
    }

    fn typed_step(value: serde_json::Value) -> StepDto {
        serde_json::from_value(value).unwrap()
    }

    fn printer() -> Printer<Utc> {
        Printer::new(Utc)
    }

    #[test]
    fn a_turn_prints_its_prompt_one_activity_line_and_its_answer() {
        let turn = typed_turn(turn(
            "t1",
            "completed",
            json!({
                "prompt": {"id": "t1", "source": "console", "prompt_kind": null,
                           "content": [{"type": "text", "text": "Run the tests"}]},
                "stop_reason": "end_turn",
                "ended_at": "2026-11-27T10:02:00Z",
                "activity": {"steps": 4, "tool_calls": 3, "files_changed": 1, "failed": 1,
                             "cancelled": 0, "thoughts": 2, "subagents": 1},
                "answer": [{"type": "text", "text": "All tests pass."}]
            }),
        ));

        assert_eq!(
            printer().turn(&turn, None),
            "10:00:00  USER  Run the tests\n\
             \x20 completed · 4 steps · 3 tool calls · 1 file changed · 1 failed · 2 thoughts · 1 subagent\n\
             10:02:00  AGENT  All tests pass.\n\
             \n"
        );
    }

    #[test]
    fn a_turn_shows_its_attention_and_counts_the_attention_it_does_not_show() {
        let turn = typed_turn(turn(
            "t1",
            "failed",
            json!({
                "attention": [
                    step("error", "e2", "t1", json!({"text": "second", "created_at": "2026-11-27T10:00:02Z"})),
                    step("error", "e1", "t1", json!({"text": "first", "created_at": "2026-11-27T10:00:01Z",
                                                     "diagnosis": "The disk is full."})),
                ],
                "attention_more": 3
            }),
        ));

        assert_eq!(
            printer().turn(&turn, None),
            "10:00:00  USER  prompt t1\n\
             \x20 10:00:01  error  first\n\
             \x20   diagnosis: The disk is full.\n\
             \x20 10:00:02  error  second\n\
             \x20 failed\n\
             \x20 3 more attention steps: --steps shows them\n\
             \n"
        );
    }

    #[test]
    fn given_steps_a_turn_prints_each_under_it_and_no_count_of_hidden_attention() {
        let turn = typed_turn(turn("t1", "completed", json!({"attention_more": 2})));
        let steps = [
            typed_step(step(
                "tool_call",
                "c1",
                "t1",
                json!({"title": "Bash", "subject": "cargo test", "status": "failed",
                       "preview": "error[E0425]\nnot found",
                       "diff_stat": {"files": 2, "added": 5, "removed": 1}}),
            )),
            typed_step(step(
                "message",
                "m1",
                "t1",
                json!({"lane": "sub-1", "content": [{"type": "text", "text": "Found it."}]}),
            )),
        ];

        assert_eq!(
            printer().turn(&turn, Some(&steps)),
            "10:00:00  USER  prompt t1\n\
             \x20 10:00:00  tool_call  Bash · cargo test · failed · 2 files +5 -1\n\
             \x20   error[E0425]\n\
             \x20   not found\n\
             \x20 10:00:00  message  sub-1: Found it.\n\
             \x20 completed\n\
             \n"
        );
    }

    #[test]
    fn a_permission_prints_its_options_while_it_waits_and_its_answer_once_answered() {
        let pending = typed_step(step("permission", "p1", "t1", json!({})));
        let answered = typed_step(step(
            "permission",
            "p1",
            "t1",
            json!({"state": "answered",
                   "answer": {"option_id": "yes", "label": "Allow", "decided_by": "ai"}}),
        ));

        assert_eq!(
            printer().step(&pending),
            "  10:00:00  permission  Allow Bash?\n    1. Allow\n    2. Reject\n"
        );
        assert_eq!(printer().answered(&pending), None);
        assert_eq!(
            printer().answered(&answered).as_deref(),
            Some("    answer: Allow (ai)\n")
        );
        assert_eq!(
            printer().step_line(&pending),
            "permission · Allow Bash?\n  1. Allow\n  2. Reject\n"
        );
        assert_eq!(
            printer().answered_line(&answered).as_deref(),
            Some("permission · answer: Allow (ai)\n")
        );
    }

    #[test]
    fn a_permission_says_why_the_ai_permission_model_had_a_part() {
        let waiting = typed_step(step(
            "permission",
            "p1",
            "t1",
            json!({"ai": "AI said ask · allow 62%, deny 5% · danger 41% (allow up to 20%, deny from 80%)"}),
        ));
        let decided = typed_step(step(
            "permission",
            "p1",
            "t1",
            json!({"state": "answered",
                   "ai": "AI allowed · allow 91%, deny 1% · danger 4% (allow up to 5%, deny from 65%)",
                   "answer": {"option_id": "yes", "label": "Allow", "decided_by": "ai"}}),
        ));

        assert_eq!(
            printer().step(&waiting),
            "  10:00:00  permission  Allow Bash?\n\
             \x20   AI said ask · allow 62%, deny 5% · danger 41% (allow up to 20%, deny from 80%)\n\
             \x20   1. Allow\n\
             \x20   2. Reject\n"
        );
        assert_eq!(
            printer().step(&decided),
            "  10:00:00  permission  Allow Bash?\n\
             \x20   AI allowed · allow 91%, deny 1% · danger 4% (allow up to 5%, deny from 65%)\n\
             \x20   answer: Allow (ai)\n"
        );
    }

    #[test]
    fn a_question_prints_its_options_while_open_and_its_answer_once_closed() {
        let open = typed_step(step(
            "question",
            "q1",
            "t1",
            json!({"hint": "Which base branch?", "options": [
                {"id": "main", "label": "Main", "description": null},
                {"id": "next", "label": "Next — the *release* line", "description": "Cut on Fridays"}
            ]}),
        ));
        let answered = typed_step(step(
            "question",
            "q1",
            "t1",
            json!({"hint": "", "state": "answered", "answer": "base: next"}),
        ));
        let declined = typed_step(step(
            "question",
            "q1",
            "t1",
            json!({"hint": "", "state": "declined"}),
        ));

        assert_eq!(
            printer().step(&open),
            "  10:00:00  question  Which base?\n\
             \x20   Which base branch?\n\
             \x20   1. Main\n\
             \x20   2. Next — the *release* line — Cut on Fridays\n"
        );
        assert_eq!(
            printer().step(&answered),
            "  10:00:00  question  Which base?\n    answer: base: next\n"
        );
        assert_eq!(printer().answered(&open), None);
        assert_eq!(
            printer().answered(&declined).as_deref(),
            Some("    declined\n")
        );
    }

    #[test]
    fn an_owner_line_names_a_turn_by_its_time_and_the_start_of_its_prompt() {
        let short = typed_turn(turn("t1", "completed", json!({})));
        let long = typed_turn(turn(
            "t2",
            "completed",
            json!({"prompt": {"id": "t2", "source": "console", "prompt_kind": null,
                              "content": [{"type": "text", "text": "word ".repeat(20)}]}}),
        ));

        assert_eq!(
            printer().owner(&short),
            "  in the turn of 10:00:00: prompt t1\n"
        );
        assert_eq!(
            printer().owner(&long),
            format!("  in the turn of 10:00:00: {}…\n", "word ".repeat(12))
        );
    }

    #[test]
    fn the_line_form_puts_each_item_on_one_line() {
        let turn = typed_turn(turn(
            "t1",
            "completed",
            json!({"activity": {"steps": 1, "tool_calls": 1, "files_changed": 0, "failed": 0,
                                "cancelled": 0, "thoughts": 0, "subagents": 0},
                   "answer": [{"type": "text", "text": "Done.\n\nAll green."}]}),
        ));
        let message = typed_step(step(
            "message",
            "m1",
            "t1",
            json!({"content": [{"type": "text", "text": "Two\nlines"}]}),
        ));

        assert_eq!(printer().opened_line(&turn), "prompt · prompt t1\n");
        assert_eq!(printer().step_line(&message), "message · Two lines\n");
        assert_eq!(
            printer().closed_line(&turn, true),
            "turn · completed · 1 step · 1 tool call\nanswer · Done. All green.\n"
        );
        assert_eq!(
            printer().closed_line(&turn, false),
            "turn · completed · 1 step · 1 tool call\n"
        );
    }

    #[test]
    fn a_step_is_settled_once_it_stops_changing() {
        for (value, expected) in [
            (step("message", "m", "t", json!({"streaming": true})), false),
            (step("message", "m", "t", json!({"streaming": false})), true),
            (
                step("tool_call", "c", "t", json!({"status": "running"})),
                false,
            ),
            (
                step("tool_call", "c", "t", json!({"status": "interrupted"})),
                true,
            ),
            (
                step("compaction", "k", "t", json!({"status": "running"})),
                false,
            ),
            (
                step(
                    "subagent",
                    "s",
                    "t",
                    json!({"state": "running", "ended_at": null}),
                ),
                false,
            ),
            (step("permission", "p", "t", json!({})), true),
        ] {
            let typed = typed_step(value.clone());
            assert_eq!(settled(&typed), expected, "{value}");
        }
    }
}
