---
id: session-adoption
status: current
updated: 2026-09-12
areas: [api, daemon, cli]
commits: []
tests:
  - crates/ariadne-daemon/tests/acp_session_adoption.rs
  - crates/ariadne-daemon/tests/outside_sessions.rs
  - crates/ariadne-cli/src/commands/session.rs
---

# Session adoption

Ariadne can take over a conversation that started outside it: a session an
ACP agent stored itself, which becomes the author of a new task.

## Scope

In: finding the stored sessions of the registry agents that can list them,
what a listed session says about itself, and adopting one into a new task as
its author.

Out: changing a task's author pin (011), importing a transcript, sessions
that Ariadne started itself (007, 021), and the desktop screen over this
(015).

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
   taken, adopted ones included; nothing of it is written to disk. The
   snapshot is taken on the first listing request, again when a request
   carries `refresh=true`, and again when it is older than 60 seconds at
   request time. A request inside that minute asks no agent.
3. A session whose agent and internal id already occur on an Ariadne session
   row is not an outside session and is never listed. A row's agent is read
   off its `model` column, which always holds `<agent>:<model>`. The rows
   are read at query time, so a session adopted after the snapshot was
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
5. `POST /v1/outside-sessions/adopt` is a user-only call. It takes the
   session's `agent_id` and `internal_session_id`; a new goal's title,
   optional description and optional repositories, or an existing goal id;
   and the new task's optional title, description, optional repository,
   agents, landing and permission mode. The reply carries the goal, task and
   adopted author session. The older task-first endpoint remains until its
   callers move. Adoption looks in the daemon snapshot first. A miss takes
   one fresh snapshot but refuses that call with 404.
6. An existing goal must be active. Where it has several repositories, the
   request's `repo_id` selects one, or the session working directory selects
   the repository that contains it. A new goal with no repository ids takes
   the registered repository containing that directory. No match is a
   conflict naming the directory. The first task agent is its author, and
   its pin must name the outside session's agent. An omitted task title is
   the first prompt, cut at 120 characters; an empty title is refused.
7. New adopted goals open active and unorchestrated. Their model and effort
   record the adopted author's pin. Adoption creates the goal where needed,
   creates and staffs the task, moves it to ready, creates its worktree and
   author row, and loads the outside internal id before waking the scheduler.
   The author starts through `session/load`, the task becomes `in_progress`,
   and no scheduler pass creates an orchestrator for that goal. The user
   completes or cancels it. The author is reached afterwards through
   `POST /v1/sessions/{id}/console/input` (008).
8. `ariadne session discover` filters and pages the outside sessions, prints
   the shown and total counts with a reusable next-page command, or follows
   every page with `--all`; below the table it names why each unavailable
   registry agent cannot list sessions. `ariadne session adopt <session-id> <task-id>
   --agent <agent-id>` adopts one and prints its author session.

## Acceptance criteria

- An agent's stored sessions appear in the listing with their recorded
  fields, named by their registry agent id
  (`acp_session_adoption.rs::an_acp_agents_stored_sessions_appear_in_the_listing`).
- An agent without `session_list` lists no sessions, and the catalog says why
  (`acp_session_adoption.rs::an_agent_without_the_capability_lists_nothing_and_shows_the_reason`).
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
- A session adopted after the snapshot was taken is absent from the next
  page without a refresh
  (`outside_sessions.rs::a_session_adopted_after_the_snapshot_is_absent_without_a_refresh`).
- A cursor the daemon cannot read is refused with `invalid_cursor`
  (`outside_sessions.rs::an_unreadable_cursor_is_refused`).
- The endpoint's query parameters and page DTO are in the OpenAPI document
  (`outside_sessions.rs::the_query_and_the_page_are_in_the_openapi_document`).
- Adopting a listed session binds it to the author seat, resumes it through
  `session/load` rather than `session/resume`, and a later console prompt
  reaches the same agent
  (`acp_session_adoption.rs::an_adopted_session_binds_the_seat_and_a_follow_up_prompt_reaches_it`).
