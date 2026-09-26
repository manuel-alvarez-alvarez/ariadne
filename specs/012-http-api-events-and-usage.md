---
id: http-api-events-and-usage
status: current
updated: 2026-09-26
areas: [api, daemon]
commits: [d94042f4, 481a405d, 224370f4, a69b953f, 1b09ac10]
tests:
  - crates/ariadne-daemon/tests/it/events.rs
  - crates/ariadne-daemon/tests/it/session_list.rs
  - crates/ariadne-daemon/tests/it/unknown_fields.rs
  - crates/ariadne-daemon/tests/it/logs.rs
  - crates/ariadne-daemon/tests/it/doctor.rs
  - crates/ariadne-daemon/tests/it/ai_permissions.rs
  - crates/ariadne-daemon/tests/it/agents.rs
  - crates/ariadne-daemon/tests/it/models.rs
  - crates/ariadne-daemon/tests/it/acp_discovery.rs
  - crates/ariadne-daemon/src/http/classify.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/events.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_terminal.rs
  - crates/ariadne-daemon/tests/it/transcript_usage.rs
  - crates/ariadne-daemon/src/transcript.rs
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
   attention raised and cleared, and a task branch's head moving (002). An
   agent event on the domain stream is the exception to the fat event (rule
   4): a payload reaches 1 MB, and every client reads every frame, so its
   frame is an `AgentEventSummaryDto` — the id, the session id, the task id,
   the kind, the `summary` (rule 13) and the time — and carries no payload.
   The whole event, payload included, is what `GET /v1/events` answers and
   what the session's console stream carries (021).
8. A session is read and driven over HTTP through its row, its kill and
   resume, and its console (`/v1/sessions/{id}/console`, `/console/input`,
   `/console/stream`, `/console/cancel`, 008, 021). The one WebSocket,
   `/console/terminal` (008), serves that same console drawn as terminal
   bytes for an emulator, and reads the emulator's keys and size; its
   messages are DTOs of `ariadne-api` like every other. No endpoint types
   into, resizes or reads an agent's own terminal, and no endpoint is one an
   agent reports events to: the daemon's own ACP runtime is the one
   reporter, and it ingests in process.
   `GET /v1/sessions` is the one listing over both kinds of session (020):
   one cursor page of Ariadne's own sessions and of the conversations the
   ACP agents stored, as a `SessionPageDto`. There is no second listing
   endpoint for the outside ones.
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
   `/v1/events/stream` carries one. What an event moves lands in one order,
   with the status last: the agent's id, the attention and the activity clock
   are on the row before it says what the agent is doing, so a row a
   concurrent reader sees at some new status already carries everything that
   status implies. The status write itself is guarded against the row it
   touches, not the one this ingestion started on: a kill landing while it
   is still running must not have its retirement undone by a status this
   event decided before the kill (008), and a relaunch landing the same way
   must not have the new launch's `starting` moved by a status the old
   launch's own report decided before it (008).
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
    changes nothing but the usage it carries (rule 15) — the turn a relaunch
    cancelled reports what it spent only then (021) — except its
    `session_end`, which is not recorded: the session did not end, and a
    console closes on a `session_end` (008). An event that names no launch
    is believed.
13. Every agent event's DTO carries a `summary`: one line built from its
    payload when the DTO is built, never stored. A tool call reads as its
    action and its subject — `Bash: cargo nextest run` — off `tool_name` and
    the first field of `tool_input` it carries among `command`, `file_path`,
    `path`, `pattern`, `url`, `prompt` and `description`; a live
    `tool_call_update`, which carries only the call under `acp`, reads its
    `title` and `rawInput` the same way. Where the payload
    carries the agent's own words instead — the prompt that began the turn,
    its `text` alone where the event carries it beside the whole `prompt`
    (021), the message or thought a turn produced, whole or as one chunk on
    the console stream, a `session.error`'s message — those are shown
    verbatim.
    A `permission.replied` reads as who answered and, where the AI
    permission model had a part, why it did not decide (022, rule 31):
    `allowed by AI (0.83, threshold 0.70)`,
    `allow-once in the console — AI said escalate (0.41, threshold 0.70)`,
    `allow-once, learned`.
    A path under the payload's `cwd` is printed relative to it, and the cwd
    itself is never printed. The summary is
    flattened to one line and cut at 200 characters with a trailing `…`,
    which is also what a payload nothing here can read summarizes to.
14. An event may carry an `ariadne_usage`: the cumulative totals of one
    transcript, named by its `source`. A report with a missing, fractional or
    negative counter, or an empty source, is dropped, and its event still
    lands. The ACP runtime also writes a launch's totals straight to the
    store while a turn runs (rule 16), under the same source its `stop`
    uses.
