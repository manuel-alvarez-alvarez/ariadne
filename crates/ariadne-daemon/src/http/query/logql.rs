//! The LogQL subset behind `/loki/api/v1/*`: a stream selector over
//! `service`, `level`, `scope` and `target`; the line filters `|=`, `!=`,
//! `|~` and `!~`; label filters such as `| level="error"`; and
//! `count_over_time`, `rate` and `sum by` over a log selector. Anything
//! else is refused by name.

use std::collections::BTreeMap;

use regex::Regex;

use super::promql::Labels;

#[derive(Debug, PartialEq)]
pub(super) struct LogqlError(pub(super) String);

type Result<T> = std::result::Result<T, LogqlError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(LogqlError(message.into()))
}

/// The stages a full LogQL has and this subset does not.
const PARSER_STAGES: &[&str] = &[
    "json",
    "logfmt",
    "regexp",
    "pattern",
    "unpack",
    "line_format",
    "label_format",
    "drop",
    "keep",
    "decolorize",
    "unwrap",
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Re,
    NotRe,
}

/// `label op "value"`, in a selector or a label filter. Its regular
/// expression is anchored at both ends, as Loki's is; a line filter's is not.
#[derive(Debug, Clone)]
struct LabelMatcher {
    label: String,
    op: Op,
    value: String,
    regex: Option<Regex>,
}

impl LabelMatcher {
    fn matches(&self, labels: &Labels) -> bool {
        let actual = labels.get(&self.label).map(String::as_str).unwrap_or("");
        match self.op {
            Op::Eq => actual == self.value,
            Op::Ne => actual != self.value,
            Op::Re => self.regex.as_ref().is_some_and(|r| r.is_match(actual)),
            Op::NotRe => !self.regex.as_ref().is_some_and(|r| r.is_match(actual)),
        }
    }
}

#[derive(Debug, Clone)]
enum Stage {
    Line {
        op: Op,
        text: String,
        regex: Option<Regex>,
    },
    Label(LabelMatcher),
}

/// A stream selector and the filters after it.
#[derive(Debug, Clone)]
pub(super) struct LogQuery {
    selector: Vec<LabelMatcher>,
    stages: Vec<Stage>,
}

impl LogQuery {
    /// Whether a stream with `labels` passes the selector.
    pub(super) fn selects(&self, labels: &Labels) -> bool {
        self.selector.iter().all(|matcher| matcher.matches(labels))
    }

