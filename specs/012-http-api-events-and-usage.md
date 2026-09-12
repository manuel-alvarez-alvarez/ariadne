---
id: http-api-events-and-usage
status: current
updated: 2026-09-12
areas: [api, daemon]
commits: [d94042f4, 481a405d, 224370f4, a69b953f, 1b09ac10]
tests:
  - crates/ariadne-daemon/tests/events.rs
  - crates/ariadne-daemon/tests/unknown_fields.rs
  - crates/ariadne-daemon/tests/logs.rs
  - crates/ariadne-daemon/tests/doctor.rs
  - crates/ariadne-daemon/tests/agents.rs
  - crates/ariadne-daemon/tests/models.rs
  - crates/ariadne-daemon/tests/acp_discovery.rs
  - crates/ariadne-daemon/src/http/classify.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/memories.rs
  - crates/ariadne-daemon/tests/acp_console.rs
---

# HTTP API, event stream and usage

The daemon's surface: what it listens on, the shape of a reply, the stream
clients follow, the agent events it records, and the token accounting behind
every row.

## Scope

In: the transports, the DTO and error envelope, OpenAPI, the SSE stream and
its guarantees, how an agent event is ingested and summarized, the agent flag
and daemon log endpoints, and how token usage is rolled up.

Out: the CLI that consumes this (014), the desktop app that consumes it (015),
and the ACP runtime that reports the agent events (021).

## Behavior

1. The daemon listens on a unix socket (`~/.ariadne/ariadne.sock`) and,
   optionally, on a TCP address for the desktop app.
2. DTOs and the error shape live in one crate (`ariadne-api`) and are the
   single source of truth for the OpenAPI document every client generates
   from. Every endpoint appears in that document.
3. A refusal is an envelope with a machine-readable code and one sentence a
   person can act on — the state machine's own explanation where a transition
   was refused (001). Every request DTO denies unknown fields, so a body that
   carries a field its DTO does not declare is refused in that same envelope,
   and the refusal names the field. A response DTO denies nothing.
4. Every write emits a **fat event**: the changed entity, whole, so a client
   can apply it without a re-fetch. A task transition carries the transition
   that caused it, whether it came through HTTP or from the scheduler.
5. The stream is SSE. It opens with a heartbeat, honours per-client filters,
   and signals a resync and closes when a client lags rather than silently
   dropping events.
6. CORS allows the preflight and cross-origin calls the desktop app makes.
7. Session events reach the stream too: launches, ingested agent events,
   attention raised and cleared, and a task branch's head moving (002).
8. A session is read and driven over HTTP through its row, its kill and
   resume, and its console (`/v1/sessions/{id}/console`, `/console/input`,
   `/console/stream`, `/console/cancel`, 008, 021). There is no endpoint
   that types into, resizes or reads a terminal, and no endpoint an agent
   reports events to: the daemon's own ACP runtime is the one reporter, and
   it ingests in process.
9. An ingested event is recorded whole, then read in the runtime's one
   vocabulary. `session_start`, `user_prompt_submit`, `pre_tool_use`,
   `post_tool_use` and `permission.replied` mark the session running; `stop`
   marks it idle; `session_end` marks it exited. `permission_request` raises
   `waiting_permission`, and `session.error` raises `agent_error`; neither
   reads as liveness. An event that matches nothing — `agent_thought`,
   `agent_message`, `plan` — is recorded and moves nothing. The live-only
   console events (021) — `agent_message_chunk`, `agent_thought_chunk`,
   `tool_call_update` — are never ingested: they reach the session's
   console stream alone, and neither `GET /v1/events` nor
   `/v1/events/stream` carries one.
10. An idle report clears the stall and the error and nothing else, so a
    permission request survives it. A permission reply hands control back to
    the agent and takes the wait down.
11. Attention is raised only on an agent somebody is still waiting on: a
    reviewer that already cast its verdict, and an orchestrator of a finished
    goal, raise none. Their events are recorded, and the status still follows
    them.
12. An event is believed only where it comes from the launch the session is
    on (008): a relaunched agent shares its session id — and, on a resumed
    conversation, its internal id — with the process it replaced, whose exit
    is still to report. A report from the launch before is recorded and
    changes nothing. An event that names no launch is believed.
