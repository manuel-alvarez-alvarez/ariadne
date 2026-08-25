//! Curated model catalogs per agent CLI: what the CLI completes and the
//! daemon serves (`GET /v1/models`). OpenCode discovers its models at runtime
//! (`opencode models`), so its curated list is empty.

use crate::AgentKind;

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
