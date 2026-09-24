---
id: outside-session-resume
status: current
updated: 2026-09-24
areas: [api, daemon, store]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/acp_session_resume.rs
  - crates/ariadne-daemon/tests/it/session_list.rs
  - crates/ariadne-store/tests/store.rs
---

# Outside session resume

An outside conversation resumes as a loose session.
A loose session has no goal, task, staffed agent or seat.

## Scope

In: discovering stored ACP conversations, listing them beside Ariadne's own
sessions, and resuming one in its recorded working directory.

Out: CLI commands, desktop screens, and task staffing.
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
   request time. A request inside that minute asks no agent, and neither
   does a request no outside session can answer: `kind=ariadne`, or one
   narrowed by a goal, task, status, seat or attention.
3. A session whose agent and internal id already occur on an Ariadne session
   row is not an outside session and is never listed. A row's agent is read
   off its `model` column, which always holds `<agent>:<model>`. The rows
   are read at query time, so a session resumed after the snapshot was
   taken is gone from the next page without a refresh.
4. `GET /v1/sessions` answers one page over both kinds of session, as a
   `SessionPageDto`: `sessions`, `next_cursor` (null on the last page),
   `total` (the count after filters) and `snapshot_at`. An outside entry
   carries its agent id, the internal session id as its own id, its working
   directory, its last activity and its first prompt as its title, and no
   goal, task, seat or status. An Ariadne entry carries its row, and its
   title is its task's, or its goal's where no task staffs it.
5. Its `SessionPageQuery` narrows the page: `kind` (`ariadne` or `outside`;
   omitted lists both), `agent` (a registry agent id, which an Ariadne row
   carries in its `model`), `status`, `seat`, `goal`, `task` and `attention`
   as they narrow an Ariadne session, `dir` (an absolute path; a session
   matches when its directory is that path or a path under it), `since` and
   `until` (RFC 3339, inclusive bounds on last activity), and `q` (a
   case-insensitive substring of the title). An outside session has no goal,
   task, seat, status or attention, so a filter over one of those leaves none
   of them. `limit` (default 50, cap 200), `cursor`, `refresh` and `all` pick
   the page.
6. The page holds the sessions active in the last 7 days, and of Ariadne's
   own the live ones alone. `since` and `until` replace that window with the
   caller's own, and `all` drops it and lists the ended Ariadne sessions too,
   as a named `status` does. A session whose last activity cannot be read is
   inside no window; an Ariadne session nothing has been heard from is as old
   as its row.
7. The sessions are ordered by last activity, newest first, ties broken by
   agent id, then session id. The cursor is an opaque keyset over the sort
   key of the page's last row, so a page cut after a refresh continues from
   that key. It carries that row's moment word for word, so a moment an agent
   dated finer than a millisecond loses nothing, and the next page skips no
   row inside that millisecond. A cursor the daemon cannot read is refused
   with a 400 envelope, code `invalid_cursor`.
8. `POST /v1/outside-sessions/resume` takes `{agent_id, internal_session_id}` and returns a live `SessionDto`.
   Only the user can call it.
   An agent session receives 403.
9. Resume finds the conversation in the daemon snapshot.
   A miss takes one fresh snapshot and refuses that call with 404.
10. The agent starts through `session/load` in its recorded working directory.
    Resume creates no goal, task, worktree or branch.
    The row stores that directory in `worktree_path`.
    It stores the agent's internal session id.
11. The session uses the daemon's default permission mode.
    Its model is `<agent>:<model>`, from the load response's model configuration option.
    Without that option, the row records the agent's discovered default model.
    Resume does not replace the loaded model with a task pin.
12. Concurrent or repeated resumes of a live loose session return the same row.
    They start one agent process.
    A later resume of an ended loose row reopens that row through the usual session revival path.
13. A loose session supports the console snapshot, console stream, input, cancel, kill, resume, and events.
    Its loaded transcript appears in the console.
    The scheduler creates no task or orchestrator for it.
