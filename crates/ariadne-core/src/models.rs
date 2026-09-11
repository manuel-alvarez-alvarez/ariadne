//! What a model is and what running one costs: [`ModelRef`], the one string a
//! model is chosen by; [`effort_error`], the check every writer of an effort
//! makes; and [`TokenUsage`], the tokens one conversation with a model spent.
//!
//! Nothing here lists models. Every agent is an ACP agent in the daemon's
//! registry, and what it can run — its models and the efforts each takes — is
//! what discovery asked it for at runtime.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign};
use std::str::FromStr;

/// What an agent runs on, as the single string that names it:
/// `<agent>:<model>` — the id of an agent in the ACP registry, and after the
/// `:` one model of it.
///
/// The agent half is structure and the model half is free text the agent is
/// handed as typed, so what splits the two is the *first* colon and never a
/// later one: `opencode-acp:ollama/llama3:8b` is that model whole, tag and
/// all. Both halves are required: an agent on its own is refused, because a
/// model is required and no agent default stands in for one. Whether the
/// agent half names an agent the registry holds is the daemon's to answer —
/// this is only the spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The registry id of the agent, which runs the model.
    pub agent: String,
    /// The model it runs.
    pub model: String,
}

impl fmt::Display for ModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.agent, self.model)
    }
}

impl FromStr for ModelRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((agent, model)) = s.split_once(':') else {
            return Err(format!(
                "`{s}` names no agent — a model is required and it is written \
                 `<agent>:<model>`, the agent an id of the ACP registry, as in \
                 `<agent>:{s}` or `{s}:<model>`"
            ));
        };
        if agent.trim().is_empty() {
            return Err(format!(
                "no agent before the `:` in `{s}` — what stands before the `:` \
                 is the id of an agent in the ACP registry"
            ));
        }
        // Whitespace alone is an empty model too: it would create a pin and
        // launch a model no agent has.
        if model.trim().is_empty() {
            return Err(format!(
                "no model after the `:` in `{s}` — a model is required, so write \
                 one of that agent's after the `:`"
            ));
        }
        Ok(Self {
            agent: agent.to_string(),
            model: model.to_string(),
        })
    }
}

/// The tokens one agent conversation has spent, as its own transcript
/// reports them.
///
/// Cumulative and total, never a delta: `input_tokens` counts every prompt
/// token the conversation was billed for, cache reads and cache writes
/// included, and `cached_input_tokens` is the subset of it served from the
/// prompt cache — so the two are never added together. `output_tokens`
/// counts completion tokens, thinking and reasoning included.
///
/// Addition is how a session, a task and a goal are totalled from the
/// transcripts under them, and it saturates: a counter no arithmetic here can
/// overflow in practice must not be the thing that panics a daemon if one
/// ever does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Prompt tokens, cache reads and cache writes included.
    pub input_tokens: u64,
    /// The subset of `input_tokens` served from the prompt cache.
    pub cached_input_tokens: u64,
    /// Completion tokens, thinking and reasoning included.
    pub output_tokens: u64,
}

impl TokenUsage {
    /// Whether nothing has been reported: the zero every read answers with
    /// where no transcript said anything.
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

impl Add for TokenUsage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(rhs.input_tokens),
            cached_input_tokens: self
                .cached_input_tokens
                .saturating_add(rhs.cached_input_tokens),
            output_tokens: self.output_tokens.saturating_add(rhs.output_tokens),
        }
    }
}

impl AddAssign for TokenUsage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sum for TokenUsage {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), Add::add)
    }
}

impl<'a> Sum<&'a TokenUsage> for TokenUsage {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.copied().sum()
    }
}

/// The registry agent a pinned model names: what stands before its first
/// colon, or the whole string where it has none.
pub fn agent_of(model: &str) -> &str {
    model.split_once(':').map_or(model, |(agent, _)| agent)
}

