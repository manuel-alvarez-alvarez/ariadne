---
id: agent-cli-adapters
status: current
updated: 2026-09-10
areas: [daemon, core]
commits: [ed1c40d3, 03fbf02d, 090c5158, e94647fd, a69b953f, 03f9c8b7]
tests:
  - crates/ariadne-daemon/src/launcher.rs
  - crates/ariadne-daemon/tests/adapter_contract.rs
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-daemon/tests/agents.rs
  - crates/ariadne-daemon/tests/resume.rs
---

# Agent CLI adapters

How Ariadne launches a concrete coding-agent CLI: the argv, the environment,
the generated config, and the hooks that report back.

Three CLIs are supported — **Claude Code**, **OpenAI Codex CLI** and
**OpenCode** — and an adapter turns a spawn or a resume request into the argv,
the env and the generated files of one of them. Every adapter meets the same
contract, and only the spelling of each clause changes from CLI to CLI. That
is the whole of what a fourth CLI has to do.

## Scope

In: the contract an adapter meets, the spelling each CLI takes it in,
per-agent launch flags, the session context in the environment, hook
installation, and resuming or reviving a session.

Out: which model to pick (011), what the session is briefed with (006), and
what a skill says (017).

## The contract

Fourteen clauses, each of them true of every adapter. `AdapterContract`
(`crates/ariadne-daemon/src/agents/contract.rs`) is this list as one Rust
interface: an adapter declares how it spells each clause, and one suite —
`tests/adapter_contract.rs` — holds every adapter to it.

1. **argv.** A launch runs the binary of its agent kind, and hands it no empty
   argument.
2. **Environment.** Every launch carries the Ariadne identity of its session —
   session, launch, goal, seat and task, and the daemon's socket — which is
   what the MCP server and the hook sink read to act as that session. It runs
   in the directory the launcher named.
3. **Generated config.** Everything a launch generates is written in the run
   directory of its session, and nothing at all in the worktree, which belongs
   to the repository.
4. **Model.** The model the session is pinned to is passed on every launch.
   Nothing falls back to a CLI default, and a model is never dropped silently.
5. **Effort.** A pinned effort is passed on every launch, and a session that
   pinned none passes none: the CLI runs the model at its own default.
6. **System prompt.** A spawn briefs the agent with the system prompt.
7. **MCP.** Every launch points the CLI at `ariadne mcp serve`, with nothing
   after `serve` and the session context in the server's environment.
8. **Hooks.** Every launch tells the CLI to report what it does to
   `ariadne agent-event --kind <agent kind>`.
9. **Flags.** The flags of the agent config reach the argv once, and the
   adapter adds none of its own. The permission bypasses are **configuration**
   — `--dangerously-skip-permissions`,
   `--dangerously-bypass-approvals-and-sandbox`, and `--auto` — read from the
   per-agent config on every launch, so an agent whose flags the user emptied
   is launched with no bypass at all.
10. **Resume.** A resume names the session it continues and delivers its
    instruction once, through the one channel that CLI takes it on. An empty
    instruction is an interactive resume: it delivers nothing, and it puts
    nothing of the adapter's own in its place, so the agent drops into its TUI
    and waits for the user.
11. **Session id.** A spawn knows the CLI's own session id up front only where
    the CLI lets the caller choose it; every other adapter waits for the event
    that carries it. Either way the id is tracked, so the session can be
    resumed and attached.
12. **Skills.** The skill documents the launcher wrote reach the CLI the way
    that CLI takes a folder of them, and an agent that loads none is pointed
    at nothing.
13. **Trust dialogs.** A spawn types nothing into the pane. A CLI that opens a
    directory-trust dialog over the folder it was launched in has it answered
    by the user, never by the daemon.
14. **Compaction.** The adapter knows the event with which its CLI says a
    compaction is over, and reads no other event as one.

## The spellings

One column per CLI, one row per clause that has a spelling. This table is the
declaration each adapter returns from `contract()`.

| Clause | Claude Code | Codex | OpenCode |
| --- | --- | --- | --- |
| Binary | `claude` | `codex` | `opencode` |
| Generated files | `system-prompt.md`, `mcp.json`, `settings.json` | none — everything is on the argv | `opencode.json`, passed as `OPENCODE_CONFIG` |
| System prompt | `--append-system-prompt <content>` | prepended to the first message — no append-safe flag | `agent.ariadne.prompt` |
| Model | `--model` | `-m` | `agent.ariadne.model` |
| Effort | `--effort`, after the model | `-c model_reasoning_effort=<level>` | `agent.ariadne.variant` |
| MCP | `--mcp-config <run>/mcp.json`: `command`, `args`, `env` | `-c mcp_servers.ariadne.command`, `.args`, `.env` | `mcp.ariadne`: `command`, which heads the arguments, and `environment` |
| Hooks | command hooks in `settings.json` | `-c hooks.<Event>=[...]` ([`ariadne_core::codex_hooks`]) | the events plugin the daemon installs, named in `plugin` |
| Session id | chosen by Ariadne, `--session-id <uuid>` | reported by the `SessionStart` hook | reported by the plugin's `session.created` event |
| Resume | `--resume <id>` | `codex resume <id>`, every config flag re-passed | `--session <id>` |
| Instruction | last argument of the argv | last argument of the argv | typed into the TUI: OpenCode drops `--prompt` on a resume |
| Skills | a session plugin at `<run>/plugin`, passed with `--plugin-dir` | none — the index in the system prompt | `skills.paths` |
| Compaction | `session_start` whose `source` is `compact` | `post_compact` | `session.compacted` |

## Behavior

1. Every session names its agent CLI and its model: both come off the pin its
   seat carries, and there is no auto — nothing detects an installed CLI, and
   no launch falls back to a CLI default model.
