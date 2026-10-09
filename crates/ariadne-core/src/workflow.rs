//! The workflow document: a linear kanban of columns that replaces the fixed
//! author, reviewer and landing pipeline.
//!
//! [`parse`] reads the whole of what a workflow document says, and nothing
//! reads a document but through it: a store that saved a bad one would carry
//! an agent nobody can load. Goals and tasks do not read a [`Workflow`] yet —
//! that is a later task, the step engine.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::models::ModelRank;

/// What a step waits for before the next one may start: the author's commit,
/// a push, a merge onto the base, or a request merged by a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum StepGate {
    Committed,
    Pushed,
    Merged,
    RequestMerged,
}

crate::wire_enum! { StepGate, "step gate", [
    Committed = "committed",
    Pushed = "pushed",
    Merged = "merged",
    RequestMerged = "request_merged",
]}

impl StepGate {
    /// The word the document itself spells a gate with. It differs from the
    /// wire spelling for one variant: the document says `request-merged`,
    /// hyphenated like every other kebab-case word in it, while the wire
    /// spells it `request_merged` like every other wire enum.
    fn from_document_word(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|gate| gate.document_word() == word)
    }

    fn document_word(&self) -> &'static str {
        match self {
            Self::RequestMerged => "request-merged",
            other => other.as_str(),
        }
    }
}

/// One column of a workflow: a step an author or reviewer works through on
/// the way to the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    /// Kebab-case, unique within the workflow.
    pub id: String,
    /// The display title, named between `[` and `]` on the column line.
    pub title: String,
    /// The description lines, joined with one space.
    pub description: String,
    /// The skills an agent on this step loads.
    pub skills: Vec<String>,
    /// The model rank the step prefers, where the document named one.
    pub rank: Option<ModelRank>,
    /// What the step waits for before the next one starts, where the
    /// document named one.
    pub gate: Option<StepGate>,
}

/// A parsed workflow document: its name and the columns of its kanban, in the
/// order the document named them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    /// Kebab-case, named on the document's first line.
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

/// A document that breaks a rule of the syntax, named by the line it broke
/// the rule on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowParseError {
    /// One-based line number.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for WorkflowParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for WorkflowParseError {}

fn fail(line: usize, message: impl Into<String>) -> WorkflowParseError {
    WorkflowParseError {
        line,
        message: message.into(),
    }
}

/// Kebab-case: one or more lowercase letters and digits, separated by single
/// hyphens. No leading, trailing or doubled hyphen.
fn is_kebab_case(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// The first line of the document: `workflow <name>`, `<name>` kebab-case.
fn parse_header(line: &str, at: usize) -> Result<String, WorkflowParseError> {
    let name = line
        .strip_prefix("workflow")
        .and_then(|rest| rest.strip_prefix(' '))
        .map(str::trim)
        .filter(|name| !name.is_empty());
    match name {
        Some(name) if is_kebab_case(name) => Ok(name.to_string()),
        _ => Err(fail(
            at,
            "a workflow document starts with \"workflow <name>\", <name> in kebab-case",
        )),
    }
}

/// A column line, `<id>[<Title>]`, matched whole: `id` before the first `[`,
/// `Title` between it and the line's closing `]`.
fn parse_column_header(line: &str) -> Option<(&str, &str)> {
    let open = line.find('[')?;
    if !line.ends_with(']') {
        return None;
    }
    let id = &line[..open];
    let title = &line[open + 1..line.len() - 1];
    (!id.is_empty() && !title.is_empty()).then_some((id, title))
}

/// A `key: value` body line, matched against one `key` word alone.
fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(key)?.strip_prefix(':').map(str::trim)
}

/// The column being read: its id and title are fixed, and its body grows one
/// line at a time until the next column line or the end of the document.
struct ColumnBuilder {
    id: String,
    title: String,
    description: Vec<String>,
    skills: Option<Vec<String>>,
    rank: Option<ModelRank>,
    gate: Option<StepGate>,
}

