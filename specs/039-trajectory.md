---
id: trajectory
status: current
updated: 2026-11-26
areas: [api, daemon, store]
commits: []
tests:
  - crates/ariadne-daemon/src/trajectory/fold.rs
  - crates/ariadne-daemon/src/trajectory/read.rs
  - crates/ariadne-daemon/src/http/trajectory.rs
  - crates/ariadne-daemon/tests/it/trajectory.rs
  - crates/ariadne-daemon/tests/it/far_tasks/source_references.rs
  - crates/ariadne-daemon/tests/it/docs.rs
---

# Trajectory

The daemon folds the events of a session into typed turns. It serves them as
a snapshot and as a stream of updates. A client draws a session from these
types and does not fold events itself.

## Scope

In: the types in `crates/ariadne-api/src/trajectory.rs`, the fold in
`crates/ariadne-daemon/src/trajectory/`, the bounded read of the stored
events, and the routes in `crates/ariadne-daemon/src/http/trajectory.rs`. The
routes are listed in 012, rule 8.

Out:

- The events themselves and their payloads. They are the ACP runtime's (021).
- The console routes and the two consoles that read them (008).
- The item detail route and the blob route. A later task adds them, and the
  types of the detail.
- The fold of event kinds that do not exist yet, such as a mode, a config
  option or a session title. The types hold them; a later task folds them.

## Behavior

### Turns

1. A turn opens on each stored `user_prompt_submit`. The id of the turn is
   the id of that event. The events of a session before its first prompt
   are a turn with no prompt, opened by the first of them.
2. An event joins the running turn. Where no turn runs, it joins the newest
   turn.
3. A turn's `state` is `running` until one of these ends it:
   - `stop` makes it `cancelled` for the stop reason `cancelled`, and
     `completed` for every other reason. The turn keeps the reason.
   - `session.error` makes it `failed`.
   - `session_start`, `session_end` or a new prompt make it `interrupted`.
4. When a turn ends, its text stops streaming. Its open tool calls become
   `cancelled` in a cancelled turn and `interrupted` in every other.
5. A closed turn carries its prompt, its activity, its attention and its
   answer, and no steps. `earlier_steps` counts the steps it holds.
6. A running turn carries its last 50 steps. `earlier_steps` counts the rest.
   Fifty steps are about one screen of a client. The rest are a page of the
   steps route away.
7. `answer` is the content of the last message of the session's own agent in
   the turn.
8. `attention` holds the pending permissions, the open questions and the
   errors of a turn, open or closed. An answer to a permission or a question
   takes it out.
9. `activity` counts the steps of the session's own lane:
   - `steps` counts the tool calls and the subagents.
   - `files_changed` counts the distinct paths of completed `edit`,
     `delete` and `move` calls.
   - `failed` and `cancelled` count the calls in those states.
   - `thoughts` counts the thoughts, and `subagents` the subagents.

### Steps

10. The id of a step is the id of the stored event that opened it. A message
    and a thought take `payload.item_id` as their id where the payload has
    one. The same field joins live chunks and the stored whole into one step.
11. The first chunk of a run opens a `message` or `thought` step with
    `streaming` true. Each later chunk of the run is a `text_append` op. The
    stored whole replaces the content and ends `streaming`. Anything else in
    the same lane also ends `streaming`.
12. `pre_tool_use` opens a `tool_call` step. A live or stored
    `tool_call_update`, `post_tool_use` and `tool_call_abandoned` change the
    newest open call of the same lane and `toolCallId`. Each change adds one
    to `detail_rev`. An update with no open call opens a step.
13. A call keeps the fields that an update does not send again. Its
    `preview` is the first five lines, and at most 400 characters, of its
    output text, else of its `rawOutput`. Its `diff_stat` counts the files
    of its diffs, and the `+` and `-` lines of a patch, or the lines between
    the common start and the common end of an old and a new text. A call with
    no `locations` takes the paths of its diffs.
14. `permission_request` opens a `permission` step in the main lane, whoever
    asks. The open call it asks about names it in `permission`.
    `permission.replied` answers the newest pending permission. A daemon
    allow of one of Ariadne's own tools is an `item_remove`, as in 008.
15. `agent_request` with a question opens a `question` step, and
    `agent_request.resolved` closes it by `request_id`. One with no question
    is a `note`.
16. `session.error` is an `error` step. `session.diagnosis` sets the
    `diagnosis` of the error it names (024).
17. `session_start`, `session_end`, `session.switched` and
    `session.pin_fallback` are `note` steps. A pin fallback is a warning.
18. `compaction_update` opens or changes a `compaction` step by
    `compaction_id`.
19. A content block with bytes carries a `blob` URL under
    `/v1/sessions/{id}/trajectory/blobs/`, and never the bytes.
20. An event of an unknown kind changes nothing. An event without the
    fields of a later daemon folds with what it has.

### Subagents and plans

21. A subagent is a `subagent` step in the turn of its parent, opened by the
    first event that names it. `subagent_update` changes its title, task,
    state and `can_cancel`. The snapshot lists every subagent in
    `subagents`.
22. An event that names a subagent is in that subagent's lane: the lane is
    its id. A subagent's `activity` counts the steps of its lane.
23. `plan` replaces the plan of its lane. `plan_id` is the id of the first
    `plan` event of the lane.

### Session

