//! What a model is and what running one costs: [`ModelRef`], the one string a
//! model is chosen by; the curated catalogs per agent CLI — what the CLI
//! completes and the daemon serves (`GET /v1/models`), each model with what
//! it is for ([`ModelProfile`]) and the reasoning efforts it accepts; the
//! [`effort_ladder`] of each CLI, which says what spending an effort buys;
//! and [`TokenUsage`], the tokens one conversation with a model spent.
//!
//! OpenCode discovers its models and their efforts at runtime (`opencode
//! models --verbose`), so it has no curated list of its own — only
//! [`opencode_profile`], what is known about the models it fronts, laid over
//! whatever discovery found.

use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign};
use std::str::FromStr;

use crate::{AgentKind, ModelTier};

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

/// One curated model: its id, what it is, and the reasoning efforts its agent
/// CLI accepts for it.
///
/// `efforts` runs cheapest → deepest and is empty for a model with no effort
/// control at all. `default_effort` is what the CLI itself runs the model at
/// when nothing is passed — informational, and `None` where there is nothing
/// to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: &'static str,
    /// The one line a picker shows beside the id.
    pub description: &'static str,
    /// The capability class it belongs to.
    pub tier: ModelTier,
    /// What it costs to run, 1 (cheapest) to 5, ranked across the whole
    /// catalog — see [`ModelProfile`].
    pub cost: Option<u8>,
    /// How fast it turns work around, 1 (slowest) to 5, at the effort its CLI
    /// runs it at.
    pub speed: Option<u8>,
    /// Task shapes it is the right choice for.
    pub best_for: &'static [&'static str],
    /// Task shapes it is the wrong choice for.
    pub avoid_for: &'static [&'static str],
    pub efforts: &'static [&'static str],
    pub default_effort: Option<&'static str>,
}

impl ModelInfo {
    /// Everything this entry says about the model itself, apart from its id
    /// and the efforts its CLI runs it at.
    pub fn profile(&self) -> ModelProfile {
        ModelProfile {
            description: self.description,
            tier: self.tier,
            cost: self.cost,
            speed: self.speed,
            best_for: self.best_for,
            avoid_for: self.avoid_for,
        }
    }
}

/// What is known about a model as a model — the half of a [`ModelInfo`] that
/// does not depend on which CLI is running it, and so can be borrowed by an
/// entry that reaches the catalog another way.
///
/// `cost` and `speed` are bands from 1 to 5 and they are *rankings*, not
/// measurements: one ladder across every agent CLI, so that a claude_code
/// entry and a codex entry on the same rung really do cost, or answer, about
/// the same. Prices go stale and would have to be chased; a ranking only
/// moves when the catalog itself does. `cost` 1 is free — nothing that is
/// paid for sits there — and 5 is the frontier; `speed` 5 is near-instant and
/// 1 is a model that thinks for minutes. `None` where nothing has been
/// established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelProfile {
    pub description: &'static str,
    pub tier: ModelTier,
    pub cost: Option<u8>,
    pub speed: Option<u8>,
    pub best_for: &'static [&'static str],
    pub avoid_for: &'static [&'static str],
}

/// One reasoning effort of one agent CLI: the name it is passed by, and what
/// spending it buys.
///
/// An effort is the CLI's, not the model's — `high` means the same thing on
/// every model of a CLI that takes it — so the ladder is written once per CLI
/// and every model that accepts a rung borrows its description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffortInfo {
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

/// Every effort an agent CLI accepts, for a model the catalog does not know:
/// the union of what its models take, which is as much as can be said about a
/// model id somebody typed by hand.
///
/// Empty for opencode, whose efforts are its models' own variant names,
/// discovered per model (`opencode models --verbose`) rather than fixed by
/// the CLI — so an undiscovered opencode model takes any variant name.
pub fn known_efforts(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::ClaudeCode => CLAUDE_CODE_EFFORTS,
        AgentKind::Codex => CODEX_EFFORTS,
        AgentKind::Opencode => &[],
    }
}

