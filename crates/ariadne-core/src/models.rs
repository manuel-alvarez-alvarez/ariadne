//! What a model is and what running one costs: [`ModelRef`], the one string a
//! model is chosen by; the curated catalogs per agent CLI — what the CLI
//! completes and the daemon serves (`GET /v1/models`); OpenCode discovers its
//! models at runtime (`opencode models`), so its curated list is empty — and
//! [`TokenUsage`], the tokens one conversation with a model spent.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign};
use std::str::FromStr;

use crate::AgentKind;

/// What an agent runs on, as the single string that names it:
/// `<agent_kind>[:<model>]` — the agent CLI, and after a `:` one model of it.
///
/// The agent half is structure and the model half is free text the CLI is
/// handed as typed, so what splits the two is the *first* colon and never a
/// later one: `opencode:ollama/llama3:8b` is that opencode id whole, tag and
/// all. A string with no colon names an agent CLI on its own default model
/// (`codex`), and a model that names no CLI has no spelling here at all —
/// nothing derives one from the other, and a refusal says so by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRef {
    /// The agent CLI, which is the choice.
    pub agent_kind: AgentKind,
    /// The model it runs; None = that CLI's own default.
    pub model: Option<String>,
}

impl ModelRef {
    /// The agent CLI on its own default model.
    pub fn of(agent_kind: AgentKind) -> Self {
        Self {
            agent_kind,
            model: None,
        }
    }
}

impl fmt::Display for ModelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.agent_kind.as_str())?;
        match &self.model {
            Some(model) => write!(f, ":{model}"),
            None => Ok(()),
        }
    }
}

impl FromStr for ModelRef {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((agent, model)) = s.split_once(':') else {
            return match agent_kind(s) {
                Some(agent_kind) => Ok(Self::of(agent_kind)),
                None => Err(format!(
                    "`{s}` names no agent CLI — write the CLI it runs on first, \
                     as in `{}:{s}` (agents: {})",
                    AgentKind::ClaudeCode.as_str(),
                    kinds()
                )),
            };
        };
        let Some(agent_kind) = agent_kind(agent) else {
            return Err(format!(
                "unknown agent `{agent}` in `{s}` — what stands before the `:` \
                 is one of {}",
                kinds()
            ));
        };
        if model.is_empty() {
            return Err(format!(
                "no model after the `:` in `{s}` — write `{agent}` on its own to \
                 run that CLI on its own default model"
            ));
        }
        Ok(Self {
            agent_kind,
            model: Some(model.to_string()),
        })
    }
}

/// One agent CLI in either of its spellings: the wire one (`claude_code`) and
/// the hyphenated one a person types (`claude-code`) name the same CLI, and
/// only the wire one is ever printed back.
fn agent_kind(raw: &str) -> Option<AgentKind> {
    raw.replace('-', "_").parse().ok()
}

/// The agent CLIs a refusal lists, in the order everything lists them.
fn kinds() -> String {
    AgentKind::ALL
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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

/// One curated model: its id and a one-line capability description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: &'static str,
    pub description: &'static str,
}

/// The curated models of an agent CLI.
pub fn curated_models(kind: AgentKind) -> &'static [ModelInfo] {
    match kind {
        AgentKind::ClaudeCode => CLAUDE_CODE,
        AgentKind::Codex => CODEX,
        AgentKind::Opencode => &[],
    }
}

const CLAUDE_CODE: &[ModelInfo] = &[
    ModelInfo {
        id: "claude-fable-5",
        description: "Frontier: highest capability; intricate multi-step agentic workflows and full SDLC loops",
    },
    ModelInfo {
        id: "claude-mythos-5",
        description: "Frontier: specialized high-end reasoning within secure configurations",
    },
    ModelInfo {
        id: "claude-opus-5",
        description: "Opus tier: massive contextual analysis, legal boilerplate logic, deep math/science",
    },
    ModelInfo {
        id: "claude-opus-4-7",
        description: "Opus tier: pinned version for multi-file architecture refactoring and complex bugs",
    },
    ModelInfo {
        id: "claude-sonnet-4-8",
        description: "Sonnet tier: production sweet spot; high speed with near-Opus engineering capability",
    },
    ModelInfo {
        id: "claude-sonnet-4-6",
        description: "Sonnet tier: legacy production staple; quick diagnostics, lower task latency",
    },
    ModelInfo {
        id: "claude-haiku-4-5",
        description: "Haiku tier: ultra-fast; inline completions, text touch-ups, shell command generation",
    },
];

