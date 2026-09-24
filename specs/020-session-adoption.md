---
id: outside-session-resume
status: current
updated: 2026-09-24
areas: [api, daemon, store]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/acp_session_resume.rs
  - crates/ariadne-daemon/tests/it/outside_sessions.rs
  - crates/ariadne-store/tests/store.rs
---

# Outside session resume

An outside conversation resumes as a loose session.
A loose session has no goal, task, staffed agent or seat.

## Scope

In: discovering stored ACP conversations and resuming one in its recorded working directory.

Out: merged session listing, CLI commands, desktop screens, and task staffing.
Session controls and revival belong to 008.
The ACP runtime belongs to 021.

## Behavior

1. Discovery asks every registry agent that the last discovery run (007)
   found ready and advertising `session_list`, over `session/list`. Each is
   one short-lived process, the same shape as a probe. An agent that pages
   its list is asked again with each `nextCursor` it answers, until a reply
   carries none; one 5 s budget covers all its pages, and the pages that
   arrived inside it are kept when it runs out. An agent without the
   capability — rejected outright, or ready but lacking it — contributes no
   sessions, and `GET /v1/acp-agents` says why, in `rejection_reason` or the
   `no_adoption` degradation. An agent that fails to answer contributes
   nothing rather than failing the listing.
2. Each listed session is an `OutsideSessionDto`: the registry `agent_id` it
   belongs to, its `internal_session_id`, its `working_directory`, its
   `last_activity_at`, and its title as `first_prompt`. The daemon keeps one
   in-memory snapshot of every agent's stored sessions and the moment it was
   taken, resumed ones included; nothing of it is written to disk. The
   snapshot is taken on the first listing request, again when a request
   carries `refresh=true`, and again when it is older than 60 seconds at
   request time. A request inside that minute asks no agent.
3. A session whose agent and internal id already occur on an Ariadne session
   row is not an outside session and is never listed. A row's agent is read
   off its `model` column, which always holds `<agent>:<model>`. The rows
   are read at query time, so a session resumed after the snapshot was
   taken is gone from the next page without a refresh.
4. `GET /v1/outside-sessions` answers one page of the snapshot, as an
   `OutsideSessionPageDto`: `sessions`, `next_cursor` (null on the last
   page), `total` (the count after filters) and `snapshot_at`. Its
   `OutsideSessionListQuery` narrows the snapshot — `agent` (a registry
   agent id), `dir` (an absolute path; a session matches when its working
   directory is that path or a path under it), `since` and `until` (RFC
   3339, inclusive bounds on last activity), `q` (a case-insensitive
   substring of the first prompt) — and picks the page: `limit` (default
   50, cap 200), `cursor` and `refresh`. The sessions are ordered by last
   activity, newest first, ties broken by agent id, then internal session
   id. The cursor is an opaque keyset over the sort key of the page's last
   row, so a page cut after a refresh continues from that key. A cursor the
   daemon cannot read is refused with a 400 envelope, code `invalid_cursor`.
5. `POST /v1/outside-sessions/resume` takes `{agent_id, internal_session_id}` and returns a live `SessionDto`.
   Only the user can call it.
   An agent session receives 403.
6. Resume finds the conversation in the daemon snapshot.
   A miss takes one fresh snapshot and refuses that call with 404.
7. The agent starts through `session/load` in its recorded working directory.
   Resume creates no goal, task, worktree or branch.
   The row stores that directory in `worktree_path`.
   It stores the agent's internal session id.
8. The session uses the daemon's default permission mode.
   Its model is `<agent>:<model>`, from the load response's model configuration option.
   Without that option, the row records the agent's discovered default model.
   Resume does not replace the loaded model with a task pin.
9. Concurrent or repeated resumes of a live loose session return the same row.
   They start one agent process.
   A later resume of an ended loose row reopens that row through the usual session revival path.
10. A loose session supports the console snapshot, console stream, input, cancel, kill, resume, and events.
    Its loaded transcript appears in the console.
    The scheduler creates no task or orchestrator for it.
11. The adoption endpoint no longer exists.
    The OpenAPI document contains the resume endpoint.

## Acceptance criteria

- An agent's stored sessions appear in the listing with their recorded
  fields, named by their registry agent id
  (`acp_session_resume.rs::an_acp_agents_stored_sessions_appear_in_the_listing`).