/// An agent CLI's whole effort ladder, cheapest → deepest, each rung with
/// what spending it buys.
///
/// The same rungs as [`known_efforts`] and in the same order; empty for
/// opencode, whose efforts are per model and discovered rather than fixed.
pub fn effort_ladder(kind: AgentKind) -> &'static [EffortInfo] {
    match kind {
        AgentKind::ClaudeCode => CLAUDE_CODE_LADDER,
        AgentKind::Codex => CODEX_LADDER,
        AgentKind::Opencode => &[],
    }
}

/// What one effort of one agent CLI buys, or `None` for a rung that CLI does
/// not have — the lookup an entry's efforts are described from.
pub fn effort_description(kind: AgentKind, effort: &str) -> Option<&'static str> {
    effort_ladder(kind)
        .iter()
        .find(|e| e.id == effort)
        .map(|e| e.description)
}

/// What is known about a model opencode discovered, keyed by the
/// `provider/model` id opencode prints it under, or `None` for one nothing
/// has been written about — which is then described from what discovery
/// itself printed.
///
/// An overlay and never a catalog: whether an opencode model can be run at
/// all is opencode's own configuration to answer, so nothing here is offered
/// unless discovery listed it first. A provider that fronts a model this
/// catalog already curates (`anthropic/…`, `openai/…`) borrows that entry's
/// profile rather than repeating it, so the two never drift apart.
pub fn opencode_profile(id: &str) -> Option<ModelProfile> {
    let sibling = |kind: AgentKind, model: &str| {
        curated_models(kind)
            .iter()
            .find(|m| m.id == model)
            .map(ModelInfo::profile)
    };
    if let Some(model) = id.strip_prefix("anthropic/") {
        return sibling(AgentKind::ClaudeCode, model);
    }
    if let Some(model) = id.strip_prefix("openai/") {
        return sibling(AgentKind::Codex, model);
    }
    OPENCODE
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, profile)| *profile)
}

/// Why an effort cannot be run on a model, or `None` when it can — the one
/// check every writer of an effort makes before storing one.
///
/// `model_efforts` is the model's own efforts where the catalog knows it
/// (`GET /v1/models`, curated or discovered), and `None` for a model id
/// nothing has listed: then claude_code and codex fall back to
/// [`known_efforts`], and opencode — whose efforts are per model — takes any
/// name that is not blank.
pub fn effort_error(
    kind: AgentKind,
    model_efforts: Option<&[String]>,
    effort: &str,
) -> Option<String> {
    if let Some(efforts) = model_efforts {
        if efforts.iter().any(|known| known == effort) {
            return None;
        }
        return Some(match efforts.is_empty() {
            true => format!("`{effort}` is no effort — that model takes none at all"),
            false => format!(
                "`{effort}` is no effort of that model — it takes {}",
                listed(efforts.iter().map(String::as_str))
            ),
        });
    }
    match known_efforts(kind) {
        [] => effort.trim().is_empty().then(|| {
            format!(
                "no effort was named — an {} model takes whichever variants it \
                 was configured with",
                kind.as_str()
            )
        }),
        known => (!known.contains(&effort)).then(|| {
            format!(
                "`{effort}` is no effort of {} — it takes {}",
                kind.as_str(),
                listed(known.iter().copied())
            )
        }),
    }
}

/// The efforts a refusal lists, in the order they are accepted.
fn listed<'a>(efforts: impl Iterator<Item = &'a str>) -> String {
    efforts.collect::<Vec<_>>().join(", ")
}

/// `claude --effort`, every level of it.
const CLAUDE_CODE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// The five levels most Claude models take.
const CLAUDE_FIVE: &[&str] = CLAUDE_CODE_EFFORTS;

/// The four the 4-6 generation takes: no `xhigh`.
const CLAUDE_FOUR: &[&str] = &["low", "medium", "high", "max"];

/// `-c model_reasoning_effort=…`, every level Codex accepts.
const CODEX_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];

/// The six levels the reasoning-heavy Codex models take.
const CODEX_SIX: &[&str] = &["low", "medium", "high", "xhigh", "max", "ultra"];

