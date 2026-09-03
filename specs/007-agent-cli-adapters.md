---
id: agent-cli-adapters
status: current
updated: 2026-09-04
areas: [daemon, core]
commits: [ed1c40d3, 03fbf02d, 090c5158, e94647fd]
tests:
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-daemon/tests/agents.rs
  - crates/ariadne-daemon/tests/resume.rs
---

# Agent CLI adapters

How Ariadne launches a concrete coding-agent CLI: the argv, the environment,
the generated config, and the hooks that report back.

## Scope

In: the three supported CLIs, per-agent launch flags, how a model and an
effort are passed to each, the session context in the environment, hook
installation, and resuming or reviving a session.

Out: which model to pick (011), and what the session is briefed with (006).

## Behavior

1. Three agent CLIs are supported: **Claude Code**, **OpenAI Codex CLI** and
   **OpenCode**. An adapter turns a spawn or resume request into argv, env and
   generated config files for one of them.
2. Where no agent is pinned, the first installed CLI is used, in the order
   `claude_code`, `codex`, `opencode`.
3. Permissions are bypassed per CLI — `--dangerously-skip-permissions`,
   `--dangerously-bypass-approvals-and-sandbox`, and `--auto` plus an
   allow-everything permission block. Those flags are **configuration**, read
   from the per-agent config on every launch, not constants in the adapters.
4. Per-agent flags are replaced whole when they are edited, and an unknown
   agent kind is refused by name.
5. A model is spelled `<agent>[:<model>]`; the agent alone runs that CLI's own
   default model. The effort is passed the way each CLI spells it: after the
   model for Claude, as a config override for Codex, and as the agent's
   variant for OpenCode.
6. Every session is launched with its Ariadne identity in the environment —
   session, goal, role and task — which is what the MCP server and the hook
   sink read to act as that session.
7. Hooks installed at spawn time report every session and tool event back to
   the daemon, and each CLI's internal session id is tracked so a session can
   be resumed and attached.
8. A resume replays the whole transcript as its first prompt, which is why the
   daemon compacts at every hand-off (010).
9. A session with no internal agent id cannot be revived and is spawned
   afresh; a session of a finished goal is not revived at all.
10. A launch hands tmux nothing that can outgrow a command line: the prompt
    goes through a plan file rather than argv.

## Acceptance criteria

- Each adapter's spawn plan is asserted whole
  (`adapters.rs::claude_spawn_plan`, `::codex_spawn_plan`, `::opencode_spawn_plan`).
- The adapters hardcode no bypass flag
  (`adapters.rs::the_adapters_hardcode_no_bypass_flag`) and pass the configured
  flags once (`::the_configured_flags_are_passed_once`).
- The effort reaches each CLI the way that CLI spells it
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
- A session without an agent id is not revived
  (`resume.rs::a_session_without_an_agent_id_is_not_revived`), nor is one of a
  finished goal (`::a_session_of_a_finished_goal_is_not_revived`).

## Sources

`crates/ariadne-daemon/src/agents/` (one module per CLI),
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/src/agents.rs`.
