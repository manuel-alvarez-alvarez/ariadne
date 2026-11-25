//! A question the agent asked with its own tool (021), as the form that
//! answers it: one field at a time, above the input.
//!
//! A choice field takes the arrows, a number, Space for a multi-select, and
//! Enter. A text field takes the text in the input box on Enter. The last
//! field's Enter sends the answer, and Esc declines the question. While the
//! box holds text on a choice field, the keys are the box's: the text is a
//! prompt, and it waits behind the turn.

use std::collections::BTreeSet;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde_json::{Map, Value};
use unicode_width::UnicodeWidthStr;

use crate::theme::{self, ASK, DIM, FAIL, FRAME, PICKED, QUESTION};
use crate::transcript::{FieldKind, FormChoice, FormField};
use crate::wrap;

/// The label in the rule over an open question.
const QUESTION_LABEL: &str = "question";
/// A multi-select choice that is in the answer, and one that is not.
const CHECKED: &str = "[x] ";
const UNCHECKED: &str = "[ ] ";

/// How far the person is through one open question.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct FormState {
    pub(super) question_id: String,
    /// The field being answered.
    pub(super) field: usize,
    /// The highlighted choice of that field.
    pub(super) picked: usize,
    /// The choices of a multi-select that are in the answer.
    pub(super) chosen: BTreeSet<usize>,
    /// The answers given so far, by field key.
    pub(super) content: Map<String, Value>,
    /// Why the last Enter did not take the field.
    pub(super) problem: Option<String>,
}

impl FormState {
    pub(super) fn new(question_id: &str) -> Self {
        Self {
            question_id: question_id.to_string(),
            ..Self::default()
        }
    }

    /// Go on to the next field, from its first choice.
    pub(super) fn next(&mut self) {
        self.field += 1;
        self.picked = 0;
        self.chosen.clear();
        self.problem = None;
    }
}

/// The value one field takes from the person: the highlighted or chosen
/// choices, or the `typed` text. `None` for an optional field left empty.
pub(super) fn take_value(
    field: &FormField,
    state: &FormState,
    typed: Option<&str>,
) -> Result<Option<Value>, String> {
    let empty = || match field.required {
        true => Err(format!("{} needs an answer.", field.title)),
        false => Ok(None),
    };
    match &field.kind {
        FieldKind::Single(choices) => Ok(choices.get(state.picked).map(|c| c.value.clone())),
        FieldKind::Multi(choices) if state.chosen.is_empty() => match field.required {
            // A required multi-select with nothing checked takes the
            // highlighted choice, as Enter on a single-select does.
            true => Ok(choices
                .get(state.picked)
                .map(|c| Value::Array(vec![c.value.clone()]))),
            false => Ok(Some(Value::Array(Vec::new()))),
        },
        FieldKind::Multi(choices) => Ok(Some(Value::Array(
            state
                .chosen
                .iter()
                .filter_map(|at| choices.get(*at))
                .map(|c| c.value.clone())
                .collect(),
        ))),
        FieldKind::Text => match typed {
            Some(text) => Ok(Some(Value::String(text.to_string()))),
            None => empty(),
        },
        FieldKind::Number { integer } => match typed.map(str::trim) {
            None => empty(),
            Some(text) if *integer => text
                .parse::<i64>()
                .map(|n| Some(Value::from(n)))
                .map_err(|_| format!("{} takes a whole number.", field.title)),
            Some(text) => text
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(|n| Some(Value::Number(n)))
                .ok_or_else(|| format!("{} takes a number.", field.title)),
        },
    }
}

/// How many choices a field offers: none for a typed one.
pub(super) fn choice_count(field: &FormField) -> usize {
    match &field.kind {
        FieldKind::Single(choices) | FieldKind::Multi(choices) => choices.len(),
        FieldKind::Text | FieldKind::Number { .. } => 0,
    }
}