/// The five of the fast one: no `ultra`.
const CODEX_FIVE: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// What each `claude --effort` level buys, from the Claude Code model
/// configuration docs.
const CLAUDE_CODE_LADDER: &[EffortInfo] = &[
    EffortInfo {
        id: "low",
        description: "The least thinking and the quickest answer: short, scoped work that is \
                      latency-sensitive rather than intelligence-sensitive",
    },
    EffortInfo {
        id: "medium",
        description: "Fewer thinking tokens, for cost-sensitive work that can trade away some \
                      depth to get them back",
    },
    EffortInfo {
        id: "high",
        description: "The balance of tokens against depth, and what every model but Opus 4.7 \
                      runs at when nothing is passed",
    },
    EffortInfo {
        id: "xhigh",
        description: "Deeper reasoning at a higher token spend, for work that has earned it; \
                      Opus 4.7 runs here by default",
    },
    EffortInfo {
        id: "max",
        description: "The most thinking there is, for demanding problems — with diminishing \
                      returns, and prone to overthinking a task that did not need it",
    },
];

/// What each `model_reasoning_effort` level buys. The wording follows what
/// Codex itself publishes for each level (`~/.codex/models_cache.json`,
/// `supported_reasoning_levels`); `minimal` is the one rung Codex accepts
/// without describing, so this says only what the name carries.
const CODEX_LADDER: &[EffortInfo] = &[
    EffortInfo {
        id: "minimal",
        description: "Less reasoning than `low` — the one rung Codex publishes nothing about",
    },
    EffortInfo {
        id: "low",
        description: "Fast responses with lighter reasoning: small, well-specified changes",
    },
    EffortInfo {
        id: "medium",
        description: "Balances speed against reasoning depth for everyday tasks, and what most \
                      Codex models run at when nothing is passed",
    },
    EffortInfo {
        id: "high",
        description: "Greater reasoning depth for complex problems, at more tokens and a longer \
                      wait",
    },
    EffortInfo {
        id: "xhigh",
        description: "Extra reasoning depth for the complex problems `high` does not settle",
    },
    EffortInfo {
        id: "max",
        description: "Maximum reasoning depth on a single task, for the hardest problems",
    },
    EffortInfo {
        id: "ultra",
        description: "Maximum reasoning, and the model delegates parts of the task to subagents \
                      of its own",
    },
];