15. Token usage is kept per session and per source, and a source replaces
    its own totals rather than adding to them. Usage rolls up to the task and
    the goal; every session of one reviewer groups together; a session that
    has reported nothing reads as zeros; and usage goes when its session
    does. A session also carries its latest context-window `used` and `size`
    pair from an ACP `usage_update`; both stay null when no update arrived,
    and a reported `cost` is neither stored nor exposed.
16. The ACP runtime takes a launch's usage from the transcript its agent
    writes, found by the session's ACP session id
    (`internal_session_id`), and names the running launch as the source:
    - A Codex rollout, `$CODEX_HOME` (else `~/.codex`)
      `/sessions/**/rollout-*<id>.jsonl`: the last `token_count` line's
      `payload.info.total_token_usage`. Its `input_tokens` already holds the
      cache reads (`cached_input_tokens`) and any cache writes, and its
      `output_tokens` the reasoning, so nothing is added to either. A resume
      starts the running total again in the same file, so a launch sums the
      last total of each segment it wrote.
    - A Claude Code transcript, `$CLAUDE_CONFIG_DIR` (else `~/.claude`)
      `/projects/<slug>/<id>.jsonl` and its subagents'
      `<slug>/<id>/subagents/agent-*.jsonl`, where the slug is the worktree's
      real path with every character but an ASCII letter or digit made a
      `-`: each `assistant` request once by `message.id`, input as
      `input_tokens` + `cache_read_input_tokens` +
      `cache_creation_input_tokens`, and cached input as the cache reads.
    A launch that resumes a conversation counts only what the file gains
    after the launch starts: the rest is an earlier launch's, under its own
    source. The transcript is read again every `Timeouts::transcript_poll`
    while a turn runs, and written to the store at once, and once more at
    the turn's end, onto its `stop` event.
    Where no transcript is found by the end of a turn, the launch falls back
    to the prompt response for the rest of its life: its well-formed
    `_meta.quota.token_count`, or its `usage` where quota is absent or
    malformed, as what that one turn spent (ACP). It adds cache reads and
    cache writes to input, records only the cache reads as cached input (a
    cache write is a token the model read for the first time, not a cache
    hit), adds the turn to the launch's earlier turns, and attaches the
    launch's totals to the turn's `stop` event. A launch never reports both.
    The cached share that the web and the CLI show is cached input over
    input, to one decimal place, and the two spell it alike
    (`ui/src/lib/format.test.ts`,
    `output.rs::the_cached_share_is_a_percent_to_one_decimal_between_zero_and_a_hundred`).
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
    session and a published task need (`git`, `gh`, `glab`), the Python
    interpreter the AI permission model installs into (022), and a worktree root it cannot
    write.
20. `GET /v1/acp-agents` serves the cached ACP registry. `POST
    /v1/acp-agents/refresh` downloads the configured registry index, searches
    `PATH` again, probes every entry, and replaces that cache. A download
    failure or refused document logs a warning and keeps the last good
    index. Refresh still searches `PATH`, probes agents, and answers the
    agent list. Both
    responses include status, measured capabilities, degradation flags, and
    a rejection reason when discovery failed.
21. `GET /v1/events` answers one page of the recorded events. `order=asc` is
    the default and answers the oldest of them, oldest first, which is what a
    client sweeping forward reads. `order=desc` answers the newest, newest
    first: a database that has recorded for months holds far more than one
    page, and the ascending page of it never moves.
22. Two cursors walk that listing: `after` takes the events above an id, and
    `before` the events below one. A descending page goes on from the id of
    its own last row.
23. The listing narrows by `session`, by `task` and by `goal`. A goal reaches
    an event through the goal of the session that reported it, or through the
    goal of the task it was on — an event carries a task and no session where
    nothing reported it, and is still its goal's.
24. An event goes with what it was reported against: a deleted session takes
    its events, a deleted task takes its own, and a deleted goal therefore
    leaves none of either. These rows are most of a database that has
    recorded for months, so a payload is stored packed, under a codec its own
    row names (`deflate`, or `none` where packing would not shrink it), and
    unpacked again in the store alone — every reader gets the JSON its
    reporter sent. The row is written and read back by one statement, which
    is all the lock that orders the ids covers: every live console chunk of
    every session waits behind that lock.
25. Three endpoints serve the AI permission model, the model the `ai`
    permission mode answers with (022): `GET /v1/permissions/ai` answers its
    settings, its prompts and the state of its install as an
    `AiPermissionsStatusDto`, `PUT /v1/permissions/ai` changes them, and
    `POST /v1/permissions/ai/refresh` runs the install again and answers 202.
    Every change of that status is the domain event `ai_permissions_updated`,
    carrying the whole DTO. It is the one event the pump does not fatten from a store row: the status is a row, an interpreter the
    daemon has just probed and where its server answers, so only the daemon
    can build it, and an install publishes one as readily as a write does.
    The event belongs to no goal or task, so a filtered stream carries none.

