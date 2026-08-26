//! What a model is and what running one costs: the curated catalogs per agent
//! CLI — what the CLI completes and the daemon serves (`GET /v1/models`);
//! OpenCode discovers its models at runtime (`opencode models`), so its
//! curated list is empty — and [`TokenUsage`], the tokens one conversation
//! with a model spent.

use std::iter::Sum;
use std::ops::{Add, AddAssign};

use crate::AgentKind;

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
