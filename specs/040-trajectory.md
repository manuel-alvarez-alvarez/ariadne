---
id: trajectory
status: current
updated: 2026-11-27
areas: [api, core, daemon, store]
commits: []
tests:
  - crates/ariadne-core/src/event_facts.rs
  - crates/ariadne-store/src/events.rs
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

In: the types in `crates/ariadne-api/src/trajectory.rs`, the event facts in
`crates/ariadne-core/src/event_facts.rs` and the columns that hold them, the
fold and the bounded read in `crates/ariadne-daemon/src/trajectory/`, and the
routes and the hub in `crates/ariadne-daemon/src/http/trajectory.rs`. The
routes are listed in 012, rule 8.

Out:

- The events themselves and their payloads. They are the ACP runtime's (021).
- The console routes and the two consoles that read them (008).
- The item detail route and the blob route. A later task adds them, and the
  types of the detail.
- The fold of event kinds that do not exist yet, such as a mode, a config
  option or a session title. The types hold them; a later task folds them.

## Behavior

### Event facts

1. The store keeps two columns beside each event's deflated payload: `lane`,
   and `facts`, a small JSON (`EventFacts`). A query can read them, and a
   payload no query can read. Migration 0029 adds them.
2. `lane` is the subagent the payload names, or null for the session's own
   lane. A permission, a question and a subagent announcement are in the
   session's lane whoever they name: the session answers and announces them.
3. `facts` holds what a read counts by: a tool call's id, status, kind and
   paths; a message's `item_id`; a subagent announcement's id and state; a
   question's or a legacy request's id; a diagnosis's error; a stop's reason;
   a compaction's id; a permission request's options and whether it names an
   Ariadne tool; and a reply's option, decider and label. An abandoned call
   is `cancelled` for the reason `cancelled`, and `interrupted` otherwise.
4. The store fills both columns at insert. When it opens, it fills them once
   from the payload for every row stored before migration 0029.

### Turns

5. A turn opens on each stored `user_prompt_submit`. The id of the turn is
   the id of that event. The events of a session before its first prompt
   are a turn with no prompt, opened by the first of them.
6. An event joins the running turn. Where no turn runs, it joins the newest
   turn.
7. A turn's `state` is `running` until one of these ends it:
   - `stop` makes it `cancelled` for the stop reason `cancelled`, and
     `completed` for every other reason. The turn keeps the reason.
   - `session.error` makes it `failed`.
   - `session_start`, `session_end` or a new prompt make it `interrupted`.
8. When a turn ends, its text stops streaming. Its open tool calls become
   `cancelled` in a cancelled turn and `interrupted` in every other.
9. A closed turn carries its prompt, its activity, its attention and its
   answer, and no steps. `earlier_steps` counts its steps.
10. A running turn carries its last 50 steps, and `earlier_steps` counts the
    rest. Fifty steps are about one screen of a client.
11. `answer` is the content of the newest message of the session's own agent
    in the turn.
12. `attention` holds the pending permissions, the open questions and the
    errors of a turn, open or closed. An answer to a permission or a question
    takes it out.
13. `activity` counts the session's own lane of the turn, every event of it:
    - `tool_calls` counts the distinct tool call ids, and `steps` those and
      the subagents.
    - `files_changed` counts the distinct paths of completed `edit`,
      `delete` and `move` calls.
    - `failed` and `cancelled` count the calls in those states.
    - `thoughts` counts the thoughts, and `subagents` the subagents started.

### Steps

14. The id of a step is the id of the stored event that opened it. A message
    and a thought take `payload.item_id` as their id where the payload has
    one. That field joins live chunks and the stored whole into one step.
15. The first chunk of a run opens a `message` or `thought` step with
    `streaming` true. Each later chunk of the run is a `text_append` op. The
    stored whole replaces the content and ends `streaming`. Anything else in
    the same lane also ends `streaming`.
16. A run with no `item_id` streams under the id `<stored>~message` or
    `<stored>~thought`, where `<stored>` is the newest stored event before
    it. Every open of the session reads that same id. When its whole is
    stored, the step takes the whole's id and time: an `item_remove` of the
    streaming id and an `item_upsert` of the whole's id. A read of the store
    names the step by the whole's id too.