## Acceptance criteria

- An HTTP mutation emits a fat event
  (`events.rs::http_mutation_emits_a_fat_event`), a transition carries its
  cause (`::http_transition_emits_task_updated_with_its_transition`), a
  scheduler transition emits without HTTP
  (`::scheduler_transition_emits_task_updated_without_http`), and the
  launcher's session writes emit session events
  (`::launcher_session_writes_emit_session_events`).
- The stream opens with a heartbeat, filters, and resyncs a lagging client
  (`events.rs::sse_stream_opens_with_a_heartbeat`,
  `::sse_stream_frames_events_and_honours_its_filters`,
  `::sse_stream_signals_resync_and_closes_when_a_client_lags`).
- CORS allows preflight and cross-origin calls
  (`events.rs::cors_allows_preflight_and_cross_origin_calls`).
- A body with a field its DTO does not declare is refused, and the refusal
  names the field
  (`unknown_fields.rs::an_unknown_field_is_refused_and_named`).
- The three AI permission paths, their schemas and the
  `ai_permissions_updated` kind are in the OpenAPI document, the doctor's
  report carries the interpreter
  (`ai_permissions.rs::the_endpoints_the_schemas_and_the_event_are_in_the_openapi_document`,
  `::the_doctor_reports_the_interpreter_the_model_needs`), and a change of
  the status reaches the stream
  (`::turning_the_model_on_starts_the_install_and_reports_it_ready`,
  `::a_prompt_change_publishes_ai_permissions_updated`).