13. Every agent event's DTO carries a `summary`: one line built from its
    payload when the DTO is built, never stored. A tool call reads as its
    action and its subject — `Bash: cargo nextest run` — off `tool_name` and
    the first field of `tool_input` it carries among `command`, `file_path`,
    `path`, `pattern`, `url`, `prompt` and `description`; a live
    `tool_call_update`, which carries only the call under `acp`, reads its
    `title` and `rawInput` the same way. Where the payload
    carries the agent's own words instead — the prompt that began the turn,
    the message or thought a turn produced, whole or as one chunk on the
    console stream, a `session.error`'s message — those are shown verbatim.
    A path under the payload's `cwd` is printed relative to it, and the cwd
    itself is never printed. The summary is
    flattened to one line and cut at 200 characters with a trailing `…`,
    which is also what a payload nothing here can read summarizes to.
14. An event may carry an `ariadne_usage`: the cumulative totals of one
    transcript, named by its `source`. A report with a missing, fractional or
    negative counter, or an empty source, is dropped, and its event still
    lands.
15. Token usage is kept per session and per source, and a source replaces
    its own totals rather than adding to them. Usage rolls up to the task and
    the goal; every session of one reviewer groups together; a session that
    has reported nothing reads as zeros; and usage goes when its session
    does.
16. The ACP runtime reads a prompt response's well-formed
    `_meta.quota.token_count`, or its `usage` where quota is absent or
    malformed. It adds cached reads and writes to input, records their sum as
    cached input, names the running launch as the source, and attaches the
    totals only to the turn's `stop` event.
17. `GET /v1/agents` lists every registry agent's flags, in registry order,
    as `AgentConfigDto{agent_id, extra_flags, default_flags}`. `PUT
    /v1/agents/{id}` replaces one agent's list whole, an empty one included,
    and refuses an agent the registry does not hold by name. A launch passes
    the flags after the agent's registry command.
18. The daemon's own log is served both as a snapshot (with a tail limit) and
    as a stream that opens with a snapshot and follows with deltas, from a
    ring buffer that evicts its oldest lines.
19. `doctor` reports the environment the daemon actually runs in: its own
    paths, every registry agent with what discovery made of it, the tools a
    session and a published task need (`git`, `gh`, `glab`), and a worktree
    root it cannot write.
20. `GET /v1/acp-agents` serves the cached ACP registry. `POST
    /v1/acp-agents/refresh` probes every entry and replaces that cache. Both
    responses include status, measured capabilities, degradation flags, and
    a rejection reason when discovery failed.

## Acceptance criteria

- An HTTP mutation emits a fat event
  (`events.rs::http_mutation_emits_a_fat_event`), a transition carries its
  cause (`::http_transition_emits_task_updated_with_its_transition`), a
  scheduler transition emits without HTTP
  (`::scheduler_transition_emits_task_updated_without_http`), and the
  launcher's session writes emit session events
  (`::launcher_session_writes_emit_session_events`).
- Memory creation emits its complete entry, and deletion emits its id
  (`memories.rs::delete_removes_a_memory`).
- The stream opens with a heartbeat, filters, and resyncs a lagging client
  (`events.rs::sse_stream_opens_with_a_heartbeat`,
  `::sse_stream_frames_events_and_honours_its_filters`,
  `::sse_stream_signals_resync_and_closes_when_a_client_lags`).
- CORS allows preflight and cross-origin calls
  (`events.rs::cors_allows_preflight_and_cross_origin_calls`).
- A body with a field its DTO does not declare is refused, and the refusal
  names the field
  (`unknown_fields.rs::an_unknown_field_is_refused_and_named`).
- Every event the runtime reports is acted on, a lifecycle event moves the
  status and raises nothing, and a wait on the user is raised and never
  reads as liveness
  (`http/classify.rs::every_event_the_runtime_reports_is_acted_on`,
  `::a_lifecycle_event_moves_the_status_and_raises_nothing`,
  `::a_wait_on_the_user_is_raised_and_never_reads_as_liveness`,
  `::an_answered_ask_hands_control_back_to_the_agent`), and an event's
  internal id is its session's
  (`::an_events_internal_id_is_its_sessions_and_nothing_elses`).
- Ingested events raise and clear session attention
  (`events.rs::ingested_events_raise_and_clear_session_attention`,
  `::an_idle_report_clears_the_stall_and_the_error_and_nothing_else`,
  `::a_permission_request_flags_the_session_as_blocked`), and not on an agent
  nobody waits on (`::a_reviewer_that_already_voted_raises_no_attention`,
  `::an_orchestrator_of_a_finished_goal_raises_no_attention`).