    /// Whether a line of a selected stream passes every filter.
    pub(super) fn keeps(&self, labels: &Labels, line: &str) -> bool {
        self.stages.iter().all(|stage| match stage {
            Stage::Line { op, text, regex } => match op {
                Op::Eq => line.contains(text.as_str()),
                Op::Ne => !line.contains(text.as_str()),
                Op::Re => regex.as_ref().is_some_and(|r| r.is_match(line)),
                Op::NotRe => !regex.as_ref().is_some_and(|r| r.is_match(line)),
            },
            Stage::Label(matcher) => matcher.matches(labels),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RangeFunction {
    CountOverTime,
    Rate,
}

#[derive(Debug, Clone)]
pub(super) struct MetricQuery {
    pub(super) function: RangeFunction,
    pub(super) log: LogQuery,
    pub(super) range_ms: i64,
    /// `Some` under `sum`: the `by` labels, empty for a plain `sum`.
    pub(super) sum_by: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub(super) enum Query {
    Log(LogQuery),
    Metric(MetricQuery),
}

pub(super) fn parse(text: &str) -> Result<Query> {
    if text.trim().is_empty() {
        return fail("missing query parameter");
    }
    let tokens = lex(text)?;
    let mut parser = Parser { tokens, at: 0 };
    let query = parser.query()?;
    match parser.peek() {
        None => Ok(query),
        Some(token) => fail(format!("unexpected {} in the query", token.describe())),
    }
}

/// One log line as the API presents it.
#[derive(Debug, Clone)]
pub(super) struct Line {
    pub(super) ns: i64,
    pub(super) labels: Labels,
    pub(super) line: String,
}

/// Evaluate a metric query at each step from `start` to `end`, both in
/// nanoseconds, over lines already narrowed by its selector and filters.
/// The points come back at their nanosecond times.
pub(super) fn evaluate(
    query: &MetricQuery,
    lines: &[Line],
    start: i64,
    end: i64,
    step: i64,
) -> Vec<(Labels, Vec<(i64, f64)>)> {
    let mut streams: BTreeMap<&Labels, Vec<i64>> = BTreeMap::new();
    for line in lines {
        streams.entry(&line.labels).or_default().push(line.ns);
    }
    for times in streams.values_mut() {
        times.sort_unstable();
    }
    let range_ns = query.range_ms * 1_000_000;
    let mut out: BTreeMap<Labels, Vec<(i64, f64)>> = BTreeMap::new();
    let mut t = start;
    while t <= end {
        let t_ns = t;
        let mut at: BTreeMap<Labels, f64> = BTreeMap::new();
        for (labels, times) in &streams {
            let count = (times.partition_point(|ns| *ns <= t_ns)
                - times.partition_point(|ns| *ns <= t_ns - range_ns))
                as f64;
            if count == 0.0 {
                continue;
            }
            let value = match query.function {
                RangeFunction::CountOverTime => count,
                RangeFunction::Rate => count / (query.range_ms as f64 / 1000.0),
            };
            let key = match &query.sum_by {
                None => (*labels).clone(),
                Some(by) => labels
                    .iter()
                    .filter(|(name, _)| by.contains(name))
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
            };
            *at.entry(key).or_default() += value;
        }
        for (labels, value) in at {
            out.entry(labels).or_default().push((t, value));
        }
        t += step;
    }
    out.into_iter().collect()
}

// ---- lexing ----

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Str(String),
    Range(String),
    Number(String),
    Punct(&'static str),
}

impl Token {
    fn describe(&self) -> String {
        match self {
            Token::Ident(name) => name.clone(),
            Token::Str(text) => format!("{text:?}"),
            Token::Range(range) => format!("[{range}]"),
            Token::Number(number) => number.clone(),
            Token::Punct(punct) => punct.to_string(),
        }
    }
}

fn lex(text: &str) -> Result<Vec<Token>> {
    const PUNCTS: &[&str] = &[
        "|=", "|~", "!=", "!~", "=~", "==", ">=", "<=", "{", "}", "(", ")", ",", "|", "=", ">", "<",
    ];
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '`' {
            let mut value = String::new();
            i += 1;
            loop {
                let Some(&next) = chars.get(i) else {
                    return fail("unterminated string in the query");
                };
                i += 1;
                if next == c {
                    break;
                }
                if next == '\\' && c == '"' {
                    let Some(&escaped) = chars.get(i) else {
                        return fail("unterminated string in the query");
                    };
                    i += 1;
                    value.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                } else {
                    value.push(next);
                }
            }
            tokens.push(Token::Str(value));
        } else if c == '[' {
            let close = chars[i..]
                .iter()
                .position(|c| *c == ']')
                .ok_or_else(|| LogqlError("unclosed range in the query".into()))?;
            tokens.push(Token::Range(chars[i + 1..i + close].iter().collect()));
            i += close + 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            tokens.push(Token::Number(chars[start..i].iter().collect()));
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let Some(punct) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
                return fail(format!("unexpected character {c:?} in the query"));
            };
            tokens.push(Token::Punct(punct));
            i += punct.len();
        }
    }
    Ok(tokens)
}

// ---- parsing ----

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    fn expect(&mut self, punct: &str) -> Result<()> {
        match self.next() {
            Some(Token::Punct(p)) if p == punct => Ok(()),
            Some(other) => fail(format!("expected {punct}, got {}", other.describe())),
            None => fail(format!("expected {punct} at the end of the query")),
        }
    }

    fn string(&mut self) -> Result<String> {
        match self.next() {
            Some(Token::Str(text)) => Ok(text),
            Some(other) => fail(format!("expected a string, got {}", other.describe())),
            None => fail("expected a string at the end of the query"),
        }
    }

    fn query(&mut self) -> Result<Query> {
        match self.peek() {
            Some(Token::Punct("{")) => Ok(Query::Log(self.log()?)),
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.at += 1;
                match name.as_str() {
                    "sum" => self.sum(),
                    "count_over_time" | "rate" => Ok(Query::Metric(self.range(&name)?)),
                    other => fail(format!("unsupported LogQL function {other}")),
                }
            }
            Some(other) => fail(format!("unexpected {} in the query", other.describe())),
            None => fail("missing query parameter"),
        }
    }

    fn grouping(&mut self) -> Result<Option<Vec<String>>> {
        match self.peek() {
            Some(Token::Ident(word)) if word == "by" => {
                self.at += 1;
                self.expect("(")?;
                let mut labels = Vec::new();
                loop {
                    match self.next() {
                        Some(Token::Ident(label)) => labels.push(label),
                        Some(Token::Punct(")")) if labels.is_empty() => return Ok(Some(labels)),
                        Some(other) => {
                            return fail(format!("expected a label, got {}", other.describe()));
                        }
                        None => return fail("unclosed by ( in the query"),
                    }
                    match self.next() {
                        Some(Token::Punct(",")) => continue,
                        Some(Token::Punct(")")) => return Ok(Some(labels)),
                        _ => return fail("expected , or ) in by ( )"),
                    }
                }
            }
            Some(Token::Ident(word)) if word == "without" => {
                fail("unsupported LogQL grouping without")
            }
            _ => Ok(None),
        }
    }

    fn sum(&mut self) -> Result<Query> {
        let before = self.grouping()?;
        self.expect("(")?;
        let mut inner = match self.next() {
            Some(Token::Ident(name)) if name == "count_over_time" || name == "rate" => {
                self.range(&name)?
            }
            Some(Token::Ident(name)) => {
                return fail(format!("unsupported LogQL function {name}"));
            }
            _ => return fail("sum takes count_over_time or rate"),
        };
        self.expect(")")?;
        let after = self.grouping()?;
        inner.sum_by = Some(before.or(after).unwrap_or_default());
        Ok(Query::Metric(inner))
    }

    fn range(&mut self, name: &str) -> Result<MetricQuery> {
        self.expect("(")?;
        let log = self.log()?;
        let range = match self.next() {
            Some(Token::Range(range)) => range,
            _ => return fail(format!("{name} takes a log selector and a [range]")),
        };
        let range_ms = super::duration_ms(&range)
            .filter(|ms| *ms > 0)
            .ok_or_else(|| LogqlError(format!("invalid range [{range}]")))?;
        self.expect(")")?;
        Ok(MetricQuery {
            function: if name == "rate" {
                RangeFunction::Rate
            } else {
                RangeFunction::CountOverTime
            },
            log,
            range_ms,
            sum_by: None,
        })
    }

    fn matcher(&mut self, label: String) -> Result<LabelMatcher> {
        let op = match self.next() {
            Some(Token::Punct("=" | "==")) => Op::Eq,
            Some(Token::Punct("!=")) => Op::Ne,
            Some(Token::Punct("=~")) => Op::Re,
            Some(Token::Punct("!~")) => Op::NotRe,
            Some(Token::Punct(other)) => {
                return fail(format!("unsupported LogQL label operator {other}"));
            }
            _ => return fail(format!("expected an operator after {label}")),
        };
        let value = match self.next() {
            Some(Token::Str(value)) => value,
            Some(other) => {
                return fail(format!(
                    "unsupported LogQL label filter {label} {}",
                    other.describe()
                ));
            }
            None => return fail("expected a string at the end of the query"),
        };
        Ok(LabelMatcher {
            regex: regex_for(op, &format!("^(?:{value})$"))?,
            label,
            op,
            value,
        })
    }

    fn log(&mut self) -> Result<LogQuery> {
        self.expect("{")?;
        let mut selector = Vec::new();
        loop {
            match self.next() {
                Some(Token::Ident(label)) => selector.push(self.matcher(label)?),
                Some(Token::Punct("}")) if selector.is_empty() => break,
                _ => return fail("expected a label matcher in the stream selector"),
            }
            match self.next() {
                Some(Token::Punct(",")) => continue,
                Some(Token::Punct("}")) => break,
                _ => return fail("expected , or } in the stream selector"),
            }
        }
        let mut stages = Vec::new();
        loop {
            let op = match self.peek() {
                Some(Token::Punct("|=")) => Op::Eq,
                Some(Token::Punct("!=")) => Op::Ne,
                Some(Token::Punct("|~")) => Op::Re,
                Some(Token::Punct("!~")) => Op::NotRe,
                Some(Token::Punct("|")) => {
                    self.at += 1;
                    match self.next() {
                        Some(Token::Ident(name)) if PARSER_STAGES.contains(&name.as_str()) => {
                            return fail(format!("unsupported LogQL stage {name}"));
                        }
                        Some(Token::Ident(label)) => {
                            stages.push(Stage::Label(self.matcher(label)?));
                            continue;
                        }
                        Some(other) => {
                            return fail(format!("unsupported LogQL stage {}", other.describe()));
                        }
                        None => return fail("expected a stage after |"),
                    }
                }
                _ => break,
            };
            self.at += 1;
            let text = self.string()?;
            stages.push(Stage::Line {
                regex: regex_for(op, &text)?,
                op,
                text,
            });
        }
        Ok(LogQuery { selector, stages })
    }
}

fn regex_for(op: Op, pattern: &str) -> Result<Option<Regex>> {
    if !matches!(op, Op::Re | Op::NotRe) {
        return Ok(None);
    }
    Regex::new(pattern)
        .map(Some)
        .map_err(|error| LogqlError(format!("invalid regular expression {pattern:?}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> Labels {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn log(text: &str) -> LogQuery {
        match parse(text).unwrap() {
            Query::Log(log) => log,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn selectors_take_the_four_matchers() {
        let api = labels(&[("service", "ariadned"), ("level", "error")]);
        assert!(log(r#"{service="ariadned"}"#).selects(&api));
        assert!(!log(r#"{service!="ariadned"}"#).selects(&api));
        assert!(log(r#"{service=~"aria.*", level!~"info|debug"}"#).selects(&api));
        assert!(
            !log(r#"{service=~"aria"}"#).selects(&api),
            "regex is anchored"
        );
    }

    #[test]
    fn line_and_label_filters_narrow_the_lines() {
        let error = labels(&[("level", "error")]);
        let query = log(r#"{service="x"} |= "failed" != "retry" |~ "task [0-9]+" | level="error""#);
        assert!(query.keeps(&error, "failed task 42"));
        assert!(!query.keeps(&error, "failed task 42, retry"));
        assert!(!query.keeps(&error, "failed task x"));
        assert!(!query.keeps(&labels(&[("level", "info")]), "failed task 42"));
        assert!(!log(r#"{service="x"} !~ "fail""#).keeps(&error, "failed"));
    }

    #[test]
    fn metric_queries_count_and_rate_per_stream_or_sum() {
        let line = |ns: i64, service: &str| Line {
            ns,
            labels: labels(&[("service", service), ("level", "info")]),
            line: String::new(),
        };
        let lines = [
            line(1_000_000_000, "a"),
            line(2_000_000_000, "a"),
            line(2_500_000_000, "b"),
        ];
        let Query::Metric(count) = parse(r#"count_over_time({level="info"}[2s])"#).unwrap() else {
            panic!()
        };
        let s = 1_000_000_000;
        let streams = evaluate(&count, &lines, 2 * s, 4 * s, s);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].1, vec![(2 * s, 2.0), (3 * s, 1.0)]);
        let Query::Metric(sum) = parse(r#"sum by (level) (rate({level="info"}[2s]))"#).unwrap()
        else {
            panic!()
        };
        let summed = evaluate(&sum, &lines, 3 * s, 3 * s, s);
        assert_eq!(
            summed,
            vec![(labels(&[("level", "info")]), vec![(3 * s, 1.0)])]
        );
        let Query::Metric(after) =
            parse(r#"sum(count_over_time({level="info"}[2s])) by (service)"#).unwrap()
        else {
            panic!()
        };
        assert_eq!(after.sum_by, Some(vec!["service".to_string()]));
    }

    #[test]
    fn anything_else_is_refused_by_name() {
        for (query, named) in [
            (r#"{service="x"} | json"#, "json"),
            (r#"avg_over_time({service="x"}[1m])"#, "avg_over_time"),
            (
                r#"sum without (level) (rate({service="x"}[1m]))"#,
                "without",
            ),
            (r#"{service="x"} | latency > 10"#, ">"),
            (r#"topk(1, rate({service="x"}[1m]))"#, "topk"),
        ] {
            let error = parse(query).unwrap_err();
            assert!(error.0.contains(named), "{query}: {error:?}");
        }
    }
}