const CODEX: &[ModelInfo] = &[
    ModelInfo {
        id: "gpt-5.6-sol",
        description: "Frontier reasoning: multi-step agentic loops, codebase-wide architecture planning",
    },
    ModelInfo {
        id: "gpt-5.5-sol",
        description: "Frontier reasoning: deep logic reasoning, long-horizon bug resolution",
    },
    ModelInfo {
        id: "gpt-5.3-codex",
        description: "Codex developer: default enterprise LTS; optimal for active engineering contexts",
    },
    ModelInfo {
        id: "gpt-5.2-codex",
        description: "Codex developer: native context compaction; single/multi-file refactoring",
    },
    ModelInfo {
        id: "codex-mini-latest",
        description: "Low latency: ultra-fast inline edits, quick explanation cards, shell tasks",
    },
    ModelInfo {
        id: "o4-mini",
        description: "Diagnostic: low-latency diagnostic tracking and fast debugging runs",
    },
    ModelInfo {
        id: "o3",
        description: "Diagnostic: classic reasoning checkups and quick inline completion blocks",
    },
    ModelInfo {
        id: "gpt-5.6-terra",
        description: "Balanced: high-volume execution layer",
    },
    ModelInfo {
        id: "gpt-5.6-luna",
        description: "Balanced: cost-effective subagent processing queues",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The three forms a model is written in, printed back the way the daemon
    /// spells them: an agent CLI on its own, an agent and a model, and an
    /// opencode id whose own colon is data rather than structure.
    #[test]
    fn a_model_is_the_agent_cli_and_then_the_model() {
        for (text, agent_kind, model) in [
            ("codex", AgentKind::Codex, None),
            ("codex:o3", AgentKind::Codex, Some("o3")),
            (
                "claude_code:claude-opus-5",
                AgentKind::ClaudeCode,
                Some("claude-opus-5"),
            ),
            (
                "opencode:ollama/llama3:8b",
                AgentKind::Opencode,
                Some("ollama/llama3:8b"),
            ),
        ] {
            let parsed: ModelRef = text.parse().expect(text);
            assert_eq!(parsed.agent_kind, agent_kind, "{text}");
            assert_eq!(parsed.model.as_deref(), model, "{text}");
            assert_eq!(parsed.to_string(), text, "and printed back as it was read");
        }
    }

    /// The hyphenated spelling names the same CLI — people type it — and the
    /// wire one is what comes back out.
    #[test]
    fn the_hyphenated_agent_spelling_reads_as_the_wire_one() {
        let parsed: ModelRef = "claude-code:claude-opus-5".parse().expect("a spelling");
        assert_eq!(parsed.agent_kind, AgentKind::ClaudeCode);
        assert_eq!(parsed.to_string(), "claude_code:claude-opus-5");
        assert_eq!(
            "claude-code".parse::<ModelRef>().expect("a CLI"),
            ModelRef::of(AgentKind::ClaudeCode)
        );
    }

    /// A model that names no CLI is refused by name, and the refusal writes
    /// out the form that would have worked with the model that was typed.
    #[test]
    fn a_model_naming_no_agent_is_refused_with_the_form_it_wanted() {
        let err = "claude-opus-5".parse::<ModelRef>().expect_err("no agent");
        assert!(err.contains("`claude-opus-5` names no agent CLI"), "{err}");
        assert!(err.contains("`claude_code:claude-opus-5`"), "{err}");
        assert!(err.contains("claude_code, codex, opencode"), "{err}");
    }

    /// An agent prefix that is no CLI is refused with the three that are.
    #[test]
    fn an_unknown_agent_is_refused_with_the_ones_there_are() {
        let err = "llama:foo".parse::<ModelRef>().expect_err("no such CLI");
        assert!(
            err.contains("unknown agent `llama` in `llama:foo`"),
            "{err}"
        );
        assert!(err.contains("claude_code, codex, opencode"), "{err}");
    }

    /// A colon with nothing after it is a model somebody meant to write, not
    /// a way to say "that CLI's default" — which is the agent on its own.
    #[test]
    fn a_colon_with_no_model_after_it_is_refused() {
        let err = "codex:".parse::<ModelRef>().expect_err("no model");
        assert!(err.contains("no model after the `:` in `codex:`"), "{err}");
        assert!(err.contains("write `codex` on its own"), "{err}");
    }

    #[test]
    fn curated_per_kind() {
        assert!(!curated_models(AgentKind::ClaudeCode).is_empty());
        assert!(!curated_models(AgentKind::Codex).is_empty());
        assert!(curated_models(AgentKind::Opencode).is_empty());
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

    #[test]
    fn ids_are_unique_within_a_kind() {
        for kind in AgentKind::ALL {
            let ids: Vec<_> = curated_models(kind).iter().map(|m| m.id).collect();
            let mut deduped = ids.clone();
            deduped.sort_unstable();
            deduped.dedup();
            assert_eq!(ids.len(), deduped.len(), "{kind:?}");
        }
    }
}