impl ColumnBuilder {
    fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: Vec::new(),
            skills: None,
            rank: None,
            gate: None,
        }
    }

    fn add_line(&mut self, line: &str, at: usize) -> Result<(), WorkflowParseError> {
        if let Some(value) = key_value(line, "skills") {
            if self.skills.is_some() {
                return Err(fail(
                    at,
                    format!("column {} repeats the skills key", self.id),
                ));
            }
            self.skills = Some(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            );
            return Ok(());
        }
        if let Some(value) = key_value(line, "rank") {
            if self.rank.is_some() {
                return Err(fail(at, format!("column {} repeats the rank key", self.id)));
            }
            self.rank = Some(
                value
                    .parse()
                    .map_err(|e: String| fail(at, format!("column {}: {e}", self.id)))?,
            );
            return Ok(());
        }
        if let Some(value) = key_value(line, "gate") {
            if self.gate.is_some() {
                return Err(fail(at, format!("column {} repeats the gate key", self.id)));
            }
            self.gate = Some(StepGate::from_document_word(value).ok_or_else(|| {
                fail(
                    at,
                    format!("column {} names an unknown gate: {value}", self.id),
                )
            })?);
            return Ok(());
        }
        self.description.push(line.to_string());
        Ok(())
    }

    fn finish(self) -> WorkflowStep {
        WorkflowStep {
            id: self.id,
            title: self.title,
            description: self.description.join(" "),
            skills: self.skills.unwrap_or_default(),
            rank: self.rank,
            gate: self.gate,
        }
    }
}