2. Per-agent flags are replaced whole when they are edited, and an unknown
   agent kind is refused by name.
3. A resume replays the whole transcript as its first prompt. Shortening it is
   the agent's own business: the daemon asks no session to compact, and reads
   the compaction a CLI reports (clause 14) as the agent being back at its
   prompt.
4. A session with no internal agent id cannot be revived and is spawned
   afresh; a session of a finished goal is not revived at all.
5. A launch hands tmux nothing that can outgrow a command line: the prompt
   goes through a plan file rather than argv.
6. The skill documents of the agent are written into its run directory before
   the adapter plans anything, one `<name>/SKILL.md` under `skills/`. They go
   there and never into the worktree. A skill the agent no longer holds is
   removed by the same write.
7. Codex is given no skill folder because it discovers skills only under its
   own home or under the project root, and the project root of an agent is its
   worktree. The index in the system prompt names every document by its
   run-directory path (006), which is the floor under all three: a CLI with no
   skill loading of its own still has a file the agent can open.
8. A freshly launched pane is watched for a trust dialog, and the session says
   it is waiting on a person. The dialog stands until they answer it — typing
   into the pane is what takes the flag down (008), and an agent waiting on a
   person is neither nudged nor relaunched (009). Nothing is pressed on the
   daemon's own account: which answer such a dialog highlights belongs to the
   CLI and moves between its releases, and one of them closes the agent.

## Acceptance criteria

Every clause of the contract is proven for all three adapters at once, by
`adapter_contract.rs`:

- Clause 1 (`::every_launch_runs_the_binary_of_its_agent_kind`).
- Clause 2 (`::every_launch_carries_the_session_context_in_its_environment`).
- Clause 3
  (`::every_launch_generates_its_files_in_the_run_dir_and_none_in_the_worktree`).
- Clause 4 (`::every_launch_passes_the_pinned_model`).
- Clause 5 (`::an_effort_reaches_the_cli_only_when_the_session_pinned_one`).
- Clause 6 (`::every_spawn_briefs_the_agent_with_the_system_prompt`).
- Clause 7 (`::every_launch_points_the_cli_at_the_ariadne_mcp_server`).
- Clause 8 (`::every_launch_reports_the_cli_events_to_the_daemon`).
- Clause 9
  (`::the_configured_flags_reach_every_launch_once_and_the_adapter_adds_none`).
- Clause 10 (`::a_resume_names_its_session_and_delivers_its_instruction_once`,
  `::an_interactive_resume_delivers_no_instruction`).
- Clause 11 (`::a_spawn_knows_its_session_id_only_where_the_cli_lets_it_be_chosen`).
- Clause 12 (`::the_skill_documents_reach_the_cli_the_way_it_takes_them`).
- Clause 13 (`::a_spawn_types_nothing_into_the_pane`).
- Clause 14 (`::each_cli_says_a_compaction_is_over_in_the_event_the_contract_names`).

The spelling of each CLI is asserted whole beside it:

- Each adapter's spawn plan is asserted whole
  (`adapters.rs::claude_spawn_plan`, `::codex_spawn_plan`, `::opencode_spawn_plan`).
- The adapters hardcode no bypass flag
  (`adapters.rs::the_adapters_hardcode_no_bypass_flag`) and pass the configured
  flags once (`::the_configured_flags_are_passed_once`).
- Each spawn plan carries its model, and the effort reaches each CLI the way
  that CLI spells it
  (`adapters.rs::claude_passes_the_effort_after_the_model`,
  `::codex_passes_the_effort_as_a_config_override`,
  `::opencode_writes_the_effort_as_the_agents_variant`).
- The base environment carries the session context
  (`adapters.rs::base_env_carries_session_context`).
- Every agent kind is listed with its flags and defaults
  (`agents.rs::every_agent_kind_is_listed_with_its_flags_and_its_defaults`),
  flags are replaced whole (`::flags_are_replaced_whole_and_the_defaults_stay_readable`),
  an unknown kind is refused (`::an_unknown_agent_kind_is_refused_by_name`), and a
  launch takes its flags from the config (`::a_launch_takes_its_flags_from_the_agent_config`).
- A launch hands tmux nothing that can outgrow it
  (`resume.rs::a_launch_hands_tmux_nothing_that_can_outgrow_it`).
- Claude Code gets the skills as a session plugin
  (`adapters.rs::claude_loads_the_skills_as_a_session_plugin`), and an agent
  with none gets no plugin
  (`::claude_passes_no_plugin_for_an_agent_with_no_skills`).
- OpenCode looks for them in the run directory
  (`adapters.rs::opencode_points_its_skill_paths_at_the_run_dir`).
- A skill the agent dropped is gone from the run directory
  (`adapters.rs::a_dropped_skill_leaves_nothing_behind`).
- A session without an agent id is not revived
  (`resume.rs::a_session_without_an_agent_id_is_not_revived`), nor is one of a
  finished goal (`::a_session_of_a_finished_goal_is_not_revived`).
- Each CLI's own compaction vocabulary is read and nothing else is mistaken
  for it (`agents/mod.rs::a_compaction_is_done_when_the_cli_says_so_and_not_before`).
- A trust dialog is recognised on a pane through the colours it is drawn in
  (`launcher.rs::a_trust_dialog_is_recognised_on_a_pane`,
  `::a_pane_reads_as_what_is_on_the_screen`), and an agent at work is not
  mistaken for one (`::a_working_pane_is_not_a_question`).

## Sources

`crates/ariadne-daemon/src/agents/contract.rs` (the contract),
`crates/ariadne-daemon/src/agents/` (one module per CLI),
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/src/agents.rs`.
