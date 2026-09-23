---
id: agent-cli-adapters
status: current
updated: 2026-09-23
areas: [daemon, core]
commits: [ed1c40d3, 03fbf02d, 090c5158, e94647fd, a69b953f, 03f9c8b7]
tests:
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-daemon/tests/it/agents.rs
  - crates/ariadne-daemon/tests/it/acp_discovery.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/resume.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
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

1. The registry holds the agents of the ACP registry index that the daemon's
   `PATH` holds, and every `[[acp_agents]]` entry of the daemon config, each
   an `id` and a `command`. `GET /v1/acp-agents` lists them all, the
   discovered ones first, each with the `source` it came from — `registry` or
   `config`. Ariadne installs nothing: the index is vendored as a snapshot
   (`crates/ariadne-daemon/acp-registry/`), and what exists here is whatever
   `PATH` answers for.

   An agent of the index takes the id the index gives it, and the command it
   is looked up and started by comes from its distribution. A `binary`
   distribution gives one name: the basename of the `cmd` of this platform's
   build (`darwin-aarch64`, `darwin-x86_64`, `linux-aarch64` or
   `linux-x86_64`), with its arguments behind it — `./goose` with `["acp"]`
   is `goose acp`. An `npx` or `uvx` distribution gives two, tried in order:
   the entry id, then the package without its `@scope/` and without its
   version — `@google/gemini-cli` is `gemini` first and `gemini-cli` after
   it. The first name the `PATH` holds as an executable is the agent's
   command, and an entry that no name of it finds is registered as nothing at
   all. Only the absolute entries of the `PATH` are searched, in their own
   order: an empty entry — and an empty `PATH` is one — or a relative one
   names the directory the daemon was started in, which is not where its
   agents come from. Such an entry is dropped, and the entries behind it are
   searched as they always were.
2. A registry id is any non-empty word without `:`, the delimiter that splits
   a pin into its agent and its model. A configured entry whose id an agent of
   the index already holds replaces that agent, where it stands: the
   configured command is the one that is probed and launched. Between
   configured entries the first holder of an id keeps it, in configuration
   order, and between index entries the first holder keeps it too. An entry
   with a taken id of its own kind, an empty id, or an id with `:` is listed
   as rejected with the reason on it, is never probed, and never answers for
   its id — an index entry as much as a configured one.
3. Discovery probes every entry at once: it starts the command — for a
   discovered agent, the very file the `PATH` search found — runs
   `initialize`, and caches what it measured. An agent's catalog — its
   models and efforts — comes off a `session/new`, and the store keeps it
   under the command and the version the agent reported in `initialize`
   (`agentInfo.version`). A daemon start opens a session only on an agent
   whose command and version have no kept catalog. An agent that reports no
   version is read on every start.

   Startup downloads nothing. It takes the stored index only when its fetch
   time is after the shipped snapshot's date at midnight UTC. Otherwise,
   it takes the snapshot. `POST /v1/acp-agents/refresh` downloads the index
   from `acp_registry_url`, then searches `PATH` again and reads every
   catalog again. The default URL is
   `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`.
   `Timeouts::registry_download` bounds the download to 30 seconds.
   The store keeps one row: the URL, accepted document, and fetch time.
   Each good download replaces it. A failed download or refused document
   logs a warning and keeps the last good copy. Refresh still searches
   `PATH`, probes every entry, and returns the agent list.

   Discovery replaces the cache as one snapshot. The session a read
   opens is closed (`session/close`) where the agent advertises it. No
   prompt is ever sent, since a prompt is a model turn the agent bills. A
   probe has five seconds; one that runs out is rejected with every
   capability it measured before it did.
4. An agent is `ready` only when it negotiates ACP version 1, opens a
   session — now, or at the version its kept catalog was read from — and
   offers a model option with at least one choice. Anything short of that is
   `rejected`, with the reason on the entry.
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
   Every `codex-acp` process also carries `INITIAL_AGENT_MODE` set to
   `agent-full-access`, so its sessions never hand an approval to Codex's
   guardian sub-agent. The setting applies to that process only and does not
   edit the user's Codex configuration.
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
17. The daemon asks no agent to compact, and does not listen for one. An
    agent that compacts near its context limit carries on afterwards, and
    what it does next moves its session as any other work does.

## Acceptance criteria