17. `pre_tool_use` opens a `tool_call` step. A `tool_call_update`, live or
    stored, a `post_tool_use` and a `tool_call_abandoned` change the open call
    of the same lane and tool call id. Each change adds one to `detail_rev`.
    An update with no open call opens a step.
18. A call keeps the fields that an update does not send again. Its
    `preview` is the first five lines, and at most 400 characters, of its
    output text, else of its `rawOutput`: a row shows that much. Its
    `diff_stat` counts the files of its diffs, and the `+` and `-` lines of a
    patch, or the lines between the common start and the common end of an
    old and a new text. A call with no `locations` takes the paths of its
    diffs.
19. `permission_request` opens a `permission` step in the main lane. The open
    call it asks about names it in `permission`. `permission.replied` answers
    the newest pending permission of the session. A daemon allow of one of
    Ariadne's own tools removes the step with an `item_remove`, as in 008.
20. A native `question` (021, rule 21) opens a `question` step. Its
    `question` is the message, and its `hint` and `options` are the title and
    the choices of the form's first field that offers choices.
    `question.resolved` closes it: `accept` is `answered`, with each field of
    the content as `field: value` in `answer`; `decline` is `declined`; and
    `cancel` is `cancelled`. A legacy `agent_request` with a question folds
    the same way, and one with no question is a `note`.
21. `session.error` is an `error` step. `session.diagnosis` sets the
    `diagnosis` of the error it names (024).
22. `session_start`, `session_end`, `session.switched` and
    `session.pin_fallback` are `note` steps. A pin fallback is a warning.
23. `compaction_update` opens a `compaction` step by `compaction_id`, and a
    later one changes it.
24. A content block with bytes carries a `blob` URL under
    `/v1/sessions/{id}/trajectory/blobs/`, and never the bytes.
25. An event of an unknown kind changes nothing. An event without the fields
    of a later daemon folds with what it has.

### Subagents and plans

26. A subagent is a `subagent` step in the turn of its parent, opened by the
    first event of the session that names it. `subagent_update` changes its
    title, task, state and `can_cancel`. The snapshot lists every subagent in
    `subagents`.
27. An event that names a subagent is in that subagent's lane. A subagent's
    `activity` counts its lane.
28. `plan` replaces the plan of its lane. `plan_id` is the id of the first
    `plan` event of the lane.

### Session

29. `session` starts from the row: status, attention, usage and context. A
    `session_updated` domain event of the session is a `session_patch`.
    `running_turn_id` names the running turn. The agent's own
    `available_commands_update` sets `commands`.

### The fold

30. The fold takes full events and heads. A head is an event's id, kind,
    lane, facts and time, and no payload. The fold keeps the index of each
    turn — its step openers, its calls, its counts — from both by one path,
    so a turn read as a few full events among many heads counts as a turn
    read whole does. A full event also makes and changes steps.
31. The fold pairs every permission request and reply, and every question
    and resolution, of the session, also those outside the turns it holds.

### The bounded read

32. A read loads at most 500 full event payloads: an open, a page of older
    turns, and a page of a turn's steps. Five hundred events are about five
    megabytes at the ten-kilobyte average of a tool end.
33. A read takes the newest event id first, and every query of the read
    stops at it.
34. A read folds the heads of the turns it shows, then loads the events
    that a client sees: each turn's prompt, its answer, and its attention
    with the replies and resolutions that changed it; the last 50 steps of a
    running turn, each with its newest event; the first and the newest event
    of each subagent; and the newest plan of each lane. It shows the newest
    turns whose events fit, the newest turn first.
35. A read also holds up to 20 older turns with attention, outside the
    turns it shows, so that a later answer or diagnosis finds its step.
36. A first event of a subagent or a plan from before the turns a read
    shows keeps the turn it came in.

### Routes

37. `GET /v1/sessions/{id}/trajectory` answers the snapshot: `rev`,
    `session`, the newest `limit` turns oldest first (20 by default, at most
    50), `has_older`, `subagents` and `plans`.