- An event from a launch the session has moved past is recorded and changes
  nothing
  (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
- The live-only console events reach neither the events listing nor the
  domain stream
  (`acp_console.rs::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- An agent event's `summary` reads a tool call as its action and its subject,
  the agent's own words where the payload carries any, a path relative to the
  cwd and never the cwd itself, one flattened line cut at 200 characters, and
  `…` where nothing of it can be read
  (`http/classify.rs::a_tool_call_reads_as_its_action_and_its_subject`,
  `::the_agents_own_words_are_shown_where_the_payload_carries_them`,
  `::a_payload_with_nothing_readable_but_its_cwd_summarizes_to_an_ellipsis`,
  `::a_path_under_the_cwd_prints_relative_and_the_cwd_never_appears`,
  `::a_long_summary_is_cut_at_200_characters`), and it reaches both
  `GET /v1/events` and the SSE stream
  (`events.rs::an_events_summary_reaches_the_snapshot_and_the_stream_alike`).
- A usage report is read whole, and a malformed one is dropped while its
  event still lands
  (`http/classify.rs::an_event_reports_the_totals_of_the_transcript_it_names`,
  `::an_absent_or_malformed_report_is_no_news`,
  `events.rs::a_malformed_report_is_dropped_and_its_event_still_lands`).
- Usage rolls up to the task and the goal
  (`events.rs::reported_usage_rolls_up_to_the_task_and_the_goal`,
  `store.rs::a_tasks_usage_groups_every_round_of_a_reviewer_together`,
  `::a_goals_usage_is_grouped_by_seat_and_counts_its_orchestrator`), a source
  replaces its own totals
  (`store.rs::a_source_replaces_its_own_totals_and_sources_add_up`), a
  session that reported nothing reads as zeros
  (`events.rs::a_session_that_has_reported_nothing_reads_as_zeros`), and
  usage goes with its session
  (`store.rs::usage_goes_when_the_session_it_belonged_to_does`).
- The ACP runtime maps standard and quota prompt usage, replaces one launch's
  totals, adds a resumed launch, leaves a silent response at zero, and rolls
  totals up (`acp_console.rs::standard_prompt_usage_replaces_launch_totals_and_rolls_up`,
  `::quota_prompt_usage_takes_precedence_over_standard_usage`,
  `::a_prompt_without_usage_keeps_zero_totals_and_records_stop`,
  `::resumed_prompt_usage_adds_a_new_launch_total`,
  `acp.rs::prompt_usage_maps_each_adapter_shape_and_prefers_quota`).
- Every registry agent is listed with its flags
  (`agents.rs::every_registry_agent_is_listed_with_its_flags_and_its_defaults`),
  flags are replaced whole
  (`::flags_are_replaced_whole_and_the_defaults_stay_readable`), an unknown
  agent is refused by name (`::an_unknown_agent_is_refused_by_name`), and a
  launch takes its flags from the config
  (`::a_launch_takes_its_flags_from_the_agent_config`).
- The log snapshot, tail, eviction and stream behave
  (`logs.rs::the_snapshot_returns_captured_lines_in_order`,
  `::tail_limits_the_snapshot_to_the_last_n_lines`,
  `::the_ring_buffer_evicts_its_oldest_lines`,
  `::the_stream_opens_with_a_snapshot_then_follows_with_deltas`), and hostile
  content survives SSE framing (`::hostile_log_content_survives_sse_framing`).
- `doctor` reports every registry agent, the tools, its own paths, and an
  unwritable worktree root
  (`doctor.rs::every_registry_agent_is_reported_with_what_discovery_made_of_it`,
  `::the_tools_a_session_and_a_published_task_need_are_reported`,
  `::the_paths_are_this_daemons_own_and_are_left_as_they_were`,
  `::a_worktree_root_the_daemon_cannot_write_is_reported_as_such`).
- Every endpoint is in the OpenAPI document
  (`logs.rs::both_endpoints_are_in_the_openapi_document`,
  `acp_console.rs::the_cancel_endpoint_is_in_the_openapi_document`,
  `doctor.rs::endpoint_is_in_the_openapi_document`,
  `models.rs::endpoint_is_in_the_openapi_document_with_nothing_to_filter_by`).
- ACP registry endpoints expose the cached result and refresh it on demand
  (`acp_discovery.rs::the_api_lists_the_three_known_agents_and_one_user_agent`,
  `::discovery_refreshes_on_demand`).

## Known gap

ACP prompt responses now report token usage through the runtime's normal
ingestion path. An adapter that reports neither supported usage shape leaves
its session, task and goal usage at zero.

## Sources

`crates/ariadne-api/`, `crates/ariadne-daemon/src/http/`,
`crates/ariadne-daemon/src/http/classify.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/bus.rs`, `crates/ariadne-store/src/usage.rs`.
