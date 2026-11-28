//! The fields of a question's form, read off its `requestedSchema`, for the
//! full-screen console's form.

use serde_json::Value;

/// One field of a question's form, read off its `requestedSchema`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FormField {
    /// The property name, which the answer's content is keyed by.
    pub key: String,
    /// The field's `title`, else its key.
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: FieldKind,
}

/// What a field takes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FieldKind {
    /// One of the choices: a `string` with `oneOf` or `enum`, or a boolean.
    Single(Vec<FormChoice>),
    /// Any of the choices: an `array` with `items.anyOf` or `items.enum`.
    Multi(Vec<FormChoice>),
    /// Free text.
    Text,
    /// A number, typed.
    Number { integer: bool },
}

/// One choice of a field: the value the answer carries, and its text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FormChoice {
    pub value: Value,
    pub title: String,
    pub description: Option<String>,
}

/// Pair only an adjacent plain string companion with its choice question.
pub(crate) fn companion<'a>(fields: &'a [FormField], field: &FormField) -> Option<&'a FormField> {
    let at = fields.iter().position(|f| f.key == field.key)?;
    let next = fields.get(at + 1)?;
    (matches!(field.kind, FieldKind::Single(_) | FieldKind::Multi(_))
        && next.key == format!("{}_custom", field.key)
        && matches!(next.kind, FieldKind::Text))
    .then_some(next)
}

pub(crate) fn questions(fields: &[FormField]) -> Vec<FormField> {
    fields
        .iter()
        .enumerate()
        .filter(|(at, _)| *at == 0 || companion(fields, &fields[*at - 1]).is_none())
        .map(|(_, field)| field.clone())
        .collect()
}

/// The fields of a form, in the order the schema lists them.
pub(crate) fn form_fields(schema: &Value, message: &str) -> Vec<FormField> {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, property)| FormField {
            key: key.clone(),
            title: string_at(property, "/title").unwrap_or_else(|| message.to_string()),
            description: string_at(property, "/description"),
            required: required.contains(&key.as_str()),
            kind: field_kind(property),
        })
        .collect()
}

fn field_kind(property: &Value) -> FieldKind {
    match property.get("type").and_then(Value::as_str) {
        Some("array") => FieldKind::Multi(choices(property.get("items").unwrap_or(&Value::Null))),
        Some("boolean") => FieldKind::Single(vec![
            FormChoice {
                value: Value::Bool(true),
                title: "Yes".into(),
                description: None,
            },
            FormChoice {
                value: Value::Bool(false),
                title: "No".into(),
                description: None,
            },
        ]),
        Some(kind @ ("number" | "integer")) => FieldKind::Number {
            integer: kind == "integer",
        },
        _ => match choices(property) {
            choices if choices.is_empty() => FieldKind::Text,
            choices => FieldKind::Single(choices),
        },
    }
}

/// The choices of a schema: `oneOf` or `anyOf` with `const` and `title`, or
/// a plain `enum`, with `enumNames` where it gives them.
fn choices(schema: &Value) -> Vec<FormChoice> {
    let titled = ["oneOf", "anyOf"]
        .iter()
        .find_map(|key| schema.get(*key).and_then(Value::as_array));
    if let Some(titled) = titled {
        return titled
            .iter()
            .filter_map(|choice| {
                let value = choice.get("const")?.clone();
                Some(FormChoice {
                    title: string_at(choice, "/title").unwrap_or_else(|| match &value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    }),
                    description: string_at(choice, "/description"),
                    value,
                })
            })
            .collect();
    }
    let names = schema.get("enumNames").and_then(Value::as_array);
    schema
        .get("enum")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(at, value)| FormChoice {
            title: names
                .and_then(|names| names.get(at))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                }),
            description: None,
            value: value.clone(),
        })
        .collect()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_string)
}
