//! The contract every agent adapter meets, and how one CLI spells it.
//!
//! Three CLIs are driven by hand-written adapters, and a fourth is meant to
//! be cheap. What makes it cheap is that the clauses below hold for all of
//! them: only the spelling of each clause changes from CLI to CLI. So an
//! adapter declares its spellings ([`AgentAdapter::contract`]), and one test
//! suite (`tests/adapter_contract.rs`) reads the declaration and holds the
//! planned launch to it.
//!
//! The clauses, in the order the spec states them (007):
//!
//! 1. **argv** — a launch runs the binary of its agent kind, and no argument
//!    of it is empty.
//! 2. **environment** — every launch carries the Ariadne session context, and
//!    runs in the directory the launcher named.
//! 3. **generated config** — everything a launch generates is written in the
//!    run dir of its session, and nothing at all in the worktree.
//! 4. **model** — the pinned model is passed on every launch.
//! 5. **effort** — the pinned effort is passed on every launch, and nothing
//!    is passed where the session pinned none.
//! 6. **system prompt** — a spawn brief the agent with the system prompt.
//! 7. **MCP** — every launch points the CLI at `ariadne mcp serve`, with
//!    nothing after `serve` and the session context in the server's
//!    environment.
//! 8. **hooks** — every launch tells the CLI to report its events to
//!    `ariadne agent-event --kind <agent kind>`.
//! 9. **flags** — the flags of the agent config reach the argv once, and the
//!    adapter adds no flag of the user's own to them.
//! 10. **resume** — a resume names the session it continues, and delivers its
//!     instruction once; an empty instruction delivers nothing.
//! 11. **session id** — a spawn knows the CLI's own session id only where the
//!     CLI lets the caller choose it.
//! 12. **skills** — the skill documents the launcher wrote reach the CLI the
//!     way that CLI takes them.
//! 13. **trust dialogs** — a spawn types nothing into the pane, so a dialog
//!     the CLI opens stands until the user answers it.
//! 14. **compaction** — the adapter knows the event with which its CLI says a
//!     compaction is over.

use std::path::Path;

use super::SpawnPlan;

/// Where one value of the contract is written in a planned launch.
///
/// The same clause is a flag for one CLI, a config override for another and a
/// key of a generated file for the third. This is that difference, and
/// [`Spelling::read`] is what makes it one thing to a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    /// Two adjacent arguments: the flag, then the value.
    Flag(&'static str),
    /// A `-c <key>=<value>` config override: two adjacent arguments, with the
    /// value TOML-quoted.
    Override(&'static str),
    /// A JSON pointer into a file the adapter writes in the run dir.
    Config {
        file: &'static str,
        pointer: &'static str,
    },
    /// The CLI takes this value nowhere of its own, and it reaches the agent
    /// inside the prompt itself.
    InThePrompt,
}

impl Spelling {
    /// The value this clause carries in `plan`, or None where the launch left
    /// it out.
    ///
    /// [`Spelling::InThePrompt`] reads nothing: the prompt is one string, and
    /// what it holds is asserted on the prompt.
    pub fn read(&self, plan: &SpawnPlan, run_dir: &Path) -> Option<String> {
        match self {
            Spelling::Flag(flag) => plan
                .argv
                .windows(2)
                .find(|pair| pair[0] == *flag)
                .map(|pair| pair[1].clone()),
            Spelling::Override(key) => {
                let prefix = format!("{key}=");
                plan.argv
                    .windows(2)
                    .find(|pair| pair[0] == "-c" && pair[1].starts_with(&prefix))
                    .map(|pair| pair[1][prefix.len()..].trim_matches('"').to_string())
            }
            Spelling::Config { file, pointer } => {
                let text = std::fs::read_to_string(run_dir.join(file)).ok()?;
                let json: serde_json::Value = serde_json::from_str(&text).ok()?;
                match json.pointer(pointer)? {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(value) => Some(value.clone()),
                    other => Some(other.to_string()),
                }
            }
            Spelling::InThePrompt => None,
        }
    }
}

/// Where a CLI's per-event hook declarations are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookSite {
    /// `-c hooks.<Event>=[...]` on the argv.
    Override,
    /// Under `/hooks/<Event>` of a JSON file the adapter generates.
    Config(&'static str),
}

/// How a CLI is told to report what it does back to the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDelivery {
    /// One command hook per event, each running `ariadne agent-event`.
    Hooks {
        site: HookSite,
        events: &'static [&'static str],
    },
    /// A plugin the daemon installs once, named by the generated config.
    Plugin(Spelling),
}