- The session listing's query and its page DTO are in the OpenAPI document,
  and the outside listing it replaced is not
  (`session_list.rs::the_query_and_the_page_are_in_the_openapi_document`).
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
  nothing, all but its `session_end`, which is not recorded
  (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`),
  and the usage it carries is kept
  (`acp_console.rs::a_relaunch_over_a_running_turn_keeps_what_the_old_launch_spent`).
- The live-only console events reach neither the events listing nor the
  domain stream
  (`acp_console.rs::the_events_listing_and_the_domain_stream_carry_no_chunk`).
- What an event moves lands with the status last
  (`events.rs::an_events_status_is_the_last_thing_it_moves`), and the status
  write is guarded against a retirement racing it
  (`store.rs::a_status_is_only_ever_written_while_the_session_is_still_live`)
  and against a relaunch racing it
  (`store.rs::a_status_is_only_written_for_the_launch_it_was_decided_for`).
- An agent event's `summary` reads a tool call as its action and its subject,
  the agent's own words where the payload carries any, a path relative to the
  cwd and never the cwd itself, one flattened line cut at 200 characters, and
  `…` where nothing of it can be read
  (`http/classify.rs::a_tool_call_reads_as_its_action_and_its_subject`,
  `::the_agents_own_words_are_shown_where_the_payload_carries_them`,
  `::a_payload_with_nothing_readable_but_its_cwd_summarizes_to_an_ellipsis`,
  `::a_path_under_the_cwd_prints_relative_and_the_cwd_never_appears`,
  `::a_long_summary_is_cut_at_200_characters`), an answered permission says
  who answered and why the model did not
  (`::an_answered_permission_says_who_answered_and_why_the_model_did_not`), and it reaches both
  `GET /v1/events` and the SSE stream
  (`events.rs::an_events_summary_reaches_the_snapshot_and_the_stream_alike`).
- A domain stream frame for an agent event carries no payload
  (`events.rs::a_domain_stream_frame_for_an_agent_event_carries_no_payload`),
  while `GET /v1/events` and the console stream carry the whole of it
  (`events.rs::the_events_listing_still_carries_the_whole_payload`,
  `::the_console_stream_still_carries_the_whole_payload`). `ariadne events`
  prints an agent event the same read off the listing as off the stream
  (`commands/events.rs::an_agent_event_reads_the_same_recorded_as_it_does_live`),
  and the desktop app, which refetches on the frame, still shows a session's
  activity (`events/dispatch.test.ts::agent events`).
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
- A context update changes the session DTO before its turn ends and carries no
  cost
  (`acp_console.rs::context_updates_keep_the_sessions_window_current_while_it_runs`).
- The ACP runtime maps standard and quota prompt usage, adds up one launch's
  turns, adds a resumed launch, leaves a silent response at zero, and rolls
  totals up (`acp_console.rs::standard_prompt_usage_adds_up_a_launchs_turns_and_rolls_up`,
  `::quota_prompt_usage_takes_precedence_over_standard_usage`,
  `::a_prompt_without_usage_keeps_zero_totals_and_records_stop`,
  `::resumed_prompt_usage_adds_a_new_launch_total`,
  `acp.rs::prompt_usage_maps_each_adapter_shape_and_prefers_quota`).
- A Codex session stores its rollout's `total_token_usage`, not its prompt
  response (`transcript_usage.rs::a_codex_session_stores_its_rollouts_total_not_its_prompt_response`);
  a Claude session counts each request once, its subagents' included
  (`::a_claude_session_counts_each_request_once_with_its_subagents`); a
  running turn's figure moves before its `stop`
  (`::a_running_turns_figure_moves_before_its_stop`); two launches of one
  session add up (`::two_launches_of_one_codex_session_add_up`); and a
  session with no transcript keeps its prompt response's figure
  (`::a_session_without_a_transcript_keeps_its_prompt_responses_figure`).
  A restart inside one launch adds its segments, a resumed launch counts
  only what it appends, a file that first appears after a resumed launch
  starts is all new, a resumed Claude launch counts only the requests it
  adds, a line still being written waits for its end, no
  file reads as no figure, and the Claude project is named after the real path
  (`transcript.rs::a_codex_restart_inside_one_launch_adds_its_segments`,
  `::a_resumed_launch_counts_only_what_it_appends`,
  `::a_rollout_that_first_appears_after_a_resumed_launch_starts_is_all_new`,
  `::a_resumed_claude_launch_counts_only_the_requests_it_adds`,
  `::a_line_still_being_written_waits_for_its_end`,
  `::no_transcript_reads_as_none`,
  `::a_claude_project_is_named_after_the_real_path`).
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
  `acp_terminal.rs::the_terminal_endpoint_is_in_the_openapi_document`,
  `doctor.rs::endpoint_is_in_the_openapi_document`,
  `models.rs::endpoint_is_in_the_openapi_document_with_nothing_to_filter_by`).
- A `config.toml` that names an unknown key stops the daemon
  (`config.rs::an_unknown_key_stops_the_daemon`).
- ACP registry endpoints expose the cached result and refresh it on demand
  (`acp_discovery.rs::the_api_lists_an_installed_index_agent_and_one_user_agent`,
  `::discovery_refreshes_on_demand`). Refresh downloads the configured index
  (`::refresh_downloads_the_configured_index_and_registers_its_installed_agent`)
  and still returns the agent list after failed downloads or refused documents
  (`::a_failed_download_keeps_the_index_and_still_rescans_and_reprobes`,
  `::a_refused_document_keeps_the_index_and_still_rescans_and_reprobes`).
- A descending page is the events recorded last, newest first
  (`events.rs::the_newest_page_is_the_events_recorded_last`,
  `store.rs::a_descending_page_answers_the_newest_events_newest_first`), and a
  page that asks for no order is still the oldest, oldest first
  (`events.rs::a_page_with_no_order_is_the_oldest_events_oldest_first`).
- `before` walks a descending page further back
  (`events.rs::a_before_page_walks_back_from_the_newest_page`,
  `store.rs::a_before_cursor_pages_back_past_the_newest_page`).
- A goal's events are the ones its sessions and its tasks reported, an event
  with no session included
  (`events.rs::a_goals_events_are_what_its_sessions_and_its_tasks_reported`),
  and a deleted goal leaves none of them behind
  (`store.rs::deleting_a_goal_leaves_no_event_of_its_sessions_or_its_tasks`).
- A payload reads back word for word however long it is, a payload above a
  kilobyte is stored smaller than it reads, one too short to shrink is stored
  as it came, and the row is written and read back by one statement
  (`store.rs::a_hundred_kilobyte_payload_reads_back_word_for_word`,
  `::a_payload_above_a_kilobyte_is_stored_smaller_than_it_reads`,
  `events.rs::an_event_is_written_and_read_back_by_one_statement`,
  `::a_codec_this_build_does_not_know_is_a_decode_error`).

## Known gap

An agent with no transcript found falls back to its prompt responses. An
adapter that reports neither supported usage shape then leaves its session,
task and goal usage at zero, and that fallback is only as whole as the
adapter makes it: codex-acp answers a prompt with the usage of the turn's
last model request, not of the turn.

Claude Code shortens a project slug longer than 200 characters and adds a
hash; such a worktree's transcript is not found, and the launch falls back.
A Claude Code resume that writes a new file under a new id is not followed
either.

## Sources

`crates/ariadne-api/`, `crates/ariadne-daemon/src/http/`,
`crates/ariadne-daemon/src/http/classify.rs`,
`crates/ariadne-daemon/src/http/events.rs`,
`crates/ariadne-daemon/src/bus.rs`, `crates/ariadne-store/src/events.rs`,
`crates/ariadne-store/src/usage.rs`,
`crates/ariadne-daemon/src/transcript.rs`.