/// Parse a workflow document, as the syntax of `specs/030-workflows.md`
/// states it: a `workflow <name>` line, then one or more columns, each a
/// `<id>[<Title>]` line and the body lines that follow it.
///
/// A document that breaks a rule is refused with the line it broke the rule
/// on and one sentence, rather than partially parsed.
pub fn parse(text: &str) -> Result<Workflow, WorkflowParseError> {
    let mut name = None;
    let mut steps: Vec<WorkflowStep> = Vec::new();
    let mut current: Option<ColumnBuilder> = None;
    let mut last_line = 0;

    for (at, raw) in text.lines().enumerate().map(|(i, l)| (i + 1, l)) {
        last_line = at;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if name.is_none() {
            name = Some(parse_header(line, at)?);
            continue;
        }
        if let Some((id, title)) = parse_column_header(line) {
            if !is_kebab_case(id) {
                return Err(fail(at, format!("column id {id} must be kebab-case")));
            }
            if let Some(column) = current.take() {
                steps.push(column.finish());
            }
            if steps.iter().any(|s| s.id == id) {
                return Err(fail(at, format!("duplicate column id: {id}")));
            }
            current = Some(ColumnBuilder::new(id, title));
            continue;
        }
        match current.as_mut() {
            Some(column) => column.add_line(line, at)?,
            None => return Err(fail(at, "a line before the first column")),
        }
    }
    if let Some(column) = current.take() {
        steps.push(column.finish());
    }
    let name = name.ok_or_else(|| {
        fail(
            last_line.max(1),
            "a workflow document starts with \"workflow <name>\", <name> in kebab-case",
        )
    })?;
    if steps.is_empty() {
        return Err(fail(
            last_line.max(1),
            "a workflow needs at least one column",
        ));
    }
    Ok(Workflow { name, steps })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = r#"workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, run the whole suite, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged
"#;

    #[test]
    fn the_shipped_develop_review_merge_document_parses() {
        let workflow = parse(SHIPPED).unwrap();
        assert_eq!(workflow.name, "develop-review-merge");
        assert_eq!(workflow.steps.len(), 3);

        let develop = &workflow.steps[0];
        assert_eq!(develop.id, "develop");
        assert_eq!(develop.title, "Develop");
        assert_eq!(
            develop.description,
            "Build the task on its branch and commit it."
        );
        assert_eq!(develop.skills, vec!["coding".to_string()]);
        assert_eq!(develop.rank, Some(ModelRank::Balanced));
        assert_eq!(develop.gate, Some(StepGate::Committed));

        let review = &workflow.steps[1];
        assert_eq!(
            review.description,
            "Run the whole suite and judge the change against the task and the repository \
             rules. Fail the step with the changes to make."
        );
        assert_eq!(review.rank, Some(ModelRank::Frontier));
        assert_eq!(review.gate, None);

        let merge = &workflow.steps[2];
        assert_eq!(merge.skills, vec!["merge".to_string()]);
        assert_eq!(merge.gate, Some(StepGate::Merged));
    }

    #[test]
    fn the_shipped_develop_review_pr_document_parses() {
        let text = r#"workflow develop-review-pr
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  pr[Pull request]
    Push the branch, open the request and keep it until a human merges or closes it.
    skills: pr-babysit
    rank: balanced
    gate: request-merged
"#;
        let workflow = parse(text).unwrap();
        assert_eq!(workflow.name, "develop-review-pr");
        let pr = &workflow.steps[2];
        assert_eq!(pr.id, "pr");
        assert_eq!(pr.title, "Pull request");
        assert_eq!(pr.gate, Some(StepGate::RequestMerged));
        assert_eq!(pr.skills, vec!["pr-babysit".to_string()]);
    }

    #[test]
    fn a_document_with_no_workflow_line_is_refused_naming_its_line() {
        let err = parse("develop[Develop]\n  do it\n").unwrap_err();
        assert_eq!(err.line, 1);
    }

    #[test]
    fn a_duplicate_column_id_is_refused_naming_its_line() {
        let text = "workflow x\n  a[A]\n    do it\n  a[Also A]\n    do it\n";
        let err = parse(text).unwrap_err();
        assert_eq!(err.line, 4);
        assert!(
            err.message.contains("duplicate column id"),
            "{}",
            err.message
        );
    }

    #[test]
    fn a_repeated_key_is_refused_naming_its_line() {
        let text = "workflow x\n  a[A]\n    skills: coding\n    skills: debugging\n";
        let err = parse(text).unwrap_err();
        assert_eq!(err.line, 4);
        assert!(err.message.contains("repeats"), "{}", err.message);
    }

    #[test]
    fn an_unknown_rank_is_refused_naming_its_line() {
        let text = "workflow x\n  a[A]\n    rank: speedy\n";
        let err = parse(text).unwrap_err();
        assert_eq!(err.line, 3);
    }

    #[test]
    fn an_unknown_gate_is_refused_naming_its_line() {
        let text = "workflow x\n  a[A]\n    gate: landed\n";
        let err = parse(text).unwrap_err();
        assert_eq!(err.line, 3);
        assert!(err.message.contains("unknown gate"), "{}", err.message);
    }

    #[test]
    fn a_document_with_no_column_is_refused() {
        let err = parse("workflow x\n").unwrap_err();
        assert_eq!(err.line, 1);
        assert!(
            err.message.contains("at least one column"),
            "{}",
            err.message
        );
    }

    #[test]
    fn blank_lines_and_indentation_are_ignored() {
        let text = "\n\n  workflow x  \n\n  a[A]\n\n      do it\n\n";
        let workflow = parse(text).unwrap();
        assert_eq!(workflow.name, "x");
        assert_eq!(workflow.steps[0].description, "do it");
    }

    #[test]
    fn request_merged_is_spelled_with_a_hyphen_in_the_document_and_an_underscore_on_the_wire() {
        assert_eq!(StepGate::RequestMerged.as_str(), "request_merged");
        let text = "workflow x\n  a[A]\n    gate: request-merged\n";
        assert_eq!(
            parse(text).unwrap().steps[0].gate,
            Some(StepGate::RequestMerged)
        );
    }
}