- Adoption into a new goal returns an active, unorchestrated goal, an
  in-progress task and the loaded author session. A scheduler pass leaves
  exactly that author and no orchestrator
  (`acp_session_adoption.rs::adoption_into_a_new_goal_creates_and_loads_the_author_without_an_orchestrator`).
- The new goal takes the registered repository containing the working
  directory; no matching repository is refused by directory
  (`acp_session_adoption.rs::adoption_into_a_new_goal_creates_and_loads_the_author_without_an_orchestrator`,
  `::a_new_goal_without_a_matching_repository_is_refused_by_directory`).
- An active goal receives the task, while planning and completed goals refuse
  it (`acp_session_adoption.rs::adoption_into_an_active_goal_adds_the_task_to_that_goal`,
  `::adoption_into_a_goal_that_is_not_active_is_refused`).
- Another agent in the author pin is refused
  (`acp_session_adoption.rs::adoption_is_refused_when_the_author_pin_names_another_agent`).
- A snapshot miss refreshes once and returns 404
  (`acp_session_adoption.rs::a_session_missing_from_the_snapshot_is_refused_after_one_fresh_snapshot`),
  and an agent-session caller is refused
  (`::an_agent_session_cannot_adopt_an_outside_session`).
- A message to an unorchestrated goal's orchestrator is refused with the
  console instruction
  (`acp_session_adoption.rs::a_message_to_an_unorchestrated_goals_orchestrator_is_refused`).
- An adopted session is not listed as outside again
  (`acp_session_adoption.rs::an_adopted_acp_session_no_longer_appears_in_the_listing`,
  `::adoption_into_a_new_goal_creates_and_loads_the_author_without_an_orchestrator`).
- Adoption is refused across agents
  (`acp_session_adoption.rs::adoption_is_refused_across_acp_agents`), and an
  agent session cannot adopt into another task
  (`::an_agent_cannot_adopt_a_session_for_another_task`).
- `session discover` sends every filter and page flag with UTC date bounds,
  follows all pages without repeating a session, prints the count and the
  reusable next-page command only when one exists, and refuses `--all` with
  `--cursor`
  (`commands/session.rs::every_discover_flag_reaches_its_query_parameter`,
  `::a_date_is_the_utc_day_boundary_for_discovery`,
  `::all_fetches_every_page_and_keeps_each_session_once`,
  `::a_next_cursor_prints_the_command_for_the_next_page`,
  `::the_last_page_prints_no_next_command`,
  `::the_discovery_count_is_shown_over_the_total`,
  `cli/tests.rs::discover_takes_filters_pages_refresh_and_all`,
  `::discover_all_and_cursor_are_exclusive`). It names each agent that cannot
  list sessions with its reason
  (`commands/session.rs::an_agent_without_the_capability_is_named_with_its_reason`).
- The new endpoint and its request, response and goal orchestration flag are
  in OpenAPI
  (`acp_session_adoption.rs::the_goal_and_task_adoption_endpoint_is_in_the_openapi_document`).
- An empty task title is refused
  (`acp_session_adoption.rs::adoption_with_an_empty_task_title_is_refused`),
  and unknown request fields are refused
  (`::the_adoption_request_denies_unknown_fields`).

## Known gap

No test uses the legacy endpoint to adopt into a task that is not `ready`.
That refusal is in `Launcher::adopt_author`.

## Sources

`crates/ariadne-api/src/sessions.rs` (`OutsideSessionListQuery`,
`OutsideSessionPageDto`),
`crates/ariadne-daemon/src/acp_sessions.rs` (the snapshot, the filtered
pages, and the snapshot adoption checks),
`crates/ariadne-daemon/src/acp_discovery.rs` (`AgentRegistry::stored_sessions`),
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs` (`adopt_author`),
`crates/ariadne-cli/src/commands/session.rs`.
