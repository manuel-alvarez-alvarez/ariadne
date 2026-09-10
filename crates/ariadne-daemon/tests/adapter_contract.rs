//! The contract test suite: one set of claims, held against every adapter.
//!
//! Spec 007 states the contract once and gives each CLI's spelling under it.
//! Here every clause is one test, and every test runs for all of
//! [`AgentKind::ALL`] — an adapter says how it spells a clause
//! ([`AgentAdapter::contract`]), and the test holds the planned launch to it.
//!
//! What each adapter spells its own way is asserted through the declaration;
//! what is the same for every CLI — the session environment, the run dir, the
//! flags of the agent config, the resume — is asserted directly. The per-CLI
//! spellings themselves are asserted whole in `adapters.rs`.

use std::path::{Path, PathBuf};

use ariadne_core::{AgentKind, Seat};
use ariadne_daemon::agents::contract::{EventDelivery, InstructionDelivery, Spelling};
use ariadne_daemon::agents::{SpawnCtx, SpawnPlan, adapter_for, write_skills};

const SESSION_ID: &str = "01sessionxxxxxxxxxxxxxxxxx";
const CLI_BIN: &str = "/usr/local/bin/ariadne";
const SYSTEM_PROMPT: &str = "SYSTEM PROMPT";
const INITIAL_PROMPT: &str = "DO THE TASK";
const MODEL: &str = "test-model";
const INSTRUCTION: &str = "apply feedback";

/// One launch under test: the run dir the adapter writes into, the worktree
/// it runs in, and the context the launcher would hand it.
struct Launch {
    run_dir: tempfile::TempDir,
    worktree: tempfile::TempDir,
    ctx: SpawnCtx,
}

impl Launch {
    /// A launch of a session pinned to a model, with one flag in its agent
    /// config, no effort and no skills.
    fn new() -> Launch {
        let run_dir = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let ctx = SpawnCtx {
            session_id: SESSION_ID.into(),
            launch_id: "01launchxxxxxxxxxxxxxxxxxx".into(),
            goal_id: "01goalxxxxxxxxxxxxxxxxxxxx".into(),
            task_id: Some("01taskxxxxxxxxxxxxxxxxxxxx".into()),
            seat: Seat::Author,
            run_dir: run_dir.path().into(),
            cwd: worktree.path().into(),
            socket_path: PathBuf::from("/tmp/ariadne.sock"),
            cli_bin: CLI_BIN.into(),
            system_prompt: SYSTEM_PROMPT.into(),
            skills_dir: None,
            initial_prompt: INITIAL_PROMPT.into(),
            model: MODEL.into(),
            effort: None,
            extra_flags: vec!["--extra".into()],
        };
        Launch {
            run_dir,
            worktree,
            ctx,
        }
    }

    fn with_flags(mut self, flags: &[&str]) -> Launch {
        self.ctx.extra_flags = flags.iter().map(|f| f.to_string()).collect();
        self
    }

    fn with_effort(mut self, effort: &str) -> Launch {
        self.ctx.effort = Some(effort.to_string());
        self
    }

    /// The launcher writes the agent's skills before the adapter plans
    /// anything; this is that state.
    fn with_skills(mut self) -> Launch {
        let documents = SKILLS
            .iter()
            .map(|(name, document)| (name.to_string(), document.to_string()))
            .collect::<Vec<_>>();
        self.ctx.skills_dir = Some(write_skills(self.run_dir.path(), &documents).unwrap());
        self
    }

    fn dir(&self) -> &Path {
        self.run_dir.path()
    }
}

const SKILLS: [(&str, &str); 2] = [
    ("coding", "---\nname: coding\n---\nWrite it."),
    ("testing", "---\nname: testing\n---\nProve it."),
];