impl EventDelivery {
    /// Every event declaration of this launch: the event, and the text
    /// declaring it. Empty where a launch declares none.
    ///
    /// A plugin forwards every event it knows on its own, so it answers with
    /// one entry naming the plugin.
    pub fn declarations(&self, plan: &SpawnPlan, run_dir: &Path) -> Vec<(String, String)> {
        match self {
            EventDelivery::Hooks {
                site: HookSite::Override,
                events,
            } => events
                .iter()
                .filter_map(|event| {
                    let prefix = format!("hooks.{event}=");
                    plan.argv
                        .windows(2)
                        .find(|pair| pair[0] == "-c" && pair[1].starts_with(&prefix))
                        .map(|pair| ((*event).to_string(), pair[1][prefix.len()..].to_string()))
                })
                .collect(),
            EventDelivery::Hooks {
                site: HookSite::Config(file),
                events,
            } => {
                let Ok(text) = std::fs::read_to_string(run_dir.join(file)) else {
                    return Vec::new();
                };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                    return Vec::new();
                };
                events
                    .iter()
                    .filter_map(|event| {
                        json.pointer(&format!("/hooks/{event}"))
                            .map(|declared| ((*event).to_string(), declared.to_string()))
                    })
                    .collect()
            }
            EventDelivery::Plugin(spelling) => spelling
                .read(plan, run_dir)
                .map(|text| ("plugin".to_string(), text))
                .into_iter()
                .collect(),
        }
    }
}

/// How a resume hands the agent the instruction it resumes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionDelivery {
    /// The last argument of the argv.
    Argv,
    /// Typed into the pane once the TUI is up
    /// ([`SpawnPlan::post_launch_input`]), for a CLI that drops a prompt on
    /// its argv when it resumes.
    TypedIntoThePane,
}

/// The contract of one adapter, in the words of the CLI it drives.
///
/// One value per clause of the module documentation; the clauses that are the
/// same for every CLI — the session environment, the run dir, the flags of
/// the agent config — need no spelling here.
#[derive(Debug, Clone, Copy)]
pub struct AdapterContract {
    /// argv[0] of every launch: the executable of the agent kind.
    pub binary: &'static str,
    /// The `--kind` this CLI's events reach `ariadne agent-event` under.
    pub event_kind: &'static str,
    /// The files the adapter writes in the run dir on every launch.
    pub generated: &'static [&'static str],
    /// Where the system prompt is passed.
    pub system_prompt: Spelling,
    /// Where the pinned model is passed.
    pub model: Spelling,
    /// Where the pinned effort is passed.
    pub effort: Spelling,
    /// Where the directory holding the skill documents is passed.
    pub skills: Spelling,
    /// Where the MCP server's command — the `ariadne` binary — is passed.
    pub mcp_command: Spelling,
    /// Where the MCP server's arguments are passed. Its value ends with the
    /// `mcp serve` pair, whether or not the binary heads the same list.
    pub mcp_arguments: Spelling,
    /// Where the MCP server's environment is passed. Its value carries the
    /// session id, whether one key or a table of them.
    pub mcp_environment: Spelling,
    /// How the CLI is told to report its events.
    pub events: EventDelivery,
    /// Whether the adapter chooses the CLI's own session id before the launch.
    pub session_id_chosen_at_spawn: bool,
    /// How a resume delivers its instruction.
    pub resume_instruction: InstructionDelivery,
    /// The event kind with which this CLI says a compaction is over.
    pub compaction_event: &'static str,
}