const CLAUDE_CODE: &[ModelInfo] = &[
    ModelInfo {
        id: "claude-fable-5",
        description: "Frontier: the deepest reasoning, for long agentic runs and whole-repo work",
        tier: ModelTier::Frontier,
        cost: Some(5),
        speed: Some(1),
        best_for: &[
            "cross-subsystem design",
            "long agentic runs over a whole repository",
            "subtle concurrency or correctness risk",
        ],
        avoid_for: &[
            "well-specified single-file fixes",
            "reviewing routine changes",
            "high-volume mechanical edits",
        ],
        efforts: CLAUDE_FIVE,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-opus-5",
        description: "Opus tier: heavy analysis and hard multi-file engineering",
        tier: ModelTier::Frontier,
        cost: Some(5),
        speed: Some(2),
        best_for: &[
            "hard multi-file engineering",
            "cross-subsystem design",
            "debugging a failure nothing has explained yet",
        ],
        avoid_for: &[
            "well-specified single-file fixes",
            "high-volume mechanical edits",
        ],
        efforts: CLAUDE_FIVE,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-sonnet-5",
        description: "Sonnet tier: the production balance of speed and capability",
        tier: ModelTier::Balanced,
        cost: Some(3),
        speed: Some(3),
        best_for: &[
            "everyday feature work",
            "well-specified multi-file changes",
            "reviewing routine changes",
        ],
        avoid_for: &["cross-subsystem design", "under-specified work"],
        efforts: CLAUDE_FIVE,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-opus-4-8",
        description: "Opus tier: the previous flagship, pinned for reproducible runs",
        tier: ModelTier::Strong,
        cost: Some(5),
        speed: Some(2),
        best_for: &[
            "hard multi-file engineering",
            "reproducing a run that was tuned against it",
        ],
        avoid_for: &[
            "well-specified single-file fixes",
            "high-volume mechanical edits",
        ],
        efforts: CLAUDE_FIVE,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-opus-4-7",
        description: "Opus tier: architecture refactors and stubborn bugs; runs deep by default",
        tier: ModelTier::Strong,
        cost: Some(5),
        speed: Some(1),
        best_for: &[
            "architecture refactors",
            "stubborn bugs nothing else has fixed",
            "subtle concurrency or correctness risk",
        ],
        avoid_for: &["well-specified single-file fixes", "latency-sensitive work"],
        efforts: CLAUDE_FIVE,
        default_effort: Some("xhigh"),
    },
    ModelInfo {
        id: "claude-opus-4-6",
        description: "Opus tier: older reasoning workhorse, no `xhigh`",
        tier: ModelTier::Strong,
        cost: Some(4),
        speed: Some(2),
        best_for: &["hard multi-file engineering", "reasoning-heavy analysis"],
        avoid_for: &[
            "well-specified single-file fixes",
            "reviewing routine changes",
        ],
        efforts: CLAUDE_FOUR,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-sonnet-4-6",
        description: "Sonnet tier: quick diagnostics and everyday edits",
        tier: ModelTier::Balanced,
        cost: Some(3),
        speed: Some(4),
        best_for: &[
            "everyday edits",
            "quick diagnostics",
            "reviewing routine changes",
        ],
        avoid_for: &[
            "cross-subsystem design",
            "subtle concurrency or correctness risk",
        ],
        efforts: CLAUDE_FOUR,
        default_effort: Some("high"),
    },
    ModelInfo {
        id: "claude-sonnet-4-5",
        description: "Sonnet tier: legacy staple, with no effort control",
        tier: ModelTier::Balanced,
        cost: Some(3),
        speed: Some(4),
        best_for: &[
            "everyday edits",
            "reproducing a run that was tuned against it",
        ],
        avoid_for: &[
            "cross-subsystem design",
            "work that wants a reasoning effort",
        ],
        efforts: &[],
        default_effort: None,
    },
    ModelInfo {
        id: "claude-haiku-4-5",
        description: "Haiku tier: fastest and cheapest; inline edits and shell tasks",
        tier: ModelTier::Fast,
        cost: Some(2),
        speed: Some(5),
        best_for: &[
            "inline edits",
            "shell and file chores",
            "high-volume mechanical edits",
        ],
        avoid_for: &[
            "cross-subsystem design",
            "subtle concurrency or correctness risk",
            "under-specified work",
        ],
        efforts: &[],
        default_effort: None,
    },
];

