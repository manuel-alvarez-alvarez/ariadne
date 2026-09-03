---
id: http-api-events-and-usage
status: current
updated: 2026-09-04
areas: [api, daemon]
commits: [d94042f4, 481a405d, 224370f4]
tests:
  - crates/ariadne-daemon/tests/events.rs
  - crates/ariadne-daemon/tests/unknown_fields.rs
  - crates/ariadne-daemon/tests/logs.rs
  - crates/ariadne-daemon/tests/doctor.rs
  - crates/ariadne-store/tests/store.rs
---

# HTTP API, event stream and usage

The daemon's surface: what it listens on, the shape of a reply, the stream
clients follow, and the token accounting behind every row.

## Scope

In: the transports, the DTO and error envelope, OpenAPI, the SSE stream and
its guarantees, the daemon log endpoints, and how token usage is reported and
rolled up.

Out: the CLI that consumes this (014) and the desktop app that consumes it
(015).

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
7. Session events reach the stream too: launches, ingested agent hook events,
   attention raised and cleared, compactions, and a task branch's head moving
   (002, 010).
8. Agent hook events are ingested per CLI and classified: a permission prompt
   or a notification flags the session as blocked, an idle report clears the
   stall and the error and nothing else, and a question is held until it is
   answered or the turn moves on. A malformed report is dropped and its event
   still lands.
9. Token usage is reported per session and per source, and a source replaces
   its own totals rather than adding to them. Usage rolls up to the task and
   the goal; every round of one reviewer groups together; a session that has
   reported nothing reads as zeros; and usage goes when its session does.
10. The daemon's own log is served both as a snapshot (with a tail limit) and
    as a stream that opens with a snapshot and follows with deltas, from a
    ring buffer that evicts its oldest lines.
11. `doctor` reports the environment the daemon actually runs in — its own
    paths, the agent CLIs and tools a session and a published task need, and a
    worktree root it cannot write.

## Acceptance criteria

- An HTTP mutation emits a fat event
  (`events.rs::http_mutation_emits_a_fat_event`), a transition carries its
  cause (`::http_transition_emits_task_updated_with_its_transition`), and a
  scheduler transition emits without HTTP
  (`::scheduler_transition_emits_task_updated_without_http`).
- The stream opens with a heartbeat, filters, and resyncs a lagging client
  (`events.rs::sse_stream_opens_with_a_heartbeat`,
  `::sse_stream_frames_events_and_honours_its_filters`,
  `::sse_stream_signals_resync_and_closes_when_a_client_lags`).
- CORS allows preflight and cross-origin calls
  (`events.rs::cors_allows_preflight_and_cross_origin_calls`).
- A body with a field its DTO does not declare is refused, and the refusal
  names the field
  (`unknown_fields.rs::an_unknown_field_is_refused_and_named`).
- Ingested events raise and clear session attention
  (`events.rs::ingested_events_raise_and_clear_session_attention`,
  `::an_idle_report_clears_the_stall_and_the_error_and_nothing_else`,
  `::an_opencode_permission_ask_flags_the_session_as_blocked`,
  `::a_claude_notification_flags_the_session_as_blocked`).
- A malformed usage report is dropped and its event still lands
  (`events.rs::a_malformed_report_is_dropped_and_its_event_still_lands`).
- Usage rolls up to the task and the goal
  (`events.rs::reported_usage_rolls_up_to_the_task_and_the_goal`,
  `store.rs::a_tasks_usage_groups_every_round_of_a_reviewer_together`,
  `::a_goals_usage_is_grouped_by_role_and_counts_its_planner`), a source
  replaces its own totals (`::a_source_replaces_its_own_totals_and_sources_add_up`),
  and usage goes with its session (`::usage_goes_when_the_session_it_belonged_to_does`).
- The log snapshot, tail, eviction and stream behave
  (`logs.rs::the_snapshot_returns_captured_lines_in_order`,
  `::tail_limits_the_snapshot_to_the_last_n_lines`,
  `::the_ring_buffer_evicts_its_oldest_lines`,
  `::the_stream_opens_with_a_snapshot_then_follows_with_deltas`), and hostile
  content survives SSE framing (`::hostile_log_content_survives_sse_framing`).
- Every endpoint is in the OpenAPI document
  (`logs.rs::both_endpoints_are_in_the_openapi_document`,
  `doctor.rs::endpoint_is_in_the_openapi_document`,
  `models.rs::endpoint_is_in_the_openapi_document_with_nothing_to_filter_by`).

## Sources

`crates/ariadne-api/`, `crates/ariadne-daemon/src/http/`,
`crates/ariadne-daemon/src/bus.rs`, `crates/ariadne-store/src/usage.rs`.
