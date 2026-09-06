---
id: agent-cli-adapters
status: current
updated: 2026-09-06
areas: [daemon, core]
commits: [ed1c40d3, 03fbf02d, 090c5158, e94647fd, a69b953f, 03f9c8b7]
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
effort are passed to each, the session context in the environment, how each
CLI is given the agent's skill documents, hook installation, and resuming or
reviving a session.

Out: which model to pick (011), what the session is briefed with (006), and
what a skill says (017).

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
   session, goal, seat and task — which is what the MCP server and the hook
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
11. The skill documents of the agent are written into its run directory before
    the adapter plans anything, one `<name>/SKILL.md` under `skills/`. They go
    there and never into the worktree, which belongs to the repository. A
    skill the agent no longer holds is removed by the same write.
12. Each adapter then points its CLI at that directory the way that CLI takes
    one:
    - Claude Code loads it as a plugin of this session alone — a manifest and
      a link to the directory, passed with `--plugin-dir`;
    - OpenCode names it in `skills.paths` of the session config;
    - Codex is given nothing, because it discovers skills only under its own
      home or under the project root, and the project root of an agent is its
      worktree.
13. The index in the system prompt names every document by its run-directory
    path (006), which is the floor under all three: a CLI with no skill
    loading of its own still has a file the agent can open.

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

## Sources

`crates/ariadne-daemon/src/agents/` (one module per CLI),
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/src/agents.rs`.