const CODEX: &[ModelInfo] = &[
    ModelInfo {
        id: "gpt-5.6-sol",
        description: "Flagship reasoning: long-horizon agentic loops and codebase-wide planning",
        tier: ModelTier::Frontier,
        cost: Some(4),
        speed: Some(2),
        best_for: &[
            "under-specified work that needs judgement",
            "long-horizon agentic loops",
            "codebase-wide planning",
        ],
        avoid_for: &[
            "well-specified single-file fixes",
            "high-volume mechanical edits",
        ],
        efforts: CODEX_SIX,
        default_effort: Some("low"),
    },
    ModelInfo {
        id: "gpt-5.6-terra",
        description: "Balanced: the everyday engineering model, deep when asked to be",
        tier: ModelTier::Balanced,
        cost: Some(3),
        speed: Some(3),
        best_for: &[
            "everyday engineering",
            "well-specified multi-file changes",
            "reviewing routine changes",
        ],
        avoid_for: &["cross-subsystem design", "under-specified work"],
        efforts: CODEX_SIX,
        default_effort: Some("medium"),
    },
    ModelInfo {
        id: "gpt-5.6-luna",
        description: "Fast and cheapest: subagent queues, small edits, high-volume runs",
        tier: ModelTier::Fast,
        cost: Some(2),
        speed: Some(4),
        best_for: &[
            "well-specified single-file fixes",
            "high-volume mechanical edits",
            "reviewing routine changes",
        ],
        avoid_for: &[
            "cross-subsystem design",
            "subtle concurrency or correctness risk",
        ],
        efforts: CODEX_FIVE,
        default_effort: Some("medium"),
    },
    ModelInfo {
        id: "gpt-5.3-codex-spark",
        description: "Real-time coding preview: low-latency pairing on a live file",
        tier: ModelTier::Fast,
        cost: Some(2),
        speed: Some(5),
        best_for: &["near-instant iteration on one file", "small scoped edits"],
        avoid_for: &[
            "cross-subsystem design",
            "long agentic runs",
            "subtle concurrency or correctness risk",
        ],
        efforts: CODEX_SIX,
        default_effort: Some("medium"),
    },
    ModelInfo {
        id: "gpt-5.5",
        description: "Legacy reasoning: pinned for work that was tuned against it",
        tier: ModelTier::Strong,
        cost: Some(4),
        speed: Some(2),
        best_for: &[
            "reasoning-heavy analysis",
            "reproducing a run that was tuned against it",
        ],
        avoid_for: &["high-volume mechanical edits", "latency-sensitive work"],
        efforts: &["low", "medium", "high", "xhigh"],
        default_effort: Some("medium"),
    },
];