/// The open question being answered, between two rules: the agent's
/// message, the fields already answered with their answers, the field being
/// answered with its choices, and the fields still to come.
pub(super) fn draw(
    message: &str,
    fields: &[FormField],
    state: &FormState,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut lines = vec![rule(width)];
    lines.extend(
        fit(message, width)
            .into_iter()
            .map(|row| Line::from(Span::styled(row, QUESTION))),
    );
    let total = fields.len();
    for (at, field) in fields.iter().enumerate() {
        if at < state.field {
            let answer = state
                .content
                .get(&field.key)
                .map(|value| field.answer_text(value))
                .unwrap_or_else(|| "(no answer)".into());
            lines.extend(hung(
                &format!("{} → {answer}", field.title),
                theme::CALL_DONE,
                DIM,
                DIM,
                width,
            ));
            continue;
        }
        if at > state.field {
            lines.extend(hung(&field.title, theme::OPTION_IDLE, DIM, DIM, width));
            continue;
        }
        let mut head = format!("{}/{total} {}", at + 1, field.title);
        match &field.kind {
            FieldKind::Single(_) => head.push_str(" · pick one"),
            FieldKind::Multi(_) => head.push_str(" · pick any"),
            FieldKind::Text | FieldKind::Number { .. } => head.push_str(" · type it in the box"),
        }
        if !field.required {
            head.push_str(" · optional");
        }
        lines.extend(hung(&head, theme::OPTION_PICKED, ASK, QUESTION, width));
        if let Some(description) = &field.description {
            lines.extend(hung(description, "  ", DIM, DIM, width));
        }
        if let FieldKind::Single(choices) | FieldKind::Multi(choices) = &field.kind {
            let multi = matches!(field.kind, FieldKind::Multi(_));
            for (index, choice) in choices.iter().enumerate() {
                lines.extend(choice_lines(
                    index,
                    choice,
                    state.picked == index,
                    multi.then(|| state.chosen.contains(&index)),
                    width,
                ));
            }
        }
    }
    if let Some(problem) = &state.problem {
        lines.extend(
            fit(problem, width)
                .into_iter()
                .map(|row| Line::from(Span::styled(row, FAIL))),
        );
    }
    lines.push(Line::from(Span::styled(theme::RULE.repeat(width), FRAME)));
    lines
}

/// The rule over the question, with its label where the pane has room.
fn rule(width: usize) -> Line<'static> {
    let lead = format!("{} ", theme::RULE);
    let labelled = lead.width() + QUESTION_LABEL.width();
    Line::from(match width.checked_sub(labelled) {
        None => vec![Span::styled(theme::RULE.repeat(width), FRAME)],
        Some(0) => vec![Span::styled(lead, FRAME), Span::styled(QUESTION_LABEL, ASK)],
        Some(rest) => vec![
            Span::styled(lead, FRAME),
            Span::styled(QUESTION_LABEL, ASK),
            Span::styled(format!(" {}", theme::RULE.repeat(rest - 1)), FRAME),
        ],
    })
}

/// One choice, numbered from one, with its box where the field takes several,
/// and its description after it: the highlighted one marked.
fn choice_lines(
    at: usize,
    choice: &FormChoice,
    highlighted: bool,
    checked: Option<bool>,
    width: usize,
) -> Vec<Line<'static>> {
    let marker = if highlighted {
        theme::OPTION_PICKED
    } else {
        theme::OPTION_IDLE
    };
    let mut lead = format!("  {marker}{}. ", at + 1);
    match checked {
        Some(true) => lead.push_str(CHECKED),
        Some(false) => lead.push_str(UNCHECKED),
        None => {}
    }
    let text = match &choice.description {
        Some(description) => format!("{} — {description}", choice.title),
        None => choice.title.clone(),
    };
    let style = if highlighted { PICKED } else { Style::new() };
    hung(&text, &lead, style, style, width)
}

/// `text` wrapped under `lead`, its later rows hung under the first.
fn hung(
    text: &str,
    lead: &str,
    lead_style: Style,
    style: Style,
    width: usize,
) -> Vec<Line<'static>> {
    let hang = lead.width();
    fit(text, width.saturating_sub(hang))
        .into_iter()
        .enumerate()
        .map(|(row, part)| {
            let lead = if row == 0 {
                lead.to_string()
            } else {
                " ".repeat(hang)
            };
            Line::from(vec![
                Span::styled(lead, lead_style),
                Span::styled(part, style),
            ])
        })
        .collect()
}