24. `session` starts from the row: status, attention, usage and context.
    A `session_updated` domain event of the session is a `session_patch`.
    `running_turn_id` names the running turn. The agent's own
    `available_commands_update` sets `commands`.

### Routes

25. `GET /v1/sessions/{id}/trajectory` answers the snapshot: `rev`,
    `session`, the newest `limit` turns oldest first (20 by default, at most
    50), `has_older`, `subagents` and `plans`.
26. With `before=<turn_id>`, it answers the whole turns older than that
    turn, read from the store, at `rev` 0.
27. With `subagent=<id>`, each turn where that subagent has steps carries
    the last 50 of them.
28. `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps` answers one
    page of a turn's steps, oldest first: the `limit` steps before the step
    `before` (50 by default, at most 200), and `earlier_steps`. With
    `subagent`, it pages that subagent's steps in the turn.
29. `GET /v1/sessions/{id}/trajectory/stream` sends one `snapshot`, then an
    `update` frame `{rev, ops}` per revision. Each `rev` is one more than the
    revision before it. A `turn_upsert` carries the turn with no steps.
    A reconnect gets a new snapshot.
30. All the subscribers of a session share one hub: one console merge
    (`open_console`'s input) feeds one fold, and one broadcast sends each
    revision. So two subscribers receive the same `rev` sequence. A
    subscriber that falls more than 1024 revisions behind receives `resync`,
    and its stream closes. The hub ends when its last subscriber leaves.
31. A session on a remote host serves the same routes through the
    forwarder, with the session named by its alias (037).

### The bounded read

32. An open reads at most 500 full event payloads. Five hundred events are
    about five megabytes at the ten-kilobyte average of a tool end.
33. The read counts the events of each window and lists the prompt ids
    first, which read no payload. It takes the newest whole turns that fit,
    and keeps up to 100 turns while it follows the session.
34. A turn larger than the bound is read as its prompt and its newest
    events. The steps left out count into its `earlier_steps`.

## Acceptance criteria

1. Streamed text arrives as appends, and the stream sends one snapshot and
   then consecutive revisions (rules 11, 29):
   `fold.rs::streamed_text_arrives_as_appends` and
   `tests/it/trajectory.rs::the_stream_sends_a_snapshot_then_updates_and_text_as_appends`.
2. A closed turn carries its prompt, activity and answer, and no steps
   (rules 5, 7, 9):
   `fold.rs::a_closed_turn_carries_its_prompt_activity_and_answer_and_no_steps`.
3. A running turn carries its last steps (rule 6):
   `fold.rs::a_running_turn_carries_its_last_steps`.
4. The steps of a turn come in pages (rule 28):
   `fold.rs::the_steps_of_a_turn_come_in_pages` and
   `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`.
5. Attention keeps a pending permission, an open question and an error,
   however old the turn (rules 8, 14, 15):
   `fold.rs::attention_keeps_what_waits_and_the_errors_of_closed_turns`.
6. `before` answers a page of older whole turns (rules 26, 33):
   `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`
   and `read.rs::a_page_before_a_turn_holds_the_turns_older_than_it`.
7. Two subscribers receive the same `rev` sequence, and a subscriber that
   lags receives `resync` (rule 30):
   `tests/it/trajectory.rs::two_subscribers_receive_the_same_rev_sequence` and
   `http/trajectory.rs::a_subscriber_that_lags_receives_resync`.
8. A subagent is a step of its parent's turn, and `?subagent=` answers its
   steps (rules 21, 22, 27):
   `fold.rs::a_subagent_is_a_step_in_its_parents_turn_and_its_steps_are_its_lane`
   and
   `tests/it/trajectory.rs::a_subagent_is_a_step_of_its_parent_and_its_steps_are_a_query_away`.
9. A remote session serves the routes through the forwarder (rule 31):
   `far_tasks/source_references.rs::source_trajectory_routes_reach_the_worker_session`.
10. The open of a long session reads a bounded count of full payloads
    (rules 32 to 34):
    `read.rs::the_open_of_a_long_session_reads_a_bounded_count_of_whole_turns`
    and `read.rs::a_turn_larger_than_the_bound_is_its_prompt_and_its_newest_events`.
11. `ui/openapi.json` and `ui/src/api/schema.d.ts` hold every trajectory
    type, every step kind and every op:
    `docs.rs::the_committed_api_pair_holds_every_trajectory_type`.
12. Item ids and old payloads (rules 10, 20):
    `fold.rs::an_item_id_names_the_message_its_chunks_and_whole_belong_to` and
    `::events_without_the_new_fields_and_unknown_kinds_fold`.

## Known gap

A turn larger than the bound counts in `activity` only the events the read
holds. Its `earlier_steps` counts the left-out events whose kind opens a step,
the subagent lanes included.

The steps route reads the stored events. A message that still streams is in
the stream, and not yet in a page.

A page with `before` holds the subagents and plans of its own events.

## Sources

`crates/ariadne-api/src/trajectory.rs`,
`crates/ariadne-daemon/src/trajectory/`,
`crates/ariadne-daemon/src/http/trajectory.rs`,
`crates/ariadne-daemon/src/http/console.rs` (`Merge`, `open_console`),
`crates/ariadne-daemon/src/http/source_sessions.rs`,
`crates/ariadne-store/src/events.rs`, and the two folds it replaces for a
client: `crates/ariadne-console/src/transcript.rs` and
`ui/src/features/sessions/console/transcript.ts`.