38. With `before=<turn_id>`, it answers the whole turns older than that
    turn, read from the store, at `rev` 0. The prompt of `before` closes the
    last of them as it closed it in the session.
39. With `subagent=<id>`, it reads the store, and each turn where that
    subagent has steps carries the last 50 of them.
40. `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps` answers one page
    of a turn's steps, oldest first: the `limit` steps before the step
    `before` (50 by default, at most 200), and `earlier_steps`. The page
    comes from the cursor, whatever an open holds. With `subagent`, it pages
    that subagent's steps. A `turn_id` that opens no turn is a 404.
41. `GET /v1/sessions/{id}/trajectory/stream` sends one `snapshot`, then an
    `update` frame `{rev, ops}` per revision. Each `rev` is one more than the
    revision before it. A `turn_upsert` carries the turn with no steps. A
    reconnect gets a new snapshot.
42. All the subscribers of a session share one hub: one console merge
    (`open_console`'s input) feeds one fold, and one broadcast sends each
    revision. So two subscribers receive the same `rev` sequence. Two that
    come at once to a session with no hub open one. A subscriber that falls
    more than 1024 revisions behind receives `resync`, and its stream closes.
    The hub ends when its last subscriber leaves.
43. A hub keeps the newest 100 turns, and the older ones with attention
    outside them.
44. A session on a remote host serves the same routes through the
    forwarder, with the session named by its alias (037).

## Acceptance criteria

1. The facts of an event, its lane, and the daemon's own allow
   (rules 2, 3): `event_facts.rs::the_facts_hold_what_a_read_counts_by`.
2. The store keeps the facts and fills them for older rows (rules 1, 4):
   `crates/ariadne-store/src/events.rs::the_facts_of_an_event_are_kept_and_filled_for_older_rows`.
3. Streamed text arrives as appends, and the stream sends one snapshot and
   then consecutive revisions (rules 15, 41):
   `fold.rs::streamed_text_arrives_as_appends` and
   `tests/it/trajectory.rs::the_stream_sends_a_snapshot_then_updates_and_text_as_appends`.
4. A run with no `item_id` keeps one id across opens and ends on its
   whole's id, in the stream and in the steps route (rule 16):
   `fold.rs::a_legacy_run_keeps_one_identity_across_opens_and_takes_its_wholes_id`
   and `tests/it/trajectory.rs::a_streamed_run_keeps_its_id_across_opens_and_ends_on_its_stored_id`.
5. An `item_id` names one item (rule 14):
   `fold.rs::an_item_id_names_the_message_its_chunks_and_whole_belong_to`.
6. A closed turn carries its prompt, activity and answer, and no steps
   (rules 5, 9, 11, 13): `fold.rs::a_closed_turn_carries_its_prompt_activity_and_answer_and_no_steps`.
7. A running turn carries its last steps (rule 10):
   `fold.rs::a_running_turn_carries_its_last_steps`.
8. A call's preview, diff stat, subject and detail revision (rules 17, 18):
   `fold.rs::a_call_previews_its_output_and_counts_its_diff`.
9. Attention, permission answers and native questions (rules 12, 19, 20):
   `fold.rs::attention_keeps_what_waits_and_the_errors_of_closed_turns`.
10. Lifecycle notes, the prompt-less first turn and an interruption
    (rules 5, 7, 22): `fold.rs::lifecycle_events_are_notes_and_a_restart_interrupts_the_turn`.
11. A compaction (rule 23):
    `fold.rs::a_compaction_is_one_step_that_its_updates_change`.
12. Unknown kinds and payloads without the new fields (rule 25):
    `fold.rs::events_without_the_new_fields_and_unknown_kinds_fold`.
13. A subagent is a step of its parent's turn, and its steps are its lane
    (rules 26, 27, 39): `fold.rs::a_subagent_is_a_step_in_its_parents_turn_and_its_steps_are_its_lane`
    and `tests/it/trajectory.rs::a_subagent_is_a_step_of_its_parent_and_its_steps_are_a_query_away`.
14. Heads and full events count a turn the same (rule 30):
    `fold.rs::heads_and_whole_events_count_a_turn_the_same`.
15. A pruned turn keeps its open question for a later answer (rules 31, 43):
    `fold.rs::a_pruned_turn_keeps_its_open_question_for_a_later_answer`.
16. An open of many turns loads at most the budget, whole turns first
    (rules 32 to 34): `read.rs::the_open_of_a_long_session_reads_a_bounded_count_of_payloads`.
17. An oversized closed turn keeps its answer and exact activity
    (rules 13, 34): `read.rs::an_oversized_closed_turn_keeps_its_answer_and_exact_activity`.
18. A running turn's window is its last steps, whatever events follow them
    (rule 34): `read.rs::a_running_turns_window_is_its_last_steps_whatever_follows_them`.
19. A turn of thousands of events loads the budget at most (rule 32):
    `read.rs::a_turn_of_thousands_of_events_loads_the_budget_at_most`.
20. Attention outside the window keeps its turn, and a later answer finds
    it (rules 12, 31, 35): `read.rs::attention_outside_the_window_keeps_its_turn`.
21. A subagent opened before the window keeps its step and parent
    (rules 26, 36): `read.rs::a_subagent_opened_before_the_window_keeps_its_step_and_parent`.
22. A page before an interrupting prompt keeps the interruption (rule 38):
    `read.rs::a_page_before_an_interrupting_prompt_keeps_the_interruption`.
23. The steps route pages by its cursor and names a turn (rule 40):
    `read.rs::the_steps_route_pages_by_its_cursor_and_names_a_turn` and
    `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`.
24. A page of older whole turns, and a page of steps, over HTTP
    (rules 37, 38, 40):
    `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`.
25. Two subscribers that open at once receive the same `rev` sequence; two
    cold subscribers share one slot; a subscriber that lags receives `resync`
    (rule 42): `tests/it/trajectory.rs::two_subscribers_receive_the_same_rev_sequence`,
    `http/trajectory.rs::two_cold_subscribers_share_one_slot` and
    `http/trajectory.rs::a_subscriber_that_lags_receives_resync`.
26. A remote session serves the routes through the forwarder (rule 44):
    `far_tasks/source_references.rs::source_trajectory_routes_reach_the_worker_session`.
27. `ui/openapi.json` and `ui/src/api/schema.d.ts` hold every trajectory
    type, every step kind and every op:
    `docs.rs::the_committed_api_pair_holds_every_trajectory_type`.

28. A diagnosis, a blob URL, a plan's id and the session's state
    (rules 21, 24, 28, 29): `fold.rs::a_diagnosis_reads_onto_the_error_it_names`,
    `::a_block_with_bytes_carries_a_blob_url_and_not_the_bytes`,
    `::a_plan_keeps_the_id_of_its_first_event` and
    `::the_session_state_follows_the_row_the_running_turn_and_the_commands`.

Rules 6 and 8 hold in the tests above: every fold test puts its events in
the running turn, and
`read.rs::a_running_turns_window_is_its_last_steps_whatever_follows_them`
and `fold.rs::heads_and_whole_events_count_a_turn_the_same` end turns with
their calls.

## Known gap

Rule 33 has no test of its own: no test can stop a read between two of its
queries. Every query of `read.rs` takes the one bound the read began with.

A diagnosis of an error older than the 20 turns with attention that a hub
holds changes no step the stream sends. A page that holds the error shows the
diagnosis.

The steps route reads the stored events. A message that still streams is in
the stream, and not yet in a page.

A page with `before` holds the subagents and plans of its own read.

## Sources

`crates/ariadne-api/src/trajectory.rs`,
`crates/ariadne-core/src/event_facts.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-store/migrations/0029_agent_event_facts.sql`,
`crates/ariadne-daemon/src/trajectory/`,
`crates/ariadne-daemon/src/http/trajectory.rs`,
`crates/ariadne-daemon/src/http/console.rs` (`Merge`, `open_console`),
`crates/ariadne-daemon/src/http/source_sessions.rs`, and the two folds it
replaces for a client: `crates/ariadne-console/src/transcript.rs` and
`ui/src/features/sessions/console/transcript.ts`.