14. The adoption endpoint no longer exists, and neither does the
    outside-session listing.
    The OpenAPI document contains the resume endpoint and the one listing.

## Acceptance criteria

- An agent's stored sessions appear in the listing with their recorded
  fields, named by their registry agent id
  (`acp_session_resume.rs::an_acp_agents_stored_sessions_appear_in_the_listing`).
- An agent without `session_list` lists no sessions, and the catalog says why
  (`acp_session_resume.rs::an_agent_without_the_capability_lists_nothing_and_shows_the_reason`).
- An agent that pages its list in three pages has every session in the
  listing
  (`session_list.rs::a_paging_agents_every_session_is_in_the_listing`).
- An agent whose budget ends mid-list is listed with the pages that arrived
  (`session_list.rs::the_pages_that_arrived_are_kept_when_an_agents_budget_ends`).
- One page holds both kinds, ordered by last activity, newest first
  (`session_list.rs::a_page_holds_both_kinds_newest_first`), and `kind`
  narrows it to one of them
  (`::kind_narrows_the_page_to_one_of_the_two`).
- An outside row carries an empty goal, task, seat and status
  (`session_list.rs::an_outside_row_carries_no_goal_task_seat_or_status`).
- A cursor continues from the same row after a refresh
  (`session_list.rs::a_cursor_continues_from_the_same_row_after_a_refresh`),
  and a moment finer than a millisecond survives it
  (`::a_cursor_keeps_a_moment_finer_than_a_millisecond`).
- A page of 5 sessions with `limit=2` is three pages through `next_cursor`,
  newest first, with `total=5` on each and a null cursor on the last
  (`session_list.rs::five_sessions_at_limit_two_are_three_pages_newest_first`).
- `agent`, `dir`, `since`, `until` and `q` each narrow the page, and `dir`
  matches a directory under the given path, of either kind
  (`session_list.rs::agent_narrows_the_listing_to_one_agents_sessions`,
  `::dir_narrows_the_listing_to_a_path_and_what_is_under_it`,
  `::dir_narrows_the_listing_by_an_ariadne_sessions_worktree`,
  `::since_narrows_the_listing_to_activity_at_or_after_it`,
  `::until_narrows_the_listing_to_activity_at_or_before_it`,
  `::q_narrows_the_listing_by_title_case_insensitively`).
- `goal`, `task`, `seat`, `status` and `attention` each narrow the page to
  Ariadne's own sessions
  (`session_list.rs::goal_narrows_the_listing_to_one_goals_sessions`,
  `::task_narrows_the_listing_to_one_tasks_sessions`,
  `::seat_narrows_the_listing_to_one_seat`,
  `::status_narrows_the_listing_to_one_status`,
  `::attention_narrows_the_listing_to_the_sessions_waiting_on_somebody`).
- The default page holds the last 7 days, `all` widens it, and `all` lists
  the sessions that have ended
  (`session_list.rs::the_default_page_holds_the_last_seven_days_and_all_widens_it`,
  `::all_lists_an_ended_ariadne_session`).
- A second request within 60 seconds asks no agent again, and a request with
  `refresh=true` does
  (`session_list.rs::a_second_request_asks_no_agent_again_but_a_refresh_does`).
- A request no outside session can answer asks no agent, even with
  `refresh=true`
  (`session_list.rs::a_page_with_no_room_for_an_outside_session_asks_no_agent`).
- A resumed outside session is in the page once, as an Ariadne session, and
  without a refresh
  (`session_list.rs::a_resumed_outside_session_is_listed_once_as_an_ariadne_session`).
- A cursor the daemon cannot read is refused with `invalid_cursor`
  (`session_list.rs::an_unreadable_cursor_is_refused`).
- The query parameters and the page DTO are in the OpenAPI document, and the
  outside listing is gone from it
  (`session_list.rs::the_query_and_the_page_are_in_the_openapi_document`).
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
`crates/ariadne-daemon/src/http/convert.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/acp.rs`.