/// Both launches of one context: a spawn, and a resume carrying an
/// instruction. Every clause that holds for a launch holds for both.
///
/// Each one is planned in a run dir of its own, from a context `shape` builds
/// afresh. An adapter rewrites its generated files on every launch, so two
/// launches sharing a run dir would let what the resume wrote answer for what
/// the spawn wrote, and a spawn that generated nothing would pass.
fn both(kind: AgentKind, shape: &dyn Fn() -> Launch) -> Vec<(Launch, SpawnPlan)> {
    let adapter = adapter_for(kind);
    let spawned = shape();
    let spawn = adapter.plan_spawn(&spawned.ctx).unwrap();
    let resumed = shape();
    let resume = adapter
        .plan_resume(&resumed.ctx, "id-1", INSTRUCTION)
        .unwrap();
    vec![(spawned, spawn), (resumed, resume)]
}

/// Clause 1. A launch runs the CLI of its agent kind, and hands it no empty
/// argument.
#[test]
fn every_launch_runs_the_binary_of_its_agent_kind() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        assert_eq!(contract.binary, kind.binary(), "{kind:?}");
        for (_launch, plan) in both(kind, &Launch::new) {
            assert_eq!(plan.argv[0], contract.binary, "{kind:?}: {:?}", plan.argv);
            assert!(
                plan.argv.iter().all(|argument| !argument.is_empty()),
                "{kind:?}: {:?}",
                plan.argv
            );
        }
    }
}

/// Clause 2. Every launch carries the Ariadne identity of its session, which
/// is what the MCP server and the hook sink read to act as it, and runs where
/// the launcher put it.
#[test]
fn every_launch_carries_the_session_context_in_its_environment() {
    for kind in AgentKind::ALL {
        for (launch, plan) in both(kind, &Launch::new) {
            let env: std::collections::HashMap<_, _> = plan.env.iter().cloned().collect();
            assert_eq!(env["ARIADNE_SESSION_ID"], SESSION_ID, "{kind:?}");
            assert_eq!(env["ARIADNE_LAUNCH_ID"], "01launchxxxxxxxxxxxxxxxxxx");
            assert_eq!(env["ARIADNE_GOAL_ID"], "01goalxxxxxxxxxxxxxxxxxxxx");
            assert_eq!(env["ARIADNE_TASK_ID"], "01taskxxxxxxxxxxxxxxxxxxxx");
            assert_eq!(env["ARIADNE_SEAT"], "author");
            assert_eq!(env["ARIADNE_SOCKET"], "/tmp/ariadne.sock");
            assert_eq!(plan.cwd, launch.ctx.cwd, "{kind:?}");
        }
    }
}

/// Clause 3. What a launch generates goes in the run dir of its session, and
/// nothing at all goes in the worktree, which belongs to the repository.
#[test]
fn every_launch_generates_its_files_in_the_run_dir_and_none_in_the_worktree() {
    for kind in AgentKind::ALL {
        for (launch, plan) in both(kind, &|| Launch::new().with_skills()) {
            for file in adapter_for(kind).contract().generated {
                assert!(
                    launch.dir().join(file).exists(),
                    "{kind:?}: no {file} in the run dir"
                );
            }
            assert_eq!(plan.cwd, launch.ctx.cwd);
            assert_eq!(
                std::fs::read_dir(launch.worktree.path()).unwrap().count(),
                0,
                "{kind:?} wrote into the worktree"
            );
        }
    }
}

/// Clause 4. The model the session is pinned to reaches the CLI on every
/// launch: nothing falls back to a CLI default.
#[test]
fn every_launch_passes_the_pinned_model() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        for (launch, plan) in both(kind, &Launch::new) {
            assert_eq!(
                contract.model.read(&plan, launch.dir()).as_deref(),
                Some(MODEL),
                "{kind:?}: {:?}",
                plan.argv
            );
        }
    }
}

