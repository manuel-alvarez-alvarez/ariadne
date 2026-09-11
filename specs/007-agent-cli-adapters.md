---
id: agent-cli-adapters
status: current
updated: 2026-09-11
areas: [daemon, core]
commits: [ed1c40d3, 03fbf02d, 090c5158, e94647fd, a69b953f, 03f9c8b7]
tests:
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-daemon/tests/agents.rs
  - crates/ariadne-daemon/tests/acp_discovery.rs
  - crates/ariadne-daemon/tests/acp_runtime.rs
  - crates/ariadne-daemon/tests/resume.rs
  - crates/ariadne-daemon/tests/skill_documents.rs
  - crates/ariadne-daemon/src/agents/acp.rs
---

# Agent launch

How Ariadne launches an agent: which agents there are, what discovery learns
about each, and what one launch is made of — the command, the environment,
the launch file, and the resume.

Every agent is an ACP agent in the daemon's registry. A launch runs the
registry command of the agent the session's pin names, as a child of the
daemon, and the ACP runtime (021) drives it from the launch file this spec
describes.

## Scope

In: the agent registry, discovery and the capabilities it caches, the flags
each agent is launched with, the launch plan and its launch file, the session
context in the environment, the skill documents on disk, and resuming or
reviving a session.

Out: the process and the protocol conversation (021), the model catalog and
where a pin is set (011), what the session is briefed with (006), and what a
skill says (017).

## Behavior

1. The registry holds three built-in agents — `claude-code-acp`
   (`claude-code-acp`), `codex-acp` (`codex acp`) and `opencode-acp`
   (`opencode acp`) — and every `[[acp_agents]]` entry of the daemon config,
   each an `id` and a `command`. `GET /v1/acp-agents` lists them all, the
   built-ins first.
2. A registry id is any non-empty word without `:`, the delimiter that splits
   a pin into its agent and its model. The first holder of an id keeps it:
   built-ins first, then the configured entries in configuration order. A
   later entry with a taken id, an empty id, or an id with `:` is listed as
   rejected with the reason on it, is never probed, and never answers for its
   id.
3. Discovery probes every entry at once: it starts the command, runs
   `initialize`, `session/new` and one empty `session/prompt`, and caches
   what it measured. `POST /v1/acp-agents/refresh` runs it again and replaces
   the cache as one snapshot.
4. An agent is `ready` only when it negotiates ACP version 1, opens a
   session, offers a model option with at least one choice, and answers a
   prompt. Anything short of that is `rejected`, with the reason on the
   entry.
5. A ready agent that lacks an optional capability is degraded, one flag per
   gap: `no_efforts` for no effort option, `no_adoption` for no
   `session_list`, and `no_restart_resume` for no `session_load`.
6. Each registry agent has one list of extra flags. `GET /v1/agents` lists
   every registry agent with its flags and an empty `default_flags`.
   `PUT /v1/agents/{id}` replaces the list whole, an empty list included, and
   refuses an id the registry does not hold by name.
7. A launch runs the registry command of the agent the pin names, with that
   agent's flags behind it. The flags are read on every launch, spawn and
   resume alike, and the launch adds no flag of its own.
8. The pin is `<agent>:<model>` (011). The agent is told only the model half,
   and the effort only where the session pinned one. Both come off the
   session row, so no launch of a session moves either.
9. Every launch carries the session context in its environment —
   `ARIADNE_SESSION_ID`, `ARIADNE_LAUNCH_ID`, `ARIADNE_GOAL_ID`,
   `ARIADNE_SEAT`, `ARIADNE_SOCKET`, and `ARIADNE_TASK_ID` for a task seat —
   and runs in the seat's worktree, or the repository for the orchestrator.
10. The launch id is fresh for every process started under a session row,
    and the row is told it before the process starts. That is what tells the
    events of a replaced agent from those of the agent that replaces it.
11. Every launch writes `acp.json` into the session's run directory: the
    system prompt, the first prompt, the model, the effort, the session it
    resumes, and one MCP server, `ariadne` — the `ariadne` CLI with the
    arguments `mcp serve` and the session context as its environment. The
    runtime sends the agent exactly what this file says.
12. A spawn opens a new agent session and carries the briefing as the first
    prompt. A resume names the agent session it continues and carries its
    instruction once, as the next prompt; an empty instruction resumes an
    agent that is told nothing.
13. A briefing has no size limit on its way to the agent: it travels in the
    launch file and the protocol, never on a command line.
14. The skill documents of the agent are written into its run directory
    before the launch is planned, one `<name>/SKILL.md` under `skills/`, and
    never into the worktree. The index in the system prompt names each one by
    that path (006). A skill the agent no longer holds is removed by the same
    write.
15. A resume is gated on discovery: an agent whose cached capabilities lack
    `session_load` is refused as not resumable, and no process starts.
