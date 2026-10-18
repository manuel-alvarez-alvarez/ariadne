//! Label matchers: PromQL's four operators (`=`, `!=`, `=~`, `!~`) over a
//! series' labels, plus its name under the reserved `__name__` label —
//! exactly as Prometheus treats it, so one matcher list narrows on name and
//! labels alike.

use serde_json::Value;

use crate::{Result, StoreError};

/// The reserved label a matcher names to narrow on a metric's name rather
/// than one of its labels.
pub const NAME_LABEL: &str = "__name__";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOp {
    Eq,
    Ne,
    /// Regular expression match (Rust `regex` syntax).
    Re,
    /// Regular expression non-match.
    NotRe,
}

#[derive(Debug, Clone)]
pub struct LabelMatcher {
    pub label: String,
    pub op: MatchOp,
    pub value: String,
}

impl LabelMatcher {
    pub fn new(label: impl Into<String>, op: MatchOp, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            op,
            value: value.into(),
        }
    }

    /// Whether a series of `name` and `labels` satisfies this matcher.
    pub(crate) fn matches(&self, name: &str, labels: &Value) -> Result<bool> {
        let actual: String = if self.label == NAME_LABEL {
            name.to_string()
        } else {
            labels
                .get(&self.label)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        Ok(match self.op {
            MatchOp::Eq => actual == self.value,
            MatchOp::Ne => actual != self.value,
            MatchOp::Re => regex(&self.value)?.is_match(&actual),
            MatchOp::NotRe => !regex(&self.value)?.is_match(&actual),
        })
    }
}

/// Whether every matcher in `matchers` holds for `name` and `labels` — an
/// empty list always holds, as an empty PromQL selector matches everything.
pub(crate) fn matches_all(matchers: &[LabelMatcher], name: &str, labels: &Value) -> Result<bool> {
    for matcher in matchers {
        if !matcher.matches(name, labels)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn regex(pattern: &str) -> Result<regex::Regex> {
    regex::Regex::new(pattern)
        .map_err(|e| StoreError::Invalid(format!("invalid label regex {pattern:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eq_and_ne_match_the_label_value() {
        let labels = json!({"service": "ariadned"});
        assert!(
            LabelMatcher::new("service", MatchOp::Eq, "ariadned")
                .matches("x", &labels)
                .unwrap()
        );
        assert!(
            !LabelMatcher::new("service", MatchOp::Eq, "other")
                .matches("x", &labels)
                .unwrap()
        );
        assert!(
            LabelMatcher::new("service", MatchOp::Ne, "other")
                .matches("x", &labels)
                .unwrap()
        );
    }

    #[test]
    fn a_missing_label_matches_only_the_empty_string() {
        let labels = json!({});
        assert!(
            LabelMatcher::new("missing", MatchOp::Eq, "")
                .matches("x", &labels)
                .unwrap()
        );
        assert!(
            !LabelMatcher::new("missing", MatchOp::Eq, "anything")
                .matches("x", &labels)
                .unwrap()
        );
    }

    #[test]
    fn the_name_label_matches_the_series_name_rather_than_a_label() {
        let labels = json!({"__name__": "decoy"});
        assert!(
            LabelMatcher::new(NAME_LABEL, MatchOp::Eq, "ariadne_telemetry_records")
                .matches("ariadne_telemetry_records", &labels)
                .unwrap()
        );
    }

    #[test]
    fn regex_operators_match_and_negate() {
        let labels = json!({"path": "/v1/tasks/42"});
        assert!(
            LabelMatcher::new("path", MatchOp::Re, "^/v1/tasks/.*")
                .matches("x", &labels)
                .unwrap()
        );
        assert!(
            LabelMatcher::new("path", MatchOp::NotRe, "^/v1/goals/.*")
                .matches("x", &labels)
                .unwrap()
        );
    }

    #[test]
    fn an_invalid_regex_is_reported_rather_than_panicking() {
        let labels = json!({});
        let error = LabelMatcher::new("x", MatchOp::Re, "(")
            .matches("x", &labels)
            .unwrap_err();
        assert!(matches!(error, StoreError::Invalid(_)));
    }

    #[test]
    fn an_empty_matcher_list_matches_everything() {
        assert!(matches_all(&[], "anything", &json!({})).unwrap());
    }
}