/// Clause 5. A pinned effort reaches the CLI on every launch, and a session
/// that pinned none passes none: the CLI runs the model at its own default.
#[test]
fn an_effort_reaches_the_cli_only_when_the_session_pinned_one() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        for (launch, plan) in both(kind, &|| Launch::new().with_effort("xhigh")) {
            assert_eq!(
                contract.effort.read(&plan, launch.dir()).as_deref(),
                Some("xhigh"),
                "{kind:?}: {:?}",
                plan.argv
            );
        }

        for (launch, plan) in both(kind, &Launch::new) {
            assert_eq!(
                contract.effort.read(&plan, launch.dir()),
                None,
                "{kind:?}: {:?}",
                plan.argv
            );
        }
    }
}

/// Clause 6. A spawn briefs the agent with the system prompt — inside the
/// first message where the CLI takes no prompt of its own.
#[test]
fn every_spawn_briefs_the_agent_with_the_system_prompt() {
    for kind in AgentKind::ALL {
        let launch = Launch::new();
        let contract = adapter_for(kind).contract();
        let spawn = adapter_for(kind).plan_spawn(&launch.ctx).unwrap();
        match contract.system_prompt {
            Spelling::InThePrompt => assert!(
                spawn
                    .argv
                    .iter()
                    .any(|argument| argument.contains(SYSTEM_PROMPT)
                        && argument.contains(INITIAL_PROMPT)),
                "{kind:?}: {:?}",
                spawn.argv
            ),
            spelling => {
                // A CLI that takes the prompt out of band is given it again
                // on a resume: the run dir is rewritten on every launch.
                for (other, plan) in both(kind, &Launch::new) {
                    assert_eq!(
                        spelling.read(&plan, other.dir()).as_deref(),
                        Some(SYSTEM_PROMPT),
                        "{kind:?}: {:?}",
                        plan.argv
                    );
                }
            }
        }
    }
}

/// Clause 7. Every launch points the CLI at this daemon's MCP server — the
/// `ariadne` binary, `mcp serve` and nothing after it — running as this
/// session.
#[test]
fn every_launch_points_the_cli_at_the_ariadne_mcp_server() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        for (launch, plan) in both(kind, &Launch::new) {
            assert_eq!(
                contract.mcp_command.read(&plan, launch.dir()).as_deref(),
                Some(CLI_BIN),
                "{kind:?}: {:?}",
                plan.argv
            );
            let arguments = contract
                .mcp_arguments
                .read(&plan, launch.dir())
                .unwrap_or_else(|| panic!("{kind:?}: no MCP arguments in {:?}", plan.argv));
            // Exactly `mcp serve`, in that order, with nothing before it and
            // nothing after. One of the three keeps the binary at the head of
            // the same list, so that is the second form the list may take —
            // and the binary is admitted there and nowhere else.
            let packed: String = arguments
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            assert!(
                packed == r#"["mcp","serve"]"#
                    || packed == format!(r#"["{CLI_BIN}","mcp","serve"]"#),
                "{kind:?}: the MCP server is not started as `mcp serve`: {arguments}"
            );
            let environment = contract
                .mcp_environment
                .read(&plan, launch.dir())
                .unwrap_or_else(|| panic!("{kind:?}: no MCP environment in {:?}", plan.argv));
            assert!(environment.contains(SESSION_ID), "{kind:?}: {environment}");
        }
    }
}

/// Clause 8. Every launch tells the CLI to report what it does to
/// `ariadne agent-event`, under the kind of its agent.
#[test]
fn every_launch_reports_the_cli_events_to_the_daemon() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        assert_eq!(contract.event_kind, kind.as_str(), "{kind:?}");
        for (launch, plan) in both(kind, &Launch::new) {
            let declared = contract.events.declarations(&plan, launch.dir());
            match contract.events {
                EventDelivery::Hooks { events, .. } => {
                    assert_eq!(declared.len(), events.len(), "{kind:?}: {declared:?}");
                    let command = format!("{CLI_BIN} agent-event --kind {}", contract.event_kind);
                    for (event, text) in declared {
                        assert!(text.contains(&command), "{kind:?} {event}: {text}");
                    }
                }
                EventDelivery::Plugin(_) => {
                    let plugin = ariadne_daemon::opencode_plugin::plugin_path();
                    assert_eq!(
                        declared,
                        vec![("plugin".to_string(), format!("file://{}", plugin.display()))],
                        "{kind:?}"
                    );
                }
            }
        }
    }
}