- Refresh downloads the configured index and registers its installed agent
  (`acp_discovery.rs::refresh_downloads_the_configured_index_and_registers_its_installed_agent`).
- A newer stored index survives a restart without a download
  (`::a_newer_kept_index_survives_a_restart_without_a_download`). An older
  index or one fetched at the snapshot date leaves the snapshot in use
  (`::a_kept_index_no_newer_than_the_snapshot_does_not_replace_it`).
- Failed downloads and refused documents keep the index, log a warning,
  and still search `PATH` and probe agents
  (`::a_failed_download_keeps_the_index_and_still_rescans_and_reprobes`,
  `::a_refused_document_keeps_the_index_and_still_rescans_and_reprobes`).
- Each good download replaces the single stored row
  (`::each_good_download_replaces_the_single_kept_index`,
  `store.rs::one_acp_index_survives_reopen_and_each_download_replaces_it`).
- A stalled response body times out without replacing the kept index
  (`acp_discovery.rs::a_stalled_index_body_times_out_and_keeps_the_last_good_copy`).

- The registry lists an installed agent of the index, under the command the
  index gives it, beside a configured agent, each with its source
  (`acp_discovery.rs::the_api_lists_an_installed_index_agent_and_one_user_agent`).
- A `PATH` holding no agent of the index registers none
  (`acp_discovery.rs::an_empty_path_registers_no_agent`), a relative entry of
  a `PATH` is not searched
  (`::a_relative_path_entry_registers_no_agent`) and does not hide an
  absolute entry behind it
  (`::an_absolute_entry_behind_a_relative_one_is_still_searched`), a package
  entry is
  found under the name of its package where its id finds nothing
  (`::an_npx_agent_is_found_under_the_name_of_its_package`), and the index
  decides which name is tried first and what follows it
  (`::the_index_maps_an_entry_to_the_first_name_the_path_holds`).
- A configured entry replaces the discovered agent of its id, and a launch
  runs the configured command
  (`acp_discovery.rs::a_configured_agent_replaces_the_discovered_agent_of_its_id`).
- An id that spells a CLI name is an agent like any other
  (`acp_discovery.rs::a_registry_id_that_spells_a_cli_name_is_an_agent_like_any_other`),
  a taken id is rejected and its first holder keeps it
  (`::a_registry_id_already_taken_is_rejected`), and an id with `:` is
  rejected, configured
  (`::a_registry_id_with_the_catalog_delimiter_is_rejected`) or of the index
  (`::an_index_id_that_cannot_be_pinned_is_rejected`, which takes an empty
  index id too).
- Discovery refreshes on demand and replaces the cache
  (`acp_discovery.rs::discovery_refreshes_on_demand`), sends no prompt
  (`::discovery_sends_no_prompt`), and a probe that runs out its time keeps
  what it measured (`::a_timed_out_probe_keeps_what_it_measured`).
- A start reads a catalog once per agent version, closes the session it
  opened, and reads again on a new version or an explicit refresh
  (`acp_discovery.rs::a_catalog_is_read_once_per_agent_version`). An agent
  with no version is read on every start
  (`::an_agent_without_a_version_is_read_on_every_start`), and the store
  keeps one catalog per agent
  (`store.rs::an_acp_catalog_is_kept_per_agent_and_replaced_by_a_newer_read`).
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
- Every Codex launch starts in full-access mode, with no guardian review
  (`acp_runtime.rs::a_codex_launch_disables_guardian_approval`).
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
- The daemon advertises no compaction support on `initialize`
  (`acp.rs::initialize_tells_the_agent_what_the_daemon_supports`).

## Known gap

Discovery also runs once when the daemon starts. No test starts the daemon
binary, so the suite proves that probe only through the refresh endpoint.

## Sources

`crates/ariadne-daemon/src/acp_discovery.rs` (the registry and discovery),
`crates/ariadne-daemon/src/acp_index.rs` (the index and what it maps to),
`crates/ariadne-daemon/acp-registry/` (the vendored snapshot and its date),
`crates/ariadne-daemon/src/agents/` (the launch plan and the launch file),
`crates/ariadne-core/src/acp.rs` (the launch-file format),
`crates/ariadne-daemon/src/launcher.rs` (the launch, the flags, the resume
gate), `crates/ariadne-daemon/src/http/catalog.rs` (the agent endpoints),
`crates/ariadne-store/src/agents.rs` (the flags).