/// `text` in rows of at most `width` columns.
fn fit(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    wrap::wrap(text, width)
        .into_iter()
        .flat_map(|line| wrap::wrap_exact(&line, width))
        .collect()
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    use ariadne_api::events::AgentEventDto;
    use ariadne_api::sessions::{AnswerQuestionRequest, QuestionAction};

    use crate::tui::testing::*;
    use crate::tui::*;

    /// The three field shapes each adapter sends: a single-select `string`
    /// with `oneOf`, a multi-select `array` with `items.anyOf`, and a plain
    /// `string`.
    fn asked() -> AgentEventDto {
        event(
            "question",
            "question",
            json!({"session_id": "agent", "question": {
                "id": "q1", "session_id": "session",
                "message": "Codex needs your input to continue.",
                "created_at": "2026-11-25T00:00:00Z",
                "schema": {"type": "object", "required": ["base"], "properties": {
                    "base": {"type": "string", "title": "Which base branch?",
                             "oneOf": [{"const": "main", "title": "Main"},
                                       {"const": "next", "title": "Next — the *release* line"}]},
                    "checks": {"type": "array", "title": "Which checks?",
                               "items": {"anyOf": [{"const": "fmt", "title": "Format"},
                                                   {"const": "clippy", "title": "Clippy"}]}},
                    "note": {"type": "string", "title": "Anything else?"},
                }},
            }}),
        )
    }

    fn resolved(action: &str, content: serde_json::Value) -> AgentEventDto {
        event(
            "question.resolved",
            "question answered",
            json!({"session_id": "agent", "question_id": "q1", "action": action,
                   "content": content, "closed_by": "user"}),
        )
    }

    fn press(console: &mut Console, code: KeyCode) -> Action {
        console.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn asking() -> Console {
        let mut console = Console::new(header());
        console.snapshot(&[asked()]);
        console
    }

    #[test]
    fn an_open_question_draws_its_fields_and_the_option_text_as_written_above_the_input() {
        let mut terminal = pane();
        let mut console = asking();
        console.show(&mut terminal).unwrap();
        let shown = shown(&terminal);

        let rows: Vec<&str> = shown.lines().collect();
        let at = |text: &str| {
            rows.iter()
                .position(|row| row.contains(text))
                .unwrap_or_else(|| panic!("{text} is shown: {shown}"))
        };
        let message = at("Codex needs your input to continue.");
        let field = at("1/3 Which base branch? · pick one");
        let option = at("2. Next — the *release* line");
        let later = at("Which checks?");
        let input = at(crate::theme::INPUT_PLACEHOLDER);
        assert!(message < field && field < option && option < later && later < input);
        assert!(shown.contains("1. Main"), "{shown}");
        assert!(shown.contains("esc decline"), "{shown}");
    }

    #[test]
    fn the_keys_answer_each_field_and_the_last_enter_sends_the_content() {
        let mut console = asking();
        assert_eq!(press(&mut console, KeyCode::Down), Action::None);
        assert_eq!(enter(&mut console), Action::None);
        assert_eq!(press(&mut console, KeyCode::Char(' ')), Action::None);
        assert_eq!(press(&mut console, KeyCode::Down), Action::None);
        assert_eq!(press(&mut console, KeyCode::Char(' ')), Action::None);
        assert_eq!(enter(&mut console), Action::None);
        type_into(&mut console, "ship it");
        assert_eq!(
            enter(&mut console),
            Action::Answer {
                question_id: "q1".into(),
                answer: AnswerQuestionRequest {
                    action: QuestionAction::Accept,
                    content: Some(
                        json!({"base": "next", "checks": ["fmt", "clippy"], "note": "ship it"})
                            .as_object()
                            .unwrap()
                            .clone()
                    ),
                },
            }
        );
    }

    #[test]
    fn escape_declines_the_question() {
        let mut console = asking();
        assert_eq!(
            press(&mut console, KeyCode::Esc),
            Action::Answer {
                question_id: "q1".into(),
                answer: AnswerQuestionRequest {
                    action: QuestionAction::Decline,
                    content: None,
                },
            }
        );
    }

    /// Text typed on a choice field is a prompt, which waits behind the
    /// turn: it answers nothing.
    #[test]
    fn text_typed_on_a_choice_field_is_sent_as_a_prompt() {
        let mut console = asking();
        type_into(&mut console, "hello");
        assert_eq!(enter(&mut console), Action::Send("hello".into()));
        let mut terminal = pane();
        console.show(&mut terminal).unwrap();
        assert!(
            shown(&terminal).contains("1/3 Which base branch?"),
            "the question is still open: {}",
            shown(&terminal)
        );
    }

    #[test]
    fn a_required_field_with_no_choice_left_is_not_taken() {
        let mut console = Console::new(header());
        let mut question = asked();
        question.payload["question"]["schema"]["properties"]["base"] =
            json!({"type": "string", "title": "Branch name"});
        console.snapshot(&[question]);
        assert_eq!(enter(&mut console), Action::None);
        let mut terminal = pane();
        console.show(&mut terminal).unwrap();
        let shown = shown(&terminal);
        assert!(shown.contains("Branch name needs an answer."), "{shown}");
        assert!(shown.contains("1/3 Branch name"), "{shown}");
    }

    #[test]
    fn a_closed_question_shows_each_answer_by_the_option_text() {
        let mut terminal = pane();
        let mut console = asking();
        console.apply(&resolved(
            "accept",
            json!({"base": "next", "checks": ["fmt", "clippy"], "note": "ship it"}),
        ));
        console.show(&mut terminal).unwrap();
        let shown = shown(&terminal);
        assert!(shown.contains("answered"), "{shown}");
        assert!(
            shown.contains("Which base branch? → Next — the *release* line"),
            "{shown}"
        );
        assert!(shown.contains("Which checks? → Format, Clippy"), "{shown}");
        assert!(shown.contains("Anything else? → ship it"), "{shown}");
        assert!(!shown.contains("esc decline"), "{shown}");
    }
}