/// Clause 9. The flags of the agent config reach every launch once, and the
/// adapter adds none of its own — an agent whose flags the user emptied is
/// launched with no permission bypass at all.
#[test]
fn the_configured_flags_reach_every_launch_once_and_the_adapter_adds_none() {
    for kind in AgentKind::ALL {
        for (_launch, plan) in both(kind, &|| Launch::new().with_flags(&["--sandbox=off"])) {
            assert_eq!(
                plan.argv
                    .iter()
                    .filter(|argument| *argument == "--sandbox=off")
                    .count(),
                1,
                "{kind:?}: {:?}",
                plan.argv
            );
        }

        for (_launch, plan) in both(kind, &|| Launch::new().with_flags(&[])) {
            for flag in kind.default_flags() {
                assert!(
                    !plan.argv.contains(&flag.to_string()),
                    "{kind:?} added {flag} itself: {:?}",
                    plan.argv
                );
            }
        }
    }
}

/// Clause 10. A resume names the session it continues and delivers its
/// instruction once, through the one channel the CLI takes it on.
#[test]
fn a_resume_names_its_session_and_delivers_its_instruction_once() {
    for kind in AgentKind::ALL {
        let launch = Launch::new();
        let contract = adapter_for(kind).contract();
        let plan = adapter_for(kind)
            .plan_resume(&launch.ctx, "id-1", INSTRUCTION)
            .unwrap();
        assert_eq!(
            plan.internal_session_id.as_deref(),
            Some("id-1"),
            "{kind:?}"
        );
        assert!(
            plan.argv.contains(&"id-1".to_string()),
            "{kind:?}: {:?}",
            plan.argv
        );
        let on_argv = plan
            .argv
            .iter()
            .filter(|argument| *argument == INSTRUCTION)
            .count();
        match contract.resume_instruction {
            InstructionDelivery::Argv => {
                assert_eq!(on_argv, 1, "{kind:?}: {:?}", plan.argv);
                assert_eq!(plan.argv.last().unwrap(), INSTRUCTION, "{kind:?}");
                assert!(plan.post_launch_input.is_none(), "{kind:?}");
            }
            InstructionDelivery::TypedIntoThePane => {
                assert_eq!(on_argv, 0, "{kind:?}: {:?}", plan.argv);
                assert_eq!(plan.post_launch_input.as_deref(), Some(INSTRUCTION));
            }
        }
    }
}

/// Clause 10. An empty instruction is an interactive resume — what
/// `ariadne attach` revives a session with — and delivers nothing: the agent
/// drops into its TUI and waits for the user.
#[test]
fn an_interactive_resume_delivers_no_instruction() {
    for kind in AgentKind::ALL {
        // One context for both plans: what is compared here is the argv, and
        // two run dirs would differ in the paths on it.
        let launch = Launch::new();
        let adapter = adapter_for(kind);
        let instructed = adapter
            .plan_resume(&launch.ctx, "id-1", INSTRUCTION)
            .unwrap();
        let interactive = adapter.plan_resume(&launch.ctx, "id-1", "").unwrap();

        assert!(interactive.post_launch_input.is_none(), "{kind:?}");
        // Exactly the instructed launch with the instruction taken off it:
        // an empty instruction delivers nothing, and puts nothing of the
        // adapter's own in its place.
        let expected = match adapter.contract().resume_instruction {
            InstructionDelivery::Argv => &instructed.argv[..instructed.argv.len() - 1],
            InstructionDelivery::TypedIntoThePane => &instructed.argv[..],
        };
        assert_eq!(
            interactive.argv, expected,
            "{kind:?}: an interactive resume carries something of its own"
        );
    }
}