/// What is known about the models opencode fronts, keyed by the
/// `provider/model` id it prints them under — an overlay on discovery, laid
/// over the line discovery can derive on its own.
///
/// The `opencode/*` entries are opencode zen's free tier: they cost nothing,
/// which is the one thing about them that is certain and the reason to reach
/// for one, and how they behave on a real repository is not something this
/// catalog has established — so they say that rather than claiming a
/// capability nobody here has measured. `speed` is `None` throughout: nothing
/// has timed them, and a locally served model answers at whatever speed the
/// machine it is on manages.
const OPENCODE: &[(&str, ModelProfile)] = &[
    (
        "opencode/big-pickle",
        ModelProfile {
            description: "Free on opencode zen: a reasoning model with a 200k context and no \
                          effort control",
            tier: ModelTier::Balanced,
            cost: Some(1),
            speed: None,
            best_for: &[
                "work whose cost has to be zero",
                "throwaway or exploratory runs",
            ],
            avoid_for: &[
                "cross-subsystem design",
                "work that has to be right first time",
            ],
        },
    ),
    (
        "opencode/hy3-free",
        ModelProfile {
            description: "Free on opencode zen: a reasoning model with a 190k context, run at \
                          low, medium or high",
            tier: ModelTier::Balanced,
            cost: Some(1),
            speed: None,
            best_for: &[
                "work whose cost has to be zero",
                "throwaway or exploratory runs",
            ],
            avoid_for: &[
                "cross-subsystem design",
                "work that has to be right first time",
            ],
        },
    ),
    (
        "opencode/ling-3.0-flash-fin-free",
        ModelProfile {
            description: "Free on opencode zen: the flash tier of Ling 3.0, a 262k context, run \
                          at low, medium or high",
            tier: ModelTier::Fast,
            cost: Some(1),
            speed: None,
            best_for: &[
                "high-volume mechanical edits",
                "work whose cost has to be zero",
            ],
            avoid_for: &[
                "cross-subsystem design",
                "subtle concurrency or correctness risk",
            ],
        },
    ),
    (
        "opencode/mimo-v2.5-free",
        ModelProfile {
            description: "Free on opencode zen: a reasoning model with a 200k context and no \
                          effort control",
            tier: ModelTier::Balanced,
            cost: Some(1),
            speed: None,
            best_for: &[
                "work whose cost has to be zero",
                "throwaway or exploratory runs",
            ],
            avoid_for: &[
                "cross-subsystem design",
                "work that has to be right first time",
            ],
        },
    ),
    (
        "opencode/muse-spark-1.2-contributor-free",
        ModelProfile {
            description: "Free on opencode zen: a million-token context and the widest effort \
                          ladder here, minimal through xhigh",
            tier: ModelTier::Strong,
            cost: Some(1),
            speed: None,
            best_for: &[
                "work whose cost has to be zero",
                "transcripts a smaller context would not hold",
            ],
            avoid_for: &[
                "work that has to be right first time",
                "latency-sensitive work",
            ],
        },
    ),
    (
        "opencode/nemotron-3-ultra-free",
        ModelProfile {
            description: "Free on opencode zen: the ultra tier of Nemotron 3, a million-token \
                          context and no effort control",
            tier: ModelTier::Strong,
            cost: Some(1),
            speed: None,
            best_for: &[
                "work whose cost has to be zero",
                "transcripts a smaller context would not hold",
            ],
            avoid_for: &[
                "work that has to be right first time",
                "latency-sensitive work",
            ],
        },
    ),
    (
        "opencode/nemotron-3.5-lightning-free",
        ModelProfile {
            description: "Free on opencode zen: the lightning tier of Nemotron 3.5, a 262k \
                          context and no effort control",
            tier: ModelTier::Fast,
            cost: Some(1),
            speed: None,
            best_for: &[
                "high-volume mechanical edits",
                "work whose cost has to be zero",
            ],
            avoid_for: &[
                "cross-subsystem design",
                "subtle concurrency or correctness risk",
            ],
        },
    ),
    (
        "ollama/qwen3.6-code",
        ModelProfile {
            description: "Served locally by Ollama: free and private, but a 32k context and no \
                          reasoning — fast and small",
            tier: ModelTier::Fast,
            cost: Some(1),
            speed: None,
            best_for: &[
                "small local edits",
                "working offline",
                "high-volume mechanical edits",
            ],
            avoid_for: &[
                "work that does not fit in a 32k context",
                "cross-subsystem design",
                "subtle concurrency or correctness risk",
            ],
        },
    ),
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

    /// Every curated model's efforts, cheapest first, and what its CLI runs it
    /// at when nothing is passed — the catalog everything else validates
    /// against, written out so a refresh has to come through here.
    #[test]
    fn every_curated_model_lists_its_efforts_and_its_default() {
        let five = ["low", "medium", "high", "xhigh", "max"];
        let four = ["low", "medium", "high", "max"];
        let six = ["low", "medium", "high", "xhigh", "max", "ultra"];
        let want: &[(AgentKind, &str, &[&str], Option<&str>)] = &[
            (AgentKind::ClaudeCode, "claude-fable-5", &five, Some("high")),
            (AgentKind::ClaudeCode, "claude-opus-5", &five, Some("high")),
            (
                AgentKind::ClaudeCode,
                "claude-sonnet-5",
                &five,
                Some("high"),
            ),
            (
                AgentKind::ClaudeCode,
                "claude-opus-4-8",
                &five,
                Some("high"),
            ),
            (
                AgentKind::ClaudeCode,
                "claude-opus-4-7",
                &five,
                Some("xhigh"),
            ),
            (
                AgentKind::ClaudeCode,
                "claude-opus-4-6",
                &four,
                Some("high"),
            ),
            (
                AgentKind::ClaudeCode,
                "claude-sonnet-4-6",
                &four,
                Some("high"),
            ),
            (AgentKind::ClaudeCode, "claude-sonnet-4-5", &[], None),
            (AgentKind::ClaudeCode, "claude-haiku-4-5", &[], None),
            (AgentKind::Codex, "gpt-5.6-sol", &six, Some("low")),
            (AgentKind::Codex, "gpt-5.6-terra", &six, Some("medium")),
            (AgentKind::Codex, "gpt-5.6-luna", &five, Some("medium")),
            (
                AgentKind::Codex,
                "gpt-5.3-codex-spark",
                &six,
                Some("medium"),
            ),
            (
                AgentKind::Codex,
                "gpt-5.5",
                &["low", "medium", "high", "xhigh"],
                Some("medium"),
            ),
        ];
        for (kind, id, efforts, default_effort) in want {
            let model = curated_models(*kind)
                .iter()
                .find(|m| m.id == *id)
                .unwrap_or_else(|| panic!("{id} is not curated"));
            assert_eq!(model.efforts, *efforts, "{id}");
            assert_eq!(model.default_effort, *default_effort, "{id}");
        }
        for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
            assert_eq!(
                curated_models(kind).len(),
                want.iter().filter(|w| w.0 == kind).count()
            );
        }
    }

    /// Every effort an agent CLI accepts says what spending it buys: the
    /// ladder is what the daemon describes an entry's efforts from, so a rung
    /// missing from it would reach a planner as a bare word.
    #[test]
    fn every_effort_of_every_cli_says_what_it_buys() {
        for kind in AgentKind::ALL {
            let ladder = effort_ladder(kind);
            assert_eq!(
                ladder.iter().map(|e| e.id).collect::<Vec<_>>(),
                known_efforts(kind),
                "{kind:?}: the ladder is the efforts, in their own order"
            );
            for effort in ladder {
                assert!(!effort.description.is_empty(), "{}", effort.id);
                assert_eq!(
                    effort_description(kind, effort.id),
                    Some(effort.description)
                );
            }
        }
        assert_eq!(effort_description(AgentKind::ClaudeCode, "ultra"), None);
        assert_eq!(effort_description(AgentKind::Opencode, "high"), None);
    }

    /// Every curated model says what it is for: a tier, bands a planner can
    /// compare across agent CLIs, and the task shapes it is and is not the
    /// choice for.
    #[test]
    fn every_curated_model_says_what_it_is_for() {
        for kind in AgentKind::ALL {
            for model in curated_models(kind) {
                let ModelInfo {
                    id,
                    description,
                    tier,
                    cost,
                    speed,
                    best_for,
                    avoid_for,
                    ..
                } = model;
                assert!(!description.is_empty(), "{id}");
                assert_ne!(*tier, ModelTier::Unknown, "{id}");
                for band in [cost, speed] {
                    let band = band.unwrap_or_else(|| panic!("{id} is unranked"));
                    assert!((1..=5).contains(&band), "{id}: {band}");
                }
                assert!(!best_for.is_empty(), "{id}");
                assert!(!avoid_for.is_empty(), "{id}");
                for shape in best_for.iter().chain(avoid_for.iter()) {
                    assert!(!shape.is_empty(), "{id}");
                }
            }
        }
    }

    /// What is known about an opencode model is an overlay on discovery, and
    /// a provider fronting a model this catalog already curates borrows that
    /// entry rather than repeating it.
    #[test]
    fn an_opencode_model_borrows_a_curated_sibling_or_its_own_overlay() {
        let free = opencode_profile("opencode/hy3-free").expect("curated");
        assert_eq!(free.tier, ModelTier::Balanced);
        assert_eq!(free.cost, Some(1), "free is the cheapest band there is");
        assert_eq!(free.speed, None, "nothing has timed it");
        assert!(!free.best_for.is_empty() && !free.avoid_for.is_empty());

        let opus = curated_models(AgentKind::ClaudeCode)
            .iter()
            .find(|m| m.id == "claude-opus-5")
            .expect("curated");
        assert_eq!(
            opencode_profile("anthropic/claude-opus-5"),
            Some(opus.profile()),
            "the same model, said once"
        );
        let luna = curated_models(AgentKind::Codex)
            .iter()
            .find(|m| m.id == "gpt-5.6-luna")
            .expect("curated");
        assert_eq!(
            opencode_profile("openai/gpt-5.6-luna"),
            Some(luna.profile())
        );
        assert_eq!(opencode_profile("anthropic/claude-nothing"), None);
        assert_eq!(opencode_profile("someone/else"), None);
    }

    /// Every overlaid model says something worth overlaying: a cost band and
    /// the task shapes it is and is not for.
    #[test]
    fn every_overlaid_opencode_model_says_what_it_is_for() {
        for (id, profile) in OPENCODE {
            assert!(!profile.description.is_empty(), "{id}");
            assert_eq!(profile.cost, Some(1), "{id}: everything here is free");
            assert!(!profile.best_for.is_empty(), "{id}");
            assert!(!profile.avoid_for.is_empty(), "{id}");
        }
        let ids: Vec<_> = OPENCODE.iter().map(|(id, _)| *id).collect();
        let mut deduped = ids.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len());
    }

    /// A model the catalog does not know falls back to everything its CLI
    /// accepts — and opencode has no such list, because its efforts are its
    /// models' own variant names.
    #[test]
    fn a_cli_has_the_efforts_its_own_models_are_run_at() {
        assert_eq!(
            known_efforts(AgentKind::ClaudeCode),
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            known_efforts(AgentKind::Codex),
            ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]
        );
        assert_eq!(known_efforts(AgentKind::Opencode), [] as [&str; 0]);
        for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
            for model in curated_models(kind) {
                for effort in model.efforts {
                    assert!(
                        known_efforts(kind).contains(effort),
                        "{}: {effort} is no effort of {kind:?}",
                        model.id
                    );
                }
            }
        }
    }

    /// A model the catalog knows takes exactly its own efforts, and a refusal
    /// names the effort that was asked for and lists the ones there are.
    #[test]
    fn a_known_model_takes_its_own_efforts_and_nothing_else() {
        let efforts = strings(&["low", "medium", "high"]);
        assert_eq!(
            effort_error(AgentKind::Codex, Some(&efforts), "medium"),
            None
        );
        let err = effort_error(AgentKind::Codex, Some(&efforts), "ultra").expect("no such effort");
        assert!(err.contains("`ultra` is no effort of that model"), "{err}");
        assert!(err.contains("low, medium, high"), "{err}");
    }

    /// A model with no effort control at all refuses every effort, and says
    /// that is what it is rather than listing nothing.
    #[test]
    fn a_model_with_no_effort_control_takes_none() {
        let err = effort_error(AgentKind::ClaudeCode, Some(&[]), "high").expect("none at all");
        assert!(err.contains("`high` is no effort"), "{err}");
        assert!(err.contains("takes none at all"), "{err}");
    }

    /// A hand-typed claude_code or codex model is held to everything its CLI
    /// accepts, which is the most that can be said about it.
    #[test]
    fn an_unlisted_model_of_a_curated_cli_is_held_to_the_clis_own_efforts() {
        assert_eq!(effort_error(AgentKind::Codex, None, "minimal"), None);
        assert_eq!(effort_error(AgentKind::ClaudeCode, None, "xhigh"), None);
        let err = effort_error(AgentKind::ClaudeCode, None, "minimal").expect("not claude's");
        assert!(
            err.contains("`minimal` is no effort of claude_code"),
            "{err}"
        );
        assert!(err.contains("low, medium, high, xhigh, max"), "{err}");
        let err = effort_error(AgentKind::Codex, None, "gigantic").expect("not codex's");
        assert!(err.contains("`gigantic` is no effort of codex"), "{err}");
    }

    /// OpenCode: a discovered model takes the variants it was discovered
    /// with, and one nothing has listed takes any name that is not blank.
    #[test]
    fn an_opencode_model_takes_its_discovered_variants_or_any_name() {
        let variants = strings(&["low", "high"]);
        assert_eq!(
            effort_error(AgentKind::Opencode, Some(&variants), "high"),
            None
        );
        let err =
            effort_error(AgentKind::Opencode, Some(&variants), "medium").expect("undiscovered");
        assert!(err.contains("`medium` is no effort of that model"), "{err}");
        assert!(err.contains("low, high"), "{err}");
        assert_eq!(effort_error(AgentKind::Opencode, None, "whatever"), None);
        let err = effort_error(AgentKind::Opencode, None, " ").expect("blank");
        assert!(err.contains("no effort was named"), "{err}");
    }

    fn strings(efforts: &[&str]) -> Vec<String> {
        efforts.iter().map(|e| e.to_string()).collect()
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