16. A session with no agent session id is not revived, and the next launch of
    its seat is a fresh spawn. A session of a finished goal is not revived at
    all. A revive brings back the very row it names, on the same agent and
    model.
17. The daemon asks no agent to compact. A completed `compaction_update`
    reads as the agent back at its prompt, and no other event does.

## Acceptance criteria

- The registry lists the three built-ins and a configured agent
  (`acp_discovery.rs::the_api_lists_the_three_known_agents_and_one_user_agent`).
- An id that spells a CLI name is an agent like any other
  (`acp_discovery.rs::a_registry_id_that_spells_a_cli_name_is_an_agent_like_any_other`),
  a taken id is rejected and its first holder keeps it
  (`::a_registry_id_already_taken_is_rejected`), and an id with `:` is
  rejected (`::a_registry_id_with_the_catalog_delimiter_is_rejected`).
- Discovery refreshes on demand and replaces the cache
  (`acp_discovery.rs::discovery_refreshes_on_demand`).
- Every required capability is enforced
  (`acp_discovery.rs::every_required_acp_capability_is_enforced`), and an agent
  with no model option is rejected with the reason shown
  (`::an_agent_without_a_model_option_is_rejected_and_doctor_shows_why`).
- Every optional gap sets its own degradation flag
  (`acp_discovery.rs::every_optional_capability_gap_sets_its_degraded_flag`).
- Every registry agent is listed with its flags
  (`agents.rs::every_registry_agent_is_listed_with_its_flags_and_its_defaults`),
  the flags are replaced whole
  (`::flags_are_replaced_whole_and_the_defaults_stay_readable`), and an
  unknown agent is refused by name (`::an_unknown_agent_is_refused_by_name`).
- A launch takes its flags from the agent config, on spawn and resume
  (`agents.rs::a_launch_takes_its_flags_from_the_agent_config`), and the
  plan adds none of its own
  (`adapters.rs::the_configured_flags_are_passed_once_and_the_adapter_adds_none`).
- A seat runs the registry command its pin names, with the bare model half
  pinned (`acp_runtime.rs::an_orchestrator_runs_on_the_registry_agent`).
- The model and effort of a session never move across its launches
  (`resume.rs::a_running_reviewer_keeps_the_model_its_session_started_on`,
  `::a_resumed_author_stays_on_the_model_its_session_started_on`,
  `::an_orchestrator_respawn_stays_on_the_goals_pin`).
- Every launch carries the session context, in the agent's environment and
  the MCP server's (`adapters.rs::every_launch_carries_the_session_context`).
- Every launch of a session reports under a new launch id
  (`resume.rs::every_launch_of_a_session_reports_under_a_new_id`).
- A spawn writes the launch file with the briefing, the pins and the
  `ariadne` MCP server, and the plan carries what the file says
  (`adapters.rs::a_spawn_plans_a_new_session_briefed_and_pinned`).
- A resume names its session and delivers its instruction once, and an empty
  one delivers nothing
  (`adapters.rs::a_resume_names_its_session_and_delivers_its_instruction_once`).
- A briefing of any size reaches the agent whole
  (`resume.rs::a_briefing_of_any_size_reaches_the_agent_whole`).
- The skill documents are on disk where the index names them
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`),
  and a dropped skill leaves nothing behind
  (`adapters.rs::a_dropped_skill_leaves_nothing_behind`).
- A session of an agent without `session_load` is refused as not resumable
  (`acp_runtime.rs::a_session_of_an_agent_without_session_load_is_not_resumable`).
- A session without an agent session id is not revived
  (`resume.rs::a_session_without_an_agent_id_is_not_revived`), a reviewer
  without one is spawned afresh
  (`::a_reviewer_without_an_agent_id_is_spawned_afresh`), a session of a
  finished goal is not revived
  (`::a_session_of_a_finished_goal_is_not_revived`), and a revive brings back
  the row it names (`::reviving_a_session_revives_it_in_place`).
- Only a completed `compaction_update` reads as a finished compaction
  (`agents/acp.rs::a_compaction_is_done_when_the_agent_says_so_and_not_before`).

## Known gap

Discovery also runs once when the daemon starts. No test starts the daemon
binary, so the suite proves that probe only through the refresh endpoint.

## Sources

`crates/ariadne-daemon/src/acp_discovery.rs` (the registry and discovery),
`crates/ariadne-daemon/src/agents/` (the launch plan and the launch file),
`crates/ariadne-core/src/acp.rs` (the launch-file format),
`crates/ariadne-daemon/src/launcher.rs` (the launch, the flags, the resume
gate), `crates/ariadne-daemon/src/http/catalog.rs` (the agent endpoints),
`crates/ariadne-store/src/agents.rs` (the flags).