- An agent without `session_list` lists no sessions, and the catalog says why
  (`acp_session_resume.rs::an_agent_without_the_capability_lists_nothing_and_shows_the_reason`).
- An agent that pages its list in three pages has every session in the
  listing
  (`outside_sessions.rs::a_paging_agents_every_session_is_in_the_listing`).
- An agent whose budget ends mid-list is listed with the pages that arrived
  (`outside_sessions.rs::the_pages_that_arrived_are_kept_when_an_agents_budget_ends`).
- A cursor continues from the same row after a refresh
  (`outside_sessions.rs::a_cursor_continues_from_the_same_row_after_a_refresh`).
- A listing of 5 sessions with `limit=2` is three pages through
  `next_cursor`, newest first, with `total=5` on each and a null cursor on
  the last
  (`outside_sessions.rs::five_sessions_at_limit_two_are_three_pages_newest_first`).
- `agent`, `dir`, `since`, `until` and `q` each narrow the listing, and
  `dir` matches a directory under the given path
  (`outside_sessions.rs::agent_narrows_the_listing_to_one_agents_sessions`,
  `::dir_narrows_the_listing_to_a_path_and_what_is_under_it`,
  `::since_narrows_the_listing_to_activity_at_or_after_it`,
  `::until_narrows_the_listing_to_activity_at_or_before_it`,
  `::q_narrows_the_listing_by_first_prompt_case_insensitively`).
- A second request within 60 seconds asks no agent again, and a request with
  `refresh=true` does
  (`outside_sessions.rs::a_second_request_asks_no_agent_again_but_a_refresh_does`).
- A session resumed after the snapshot was taken is absent from the next
  page without a refresh
  (`outside_sessions.rs::a_session_resumed_after_the_snapshot_is_absent_without_a_refresh`).
- A cursor the daemon cannot read is refused with `invalid_cursor`
  (`outside_sessions.rs::an_unreadable_cursor_is_refused`).
- The endpoint's query parameters and page DTO are in the OpenAPI document
  (`outside_sessions.rs::the_query_and_the_page_are_in_the_openapi_document`).
- A row with no goal, task or seat survives a store reopen
  (`store.rs::a_loose_session_round_trips_without_a_goal_task_or_seat`).
- Resume loads the recorded directory and internal id, records the loaded model, and creates no scheduled work or worktree
  (`acp_session_resume.rs::an_outside_session_loads_in_its_directory_without_scheduled_work`).
- Concurrent resumes return one row and start one process
  (`acp_session_resume.rs::concurrent_resumes_return_one_row_and_start_one_process`).
- A load without a model option records the agent's default model
  (`acp_session_resume.rs::no_loaded_model_uses_the_agents_default_model`).
- A catalog kept before default models were recorded is refreshed without a
  manual registry refresh. Resume then records the loaded model or the default
  (`acp_session_resume.rs::an_old_catalog_does_not_block_the_loaded_model`,
  `::an_old_catalog_recovers_the_default_when_load_has_no_model`).
- A snapshot miss refreshes once and returns 404
  (`acp_session_resume.rs::a_session_missing_from_the_snapshot_is_refused_after_one_fresh_snapshot`).
- An agent caller receives 403
  (`acp_session_resume.rs::an_agent_session_cannot_resume_an_outside_session`).
- The console serves loaded history, takes input, cancels a turn, and survives kill followed by resume
  (`acp_session_resume.rs::a_loose_console_serves_history_takes_input_and_cancels`).
- A loose session uses the configured permission mode and accepts its answer through console input
  (`acp_session_resume.rs::a_loose_session_uses_the_daemons_permission_mode`).
- OpenAPI contains resume and omits adoption
  (`acp_session_resume.rs::the_resume_endpoint_replaces_adoption_in_openapi`).

## Known gap

Outside resumes share one lock through the load handshake. Different
conversations wait for each other, up to the load timeout. The lock prevents
duplicate processes and prevents a second caller receiving a row before load completes.

## Sources

`crates/ariadne-api/src/sessions.rs`,
`crates/ariadne-daemon/src/acp_sessions.rs`,
`crates/ariadne-daemon/src/acp_discovery.rs`,
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/acp.rs`.