/// Clause 11. A spawn knows the CLI's own session id up front only where the
/// CLI lets the caller choose it; the rest wait for the event that carries it.
#[test]
fn a_spawn_knows_its_session_id_only_where_the_cli_lets_it_be_chosen() {
    for kind in AgentKind::ALL {
        let launch = Launch::new();
        let contract = adapter_for(kind).contract();
        let plan = adapter_for(kind).plan_spawn(&launch.ctx).unwrap();
        assert_eq!(
            plan.internal_session_id.is_some(),
            contract.session_id_chosen_at_spawn,
            "{kind:?}: {:?}",
            plan.internal_session_id
        );
    }
}

/// Clause 12. The skill documents the launcher wrote reach the CLI the way
/// that CLI takes a folder of them, and an agent that loads none is pointed
/// at nothing.
#[test]
fn the_skill_documents_reach_the_cli_the_way_it_takes_them() {
    for kind in AgentKind::ALL {
        let contract = adapter_for(kind).contract();
        for (launch, plan) in both(kind, &|| Launch::new().with_skills()) {
            let delivered = contract.skills.read(&plan, launch.dir());
            if contract.skills == Spelling::InThePrompt {
                // Nothing of its own: the index in the system prompt names
                // every document by its run-dir path.
                assert_eq!(delivered, None, "{kind:?}");
                continue;
            }
            let root = PathBuf::from(
                delivered.unwrap_or_else(|| panic!("{kind:?}: no skills in {:?}", plan.argv)),
            );
            for (name, document) in SKILLS {
                let found = skill_under(&root, name)
                    .unwrap_or_else(|| panic!("{kind:?}: no {name} under {}", root.display()));
                assert_eq!(
                    std::fs::read_to_string(found).unwrap(),
                    document,
                    "{kind:?}"
                );
            }
        }

        for (launch, plan) in both(kind, &Launch::new) {
            assert_eq!(
                contract.skills.read(&plan, launch.dir()),
                None,
                "{kind:?}: {:?}",
                plan.argv
            );
        }
    }
}

/// The `<name>/SKILL.md` of one skill under the folder the adapter delivered,
/// wherever in it the CLI's own layout puts it.
fn skill_under(root: &Path, name: &str) -> Option<PathBuf> {
    let document = root.join(name).join("SKILL.md");
    if document.exists() {
        return Some(document);
    }
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(found) = skill_under(&path, name)
        {
            return Some(found);
        }
    }
    None
}

/// Clause 13. A spawn types nothing into the pane. A CLI that opens a
/// directory-trust dialog over the worktree has it answered by the user, and
/// the daemon presses nothing on its own account (008, 009).
#[test]
fn a_spawn_types_nothing_into_the_pane() {
    for kind in AgentKind::ALL {
        let launch = Launch::new();
        let plan = adapter_for(kind).plan_spawn(&launch.ctx).unwrap();
        assert!(
            plan.post_launch_input.is_none(),
            "{kind:?}: {:?}",
            plan.post_launch_input
        );
    }
}

/// Clause 14. The adapter knows the event with which its CLI says a
/// compaction is over, and reads no other event as one.
#[test]
fn each_cli_says_a_compaction_is_over_in_the_event_the_contract_names() {
    // The payload carries what Claude Code needs to tell a compacted session
    // start from a resumed one; the other two read no payload at all.
    let compacted = serde_json::json!({ "source": "compact" });
    for kind in AgentKind::ALL {
        let adapter = adapter_for(kind);
        let event = adapter.contract().compaction_event;
        assert!(adapter.compaction_done(event, &compacted), "{kind:?}");
        for other in AgentKind::ALL.map(|other| adapter_for(other).contract().compaction_event) {
            if other != event {
                assert!(
                    !adapter.compaction_done(other, &compacted),
                    "{kind:?} {other}"
                );
            }
        }
    }
}
