//! A question the agent asked with its own tool (021), as the form that
//! answers it: one field at a time, above the input.
//!
//! Its fields are read off the question the host lists ([`crate::fields`]),
//! since the question step of the trajectory names its first choice field
//! alone.
//!
//! A choice field takes the arrows, a number, Space for a multi-select, and
//! Enter. A text field takes the text in the input box on Enter. The last
//! field's Enter sends the answer, and Esc declines the question. While the
//! box holds text on a choice field with a companion, Enter takes a custom answer.

use std::collections::BTreeSet;

use ariadne_api::sessions::{AnswerQuestionRequest, QuestionAction};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use serde_json::{Map, Value};
use unicode_width::UnicodeWidthStr;

use crate::fields::{self, FieldKind, FormChoice, FormField};
use crate::theme::{self, ASK, DIM, FAIL, FRAME, PICKED, QUESTION};

use super::{Action, App};
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
fn draw(
    message: &str,
    fields: &[FormField],
    state: &FormState,
    width: usize,
    most: usize,
) -> Vec<Line<'static>> {
    if most == 0 {
        return Vec::new();
    }
    let width = width.max(8);
    let all_fields = fields;
    let fields = crate::fields::questions(fields);
    let Some(field) = fields.get(state.field) else {
        return vec![rule(width)];
    };
    let title = if field.title.is_empty() || field.title == message {
        QUESTION_LABEL
    } else {
        &field.title
    };
    let mut header = title.to_string();
    if fields.len() > 1 {
        header.push_str(&format!(" · {} of {}", state.field + 1, fields.len()));
        for question in &fields {
            let answered = state.content.contains_key(&question.key)
                || crate::fields::companion(all_fields, question)
                    .is_some_and(|c| state.content.contains_key(&c.key));
            header.push_str(if answered { " ✓" } else { " ○" });
        }
    }
    let mut lines = hung(&header, theme::OPTION_PICKED, ASK, QUESTION, width);
    lines.extend(hung(
        field.description.as_deref().unwrap_or(message),
        "",
        QUESTION,
        QUESTION,
        width,
    ));
    let mut selected = None;
    if let FieldKind::Single(choices) | FieldKind::Multi(choices) = &field.kind {
        for (index, choice) in choices.iter().enumerate() {
            if index == state.picked {
                selected = Some(lines.len());
            }
            lines.extend(choice_lines(
                index,
                choice,
                state.picked == index,
                matches!(field.kind, FieldKind::Multi(_)).then(|| state.chosen.contains(&index)),
                width,
            ));
        }
        if crate::fields::companion(all_fields, field).is_some() {
            if choices.len() == state.picked {
                selected = Some(lines.len());
            }
            lines.extend(choice_lines(
                choices.len(),
                &FormChoice {
                    title: "Type your own answer…".into(),
                    description: None,
                    value: Value::Null,
                },
                state.picked == choices.len(),
                None,
                width,
            ));
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
    if lines.len() > most {
        // Keep the field title and leave enough room for the highlighted choice.
        let tail = lines.len() - most + 1;
        let start = selected.map_or(tail, |row| tail.min(row));
        lines.drain(1..start);
        lines.truncate(most);
    }
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
    let suffix = "(recommended)";
    let recommended = choice.title.to_lowercase().ends_with(suffix);
    let title = if recommended {
        choice.title[..choice.title.len() - suffix.len()].trim_end()
    } else {
        &choice.title
    };
    let style = if highlighted { PICKED } else { Style::new() };
    let mut lines = hung(title, &lead, style, style, width);
    if recommended {
        lines.extend(hung("recommended", "    ", ASK, ASK, width));
    }
    if let Some(description) = &choice.description {
        lines.extend(hung(description, "    ", DIM, DIM, width));
    }
    lines
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
    crate::markdown::plain(text, width.saturating_sub(hang), style)
        .into_iter()
        .enumerate()
        .map(|(row, mut line)| {
            let lead = if row == 0 {
                lead.to_string()
            } else {
                " ".repeat(hang)
            };
            line.spans.insert(0, Span::styled(lead, lead_style));
            line
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

impl App {
    /// The form's state for the question `question_id`: fresh for a
    /// question it has not seen.
    fn form_state(&self, question_id: &str) -> FormState {
        match self.form.question_id == question_id {
            true => self.form.clone(),
            false => FormState::new(question_id),
        }
    }

    /// The open question as the form draws it, above the input.
    pub(super) fn form_lines(&self, width: usize, most: usize) -> Option<Vec<Line<'static>>> {
        let (question, form) = self.question()?;
        Some(match form {
            Some((message, fields)) => draw(
                &message,
                &fields,
                &self.form_state(&question.request_id),
                width,
                most,
            ),
            None => {
                let mut lines = hung(
                    &question.question,
                    theme::OPTION_PICKED,
                    ASK,
                    QUESTION,
                    width,
                );
                lines.extend(hung("reading the question…", "  ", DIM, DIM, width));
                lines.truncate(most);
                lines
            }
        })
    }

    /// One key on the open question, or `None` where the key is the input
    /// box's: text typed on a choice field is a prompt.
    pub(super) fn form_key(&mut self, key: KeyEvent) -> Option<Action> {
        let (question_id, all_fields) = match self.question()? {
            (question, Some((_, fields))) => (question.request_id.clone(), fields),
            // The form is still being read: Esc declines all the same.
            (question, None) if key.code == KeyCode::Esc => {
                (question.request_id.clone(), Vec::new())
            }
            (_, None) => return None,
        };
        let fields = fields::questions(&all_fields);
        let mut state = self.form_state(&question_id);
        if key.code == KeyCode::Esc {
            self.form = FormState::default();
            return Some(Action::Answer {
                question_id,
                answer: AnswerQuestionRequest {
                    action: QuestionAction::Decline,
                    content: None,
                },
            });
        }
        if fields.is_empty() {
            // A form with no fields has nothing to take: Enter accepts it.
            if key.code == KeyCode::Enter && key.modifiers.is_empty() && self.input.is_empty() {
                self.form = FormState::default();
                return Some(Action::Answer {
                    question_id,
                    answer: AnswerQuestionRequest {
                        action: QuestionAction::Accept,
                        content: Some(Default::default()),
                    },
                });
            }
            return None;
        }
        let field = fields.get(state.field)?;
        if matches!(key.code, KeyCode::Tab | KeyCode::Left | KeyCode::Right)
            && key.modifiers.is_empty()
            && self.input.is_empty()
        {
            state.field = if key.code == KeyCode::Left {
                (state.field + fields.len() - 1) % fields.len()
            } else {
                (state.field + 1) % fields.len()
            };
            state.picked = 0;
            state.chosen.clear();
            let target = &fields[state.field];
            if let Some(value) = state.content.get(&target.key) {
                match &target.kind {
                    fields::FieldKind::Single(choices) => {
                        state.picked = choices.iter().position(|c| &c.value == value).unwrap_or(0);
                    }
                    fields::FieldKind::Multi(choices) => {
                        for (index, choice) in choices.iter().enumerate() {
                            if value
                                .as_array()
                                .is_some_and(|values| values.contains(&choice.value))
                            {
                                state.chosen.insert(index);
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.form = state;
            return Some(Action::None);
        }
        let custom = fields::companion(&all_fields, field);
        let count = choice_count(field) + usize::from(custom.is_some());
        let typing = count == 0;
        let boxed = !self.input.is_empty();
        match key.code {
            KeyCode::Up if !typing && !boxed => {
                state.picked = state.picked.saturating_sub(1);
            }
            KeyCode::Down if !typing && !boxed => {
                state.picked = (state.picked + 1).min(count.saturating_sub(1));
            }
            KeyCode::Char(digit @ '1'..='9') if !typing && !boxed => {
                let index = digit as usize - '1' as usize;
                if index < count {
                    state.picked = index;
                    if matches!(field.kind, fields::FieldKind::Single(_))
                        && index < choice_count(field)
                    {
                        self.form = state;
                        return self.form_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                }
            }
            KeyCode::Char(' ') if !boxed && matches!(field.kind, fields::FieldKind::Multi(_)) => {
                if !state.chosen.remove(&state.picked) {
                    state.chosen.insert(state.picked);
                }
            }
            KeyCode::Enter
                if key.modifiers.is_empty() && (typing || !boxed || custom.is_some()) =>
            {
                let typed = if typing || custom.is_some() {
                    self.input.take()
                } else {
                    None
                };
                let custom_text = custom.zip(typed.as_deref());
                let result = if let Some((custom, text)) = custom_text {
                    state.content.insert(
                        custom.key.clone(),
                        serde_json::Value::String(text.to_string()),
                    );
                    if matches!(field.kind, fields::FieldKind::Single(_)) || state.chosen.is_empty()
                    {
                        state.content.remove(&field.key);
                        Ok(None)
                    } else {
                        take_value(field, &state, None)
                    }
                } else if custom.is_some() && state.picked == count - 1 {
                    Err("Type your own answer in the box.".into())
                } else {
                    if let Some(custom) = custom {
                        state.content.remove(&custom.key);
                    }
                    take_value(field, &state, typed.as_deref())
                };
                match result {
                    Ok(value) => {
                        if let Some(value) = value {
                            state.content.insert(field.key.clone(), value);
                        }
                        state.next();
                    }
                    Err(problem) => state.problem = Some(problem),
                }
                if state.field >= fields.len()
                    && let Some((index, missing)) = fields.iter().enumerate().find(|(_, field)| {
                        field.required
                            && !state.content.contains_key(&field.key)
                            && !fields::companion(&all_fields, field)
                                .is_some_and(|custom| state.content.contains_key(&custom.key))
                    })
                {
                    state.field = index;
                    state.problem = Some(format!("{} needs an answer.", missing.title));
                }
                if state.field >= fields.len() {
                    self.form = FormState::default();
                    return Some(Action::Answer {
                        question_id,
                        answer: AnswerQuestionRequest {
                            action: QuestionAction::Accept,
                            content: Some(state.content),
                        },
                    });
                }
            }
            _ => return None,
        }
        self.form = state;
        Some(Action::None)
    }
}