/// Why an effort cannot be run on a model, or `None` when it can — the one
/// check every writer of an effort makes before storing one.
///
/// `model_efforts` is the model's own efforts where the catalog knows it
/// (`GET /v1/models`, as discovery found them), and `None` for a model id
/// nothing has listed: an effort is an agent's own configuration value, so
/// one nothing has listed takes any name that is not blank.
pub fn effort_error(model_efforts: Option<&[String]>, effort: &str) -> Option<String> {
    let Some(efforts) = model_efforts else {
        return effort.trim().is_empty().then(|| {
            "no effort was named — a model nothing has listed takes whichever \
             effort its agent was configured with"
                .to_string()
        });
    };
    if efforts.iter().any(|known| known == effort) {
        return None;
    }
    Some(match efforts.is_empty() {
        true => format!("`{effort}` is no effort — that model takes none at all"),
        false => format!(
            "`{effort}` is no effort of that model — it takes {}",
            efforts.join(", ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model is an agent and a model, printed back the way it was read —
    /// and a model id whose own colon is data rather than structure keeps it.
    #[test]
    fn a_model_is_the_agent_and_then_the_model() {
        for (text, agent, model) in [
            ("codex-acp:o3", "codex-acp", "o3"),
            (
                "claude-code-acp:claude-opus-5",
                "claude-code-acp",
                "claude-opus-5",
            ),
            (
                "opencode-acp:ollama/llama3:8b",
                "opencode-acp",
                "ollama/llama3:8b",
            ),
        ] {
            let parsed: ModelRef = text.parse().expect(text);
            assert_eq!(parsed.agent, agent, "{text}");
            assert_eq!(parsed.model, model, "{text}");
            assert_eq!(parsed.to_string(), text, "and printed back as it was read");
        }
    }

    /// A string with no colon names no agent, and the refusal writes out the
    /// form that would have worked.
    #[test]
    fn a_model_naming_no_agent_is_refused_with_the_form_it_wanted() {
        let err = "claude-opus-5".parse::<ModelRef>().expect_err("no agent");
        assert!(err.contains("`claude-opus-5` names no agent"), "{err}");
        assert!(err.contains("`<agent>:claude-opus-5`"), "{err}");
    }

    /// A colon with nothing before it names no agent.
    #[test]
    fn a_colon_with_no_agent_before_it_is_refused() {
        let err = ":o3".parse::<ModelRef>().expect_err("no agent");
        assert!(err.contains("no agent before the `:` in `:o3`"), "{err}");
    }

    /// A colon with nothing after it is a model somebody meant to write, and
    /// there is no default to fall back to. Whitespace alone is nothing too.
    #[test]
    fn a_colon_with_no_model_after_it_is_refused() {
        for text in ["codex-acp:", "codex-acp: ", "codex-acp:   "] {
            let err = text.parse::<ModelRef>().expect_err(text);
            assert!(
                err.contains(&format!("no model after the `:` in `{text}`")),
                "{text}: {err}"
            );
            assert!(err.contains("a model is required"), "{text}: {err}");
        }
    }

    /// A model the catalog knows takes exactly its own efforts, and a refusal
    /// names the effort that was asked for and lists the ones there are.
    #[test]
    fn a_known_model_takes_its_own_efforts_and_nothing_else() {
        let efforts = strings(&["low", "medium", "high"]);
        assert_eq!(effort_error(Some(&efforts), "medium"), None);
        let err = effort_error(Some(&efforts), "ultra").expect("no such effort");
        assert!(err.contains("`ultra` is no effort of that model"), "{err}");
        assert!(err.contains("low, medium, high"), "{err}");
    }

    /// A model with no effort control at all refuses every effort, and says
    /// that is what it is rather than listing nothing.
    #[test]
    fn a_model_with_no_effort_control_takes_none() {
        let err = effort_error(Some(&[]), "high").expect("none at all");
        assert!(err.contains("`high` is no effort"), "{err}");
        assert!(err.contains("takes none at all"), "{err}");
    }

    /// A model nothing has listed takes any effort name that is not blank.
    #[test]
    fn an_unlisted_model_takes_any_effort_but_a_blank_one() {
        assert_eq!(effort_error(None, "whatever"), None);
        let err = effort_error(None, " ").expect("blank");
        assert!(err.contains("no effort was named"), "{err}");
    }

    fn strings(efforts: &[&str]) -> Vec<String> {
        efforts.iter().map(|e| e.to_string()).collect()
    }

    /// Totals add counter by counter, and an empty run of them is the zero
    /// every unreported session reads as.
    #[test]
    fn usage_adds_up_and_starts_at_zero() {
        let one = TokenUsage {
            input_tokens: 100,
            cached_input_tokens: 80,
            output_tokens: 10,
        };
        let two = TokenUsage {
            input_tokens: 5,
            cached_input_tokens: 1,
            output_tokens: 2,
        };
        assert_eq!(
            one + two,
            TokenUsage {
                input_tokens: 105,
                cached_input_tokens: 81,
                output_tokens: 12,
            }
        );
        assert_eq!([one, two].into_iter().sum::<TokenUsage>(), one + two);
        assert!(
            std::iter::empty::<TokenUsage>()
                .sum::<TokenUsage>()
                .is_zero()
        );
    }

    /// Nothing here panics a daemon: a counter at the top of its range
    /// saturates rather than overflowing.
    #[test]
    fn usage_saturates_rather_than_overflowing() {
        let full = TokenUsage {
            input_tokens: u64::MAX,
            cached_input_tokens: u64::MAX,
            output_tokens: u64::MAX,
        };
        assert_eq!(full + full, full);
    }
}
