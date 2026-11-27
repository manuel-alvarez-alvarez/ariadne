---
id: trajectory
status: current
updated: 2026-11-28
areas: [api, core, daemon, store]
commits: []
tests:
  - crates/ariadne-core/src/event_facts.rs
  - crates/ariadne-store/src/events.rs
  - crates/ariadne-store/tests/plan_facts_migration.rs
  - crates/ariadne-daemon/src/trajectory/fold.rs
  - crates/ariadne-daemon/src/trajectory/read.rs
  - crates/ariadne-daemon/src/http/trajectory.rs
  - crates/ariadne-daemon/tests/it/trajectory.rs
  - crates/ariadne-daemon/tests/it/trajectory_acp.rs
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
- The stored kinds of the session state an agent reports, and the display
  state the session row keeps. They are the ACP runtime's (021, rule 24).

## Behavior

### Event facts

1. The store keeps two columns beside each event's deflated payload: `lane`,
   and `facts`, a small JSON (`EventFacts`). A query can read them, and no
   query can read a payload. Migration 0031 adds them, an index on
   `(session_id, kind, id)` for the reads of one kind, and the table
   `store_fills`.
2. `lane` is the subagent the payload names, or null for the session's own
   lane. A permission, a question and a subagent announcement are in the
   session's lane whoever they name: the session answers and announces them.
3. `facts` holds what a read counts by: a tool call's id, status, kind,
   paths, title and locations, the fields of its input that its subject
   reads (a command's first line, a path, a line, a pattern, a URL), and
   whether it gives a preview and carries diffs, never the output itself; a message's `item_id`; a subagent announcement's id and state; a
   question's or a legacy request's id; a diagnosis's error; a stop's
   reason; a compaction's id; a plan event's ACP `plan_id`; a permission request's options and whether it
   names an Ariadne tool; and a reply's option, decider and label. Facts are
   ids, states and kinds alone: no attention step's content is copied there,
   so a permission, a question and an error show from their payloads. The
   user decided this, through the orchestrator.
4. The store derives both columns from the kind and the payload at insert,
   so the code that emits an event does not know them. A stored
   `tool_call_update` `{session_id, tool_call_id, acp}` and a
   `tool_call_abandoned` `{session_id, tool_call_id, reason, acp}` give their
   call's id from `acp.toolCallId`, else `tool_call_id`. An abandoned call is
   `cancelled` for the reason `cancelled`, and `interrupted` otherwise.
5. When the store opens, it fills the columns of every row stored before
   migration 0031 from its payload, before it serves a read. So an old
   session reads as exactly as a new one.
6. The fill runs once. A finished fill writes its row in `store_fills`, and
   a store with that row does not fill again. A later fact of an older kind
   comes with a migration that clears the facts of that kind and the row of
   the fill, so the fill runs again on those rows alone. Migration 0032 does
   this for `plan_update` and `plan_removed`, whose facts now name the plan.
7. The fill is safe to stop. It reads 500 rows a batch, each batch in a
   transaction of its own, through a partial index of the rows with no
   facts. A fill that stops keeps its finished batches and goes on from the
   next row at the next open.
8. The fill costs one read and one inflate of each old row, once: about
   46 µs a row, so about four seconds for an 80,000-row database. Five
   hundred rows are about five megabytes of payload, which one transaction
   holds.

### Turns

9. A turn opens on each stored `user_prompt_submit`. The id of the turn is
   the id of that event. The events of a session before its first prompt
   are a turn with no prompt, opened by the first of them.
10. An event joins the running turn. Where no turn runs, it joins the newest
   turn.
11. A turn's `state` is `running` until one of these ends it:
   - `stop` makes it `cancelled` for the stop reason `cancelled`, and
     `completed` for every other reason. The turn keeps the reason.
   - `session.error` makes it `failed`.
   - `session_start`, `session_end` or a new prompt make it `interrupted`.
12. When a turn ends, its text stops streaming. Its open tool calls become
   `cancelled` in a cancelled turn and `interrupted` in every other.
13. A closed turn carries its prompt, its activity, its attention and its
   answer, and no steps. `earlier_steps` counts its steps.
14. A running turn carries its last 50 steps, and `earlier_steps` counts the
    rest. Fifty steps are about one screen of a client.
15. `answer` is the content of the newest message of the session's own agent
    in the turn.
16. `attention` holds, open turn or closed, the waiting permissions and the
    open questions of the turn, newest first, up to the read limit of 100,
    and then the newest 20 errors of the turn, newest first, and no more.
    The limits hold on every path: a cold read, a warm hub's snapshot, and a
    `turn_upsert`. A hundred waiting steps are five screens, and with the 20
    errors and a running turn's window they fit the payload budget of one
    read, so a cold read and a warm hub hold the same steps. When an answer
    or a new step changes which steps the limits hold, the hub loads the
    payloads of the steps its `attention` lacks, at most 500 at a time, and
    fills them in. The event and its fill are one revision: a subscriber
    sees, and joins on, the whole window and never the turn between them. A
    row counts as filled only once its load succeeded; a load that fails
    ends the hub with `resync`, and the next open reads the window whole. `attention_more` counts each attention step of the turn that
    `attention` does not hold: the waiting steps past the read limit and the
    errors past 20. No read leaves an attention step out without this count,
    and the steps route of the turn returns each step `attention` does not
    hold. An answer to a permission or a question takes it out. The user
    decided this rule, through the orchestrator.
17. `activity` counts the session's own lane of the turn, every event of it:
    - `tool_calls` counts the distinct tool call ids, and `steps` those and
      the subagents.
    - `files_changed` counts the distinct paths of completed `edit`,
      `delete` and `move` calls.
    - `failed` and `cancelled` count the calls in those states.
    - `thoughts` counts the thoughts, and `subagents` the subagents started.

### Steps

18. The id of a step is the id of the stored event that opened it. A message
    and a thought take `payload.item_id` as their id where the payload has
    one. That field joins live chunks and the stored whole into one step.
19. The first chunk of a run opens a `message` or `thought` step with
    `streaming` true. Each later chunk of the run is a `text_append` op. The
    stored whole replaces the content and ends `streaming`. Anything else in
    the same lane also ends `streaming`. A chunk can reach the stream after
    the whole of its item: such a late chunk changes nothing, since the
    whole holds its text.
20. A run that streams has its `item_id` as its id on every open, and after
    it ends: its chunks and its stored whole carry the same `item_id` (021,
    rule 6). A run with no `item_id` — from a daemon before that field —
    streams under the id of the chunk that opened it, and has no promise of
    one id across a new open. When its whole is stored, it takes the whole's
    id and time: an `item_remove` of the chunk's id and an `item_upsert` of
    the whole's id, the id a read of the store gives it. The user decided
    this rule, through the orchestrator.
21. `pre_tool_use` opens a `tool_call` step. A `tool_call_update`, live or
    stored, a `post_tool_use` and a `tool_call_abandoned` change the open call
    of the same lane and tool call id. Each change adds one to `detail_rev`.
    An update with no open call opens a step. A `tool_call_abandoned` ends
    its call `cancelled` for the reason `cancelled`, and `interrupted` for
    every other reason (rule 4). A new open shows a running call with the
    status of its newest stored progress and the preview of the newest
    stored update that gives one (rule 22).
22. A call keeps the fields that an update does not send again. A cold
    read gives a call of any count of updates the step a warm fold gives
    it: it takes the title, kind, locations, subject and status of each
    update from its facts, counts each update in `detail_rev`, and loads
    the payload of the opener, the newest update, the newest update that
    gives a preview and the newest with diffs. Its
    `preview` is the first five lines, and at most 400 characters, of its
    output text, else of its `rawOutput`: a row shows that much. Its
    `diff_stat` counts the files of its diffs, and the `+` and `-` lines of a
    patch, or the lines between the common start and the common end of an
    old and a new text. A call with no `locations` takes the paths of its
    diffs.
23. `permission_request` opens a `permission` step in the main lane. The open
    call it asks about names it in `permission`. `permission.replied` answers
    the newest pending permission of the session. A daemon allow of one of
    Ariadne's own tools removes the step with an `item_remove`, as in 008.
24. A native `question` (021, rule 21) opens a `question` step. Its
    `question` is the message, and its `hint` and `options` are the title and
    the choices of the form's first field that offers choices.
    `question.resolved` closes it: `accept` is `answered`, with each field of
    the content as `field: value` in `answer`; `decline` is `declined`; and
    `cancel` is `cancelled`. A legacy `agent_request` with a question folds
    the same way, and one with no question is a `note`.
25. `session.error` is an `error` step, and `session.diagnosis` sets the
    `diagnosis` of the error it names (024), both from their payloads. The
    index counts every error of a turn from its head; a read loads the
    payloads of the newest 20 and their diagnoses.
26. `session_start`, `session_end`, `session.switched` and
    `session.pin_fallback` are `note` steps. A pin fallback is a warning.
    A `notice` (021, rule 24) is a `note` step with the origin `notice`. Its
    text is the title, then ` · ` and the description where it has one. A
    `warning` or an `error` notice is a warning, and every other severity
    is info: a note has no stronger severity.
27. `compaction_update` opens a `compaction` step by `compaction_id`, and a
    later one changes it: its status, its summary where it gives one, and
    its error. The later one is the step's newest change, so a read loads it
    beside the opener. A compaction holds one step in its lane.
28. A content block with bytes carries a `blob` URL under
    `/v1/sessions/{id}/trajectory/blobs/`, and never the bytes. Adjacent text
    blocks read as one: a stored whole keeps a block per chunk (021, rule 6),
    and the stream appends its chunks to one block.
29. An event of an unknown kind changes nothing. An event without the fields
    of a later daemon folds with what it has.

### Subagents and plans

30. A subagent is a `subagent` step in the turn of its parent, opened by the
    first event of the session that names it. `subagent_update` changes its
    title, task, state and `can_cancel`. The snapshot lists every subagent in
    `subagents`.
31. An event that names a subagent is in that subagent's lane. A subagent's
    `activity` counts its lane.
32. A plan is the plan of a lane and an ACP `plan_id`. Its `plan_id` on the
    wire is the id of its first stored event, so every fold of the session
    names it the same.
    - A `plan_update` replaces the plan of its lane and `plan_id`: an
      `items` plan with its entries, and a `markdown` plan with its content
      and its path. A plan of another type folds as an `items` plan with
      the entries it has.
    - A `plan` of the old kind carries no `plan_id`. It replaces the one
      old-form plan of its lane.
    - A `plan_removed` marks the plan of its lane and `plan_id` removed. A
      later `plan_update` of that `plan_id` opens it again.
    - Each change is a `plan_upsert` with the whole plan, and the turn that
      made it.

### Session

33. `session` starts from the row: status, attention, usage and context. A
    `session_updated` domain event of the session is a `session_patch`.
    `running_turn_id` names the running turn.
    - `mode`, `config_options`, `title`, `cost` and `commands` start from the
      display state of the row (021, rule 24). A hub reads it after it
      follows the console, so a change after that read reaches it live.
    - A live `display_state_update` sets each field it carries, and a
      change is a `session_patch`.
    - A config option lists the values of every group of a grouped select.
      A value that is not text, such as a boolean, reads as its JSON.
    - The agent's own `available_commands_update` sets `commands`. A stored
      one, from a daemon before the display state, sets them only where the
      display state gave none; a cold read loads the newest one of the
      session.

### The fold

34. The fold takes full events and heads. A head is an event's id, kind,
    lane, facts and time, and no payload. The fold keeps the index of each
    turn — its step openers, its calls, its counts — from both by one path,
    so a turn read as a few full events among many heads counts as a turn
    read whole does. A full event also makes and changes steps. Each opener
    keeps the step's id on the wire and the stored row that opened it, and
    the index keeps each step's newest stored change apart: a message whose
    `item_id` is not its event's id is loaded by its rows.
35. The fold pairs every permission request and reply, and every question
    and resolution, of the session, also those outside the turns it holds.

### The bounded read

36. A read loads at most 500 full event payloads: an open, a page of older
    turns, and a page of a turn's steps. Five hundred events are about five
    megabytes at the ten-kilobyte average of a tool end.
37. A read takes the newest event id first, and every query of the read
    stops at it.
38. A read folds the heads of the turns it shows, then loads the rows a
    client sees, in this order, as far as the budget goes:
    - each shown turn whole, newest turn first: its prompt, its answer, its
      newest 100 permissions and questions that wait, and its newest 20
      errors with their diagnoses, the limits of rule 16. The first turn
      whose whole does not fit is not shown, nor any older one, so each shown
      turn holds the `attention` a warm hub holds. The newest turn shows its
      prompt and answer even where its whole does not fit;
    - the last 50 steps of the newest turn's lane if it runs, each with its
      newest change, newest step first;
    - the attention of the older turns that a read holds outside the shown
      ones (rule 39), one step at a time;
    - the first event of each plan, its newest update and its newest
      event, which can be a `plan_removed` (rule 32);
    - the first event and the newest update of each subagent.
39. A read also holds the older turns with attention, outside the turns it
    shows, so that a later answer or diagnosis finds its step: every turn
    with a permission or a question that waits, and the newest 20 turns with
    an error. Twenty keeps a day of failed turns.
40. A first event of a subagent or a plan from before the turns a read
    shows keeps the turn it came in.

### Routes

41. `GET /v1/sessions/{id}/trajectory` answers the snapshot: `rev`,
    `session`, the newest `limit` turns oldest first (20 by default, at most
    50), `has_older`, `subagents` and `plans`.
42. With `before=<turn_id>`, it answers the whole turns older than that
    turn, read from the store, at `rev` 0. The prompt of `before` closes the
    last of them as it closed it in the session.
43. With `subagent=<id>`, it reads the store. It chooses the newest `limit`
    turns that hold events of that lane, and each carries the last 50 of
    them. A lane that no turn holds answers no turn.
44. `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps` answers one page
    of a turn's steps, oldest first: the `limit` steps before the step
    `before` (50 by default, at most 200), and `earlier_steps`. The page
    comes from the cursor, whatever an open holds: `before` names a step,
    and the page holds the steps the lane opened before it, by their place.
    A `before` that names no step of the lane is an empty page. A page cut
    short by the budget keeps its newest steps, and `earlier_steps` counts
    the rest. The prompt after the turn closes it as it closed it in the
    session, so a call that prompt cut short reads as `interrupted`. With
    `subagent`, it pages that subagent's steps. A `turn_id` that opens no
    turn is a 404.
45. `GET /v1/sessions/{id}/trajectory/stream` sends one `snapshot`, then an
    `update` frame `{rev, ops}` per revision. Each `rev` is one more than the
    revision before it. A `turn_upsert` carries the turn with no steps. A
    reconnect gets a new snapshot.
46. All the subscribers of a session share one hub: one console merge
    (`open_console`'s input) feeds one fold, and one broadcast sends each
    revision. So two subscribers receive the same `rev` sequence. Two that
    come at once to a session with no hub open one. A subscriber that falls
    more than 1024 revisions behind receives `resync`, and its stream closes.
    The hub ends when its last subscriber leaves.
47. A hub keeps the newest 100 turns, and the older ones with attention
    outside them.
48. A session on a remote host serves the same routes through the
    forwarder, with the session named by its alias (037).

## Acceptance criteria

1. The facts of an event, its lane, and the daemon's own allow
   (rules 2, 3): `event_facts.rs::the_facts_hold_what_a_read_counts_by`,
   `event_facts.rs::a_calls_facts_hold_its_small_fields_and_no_output`,
   `event_facts.rs::only_an_output_with_a_preview_is_flagged`.
2. The facts of the stored call updates of the ticket contract (rule 4):
   `event_facts.rs::the_stored_call_updates_give_the_facts_of_their_call`.
3. The store keeps the facts at insert, fills them for an old row, and
   fills once (rules 1, 4, 5, 6):
   `crates/ariadne-store/src/events.rs::the_facts_of_an_event_are_kept_and_filled_for_older_rows`.
4. A fill stopped after a batch goes on from the next row, reads each row
   once, and ends once (rules 7, 8):
   `crates/ariadne-store/src/events.rs::a_fill_stopped_after_a_batch_goes_on_where_it_stopped`.
5. An old session reads as exactly as a new one: the same counts, answer
   and attention (rule 5):
   `read.rs::an_old_session_reads_exactly_once_its_facts_are_filled`.
6. Streamed text arrives as appends, and the stream sends one snapshot and
   then consecutive revisions (rules 19, 45):
   `fold.rs::streamed_text_arrives_as_appends` and
   `tests/it/trajectory.rs::the_stream_sends_a_snapshot_then_updates_and_text_as_appends`.
7. A streamed run keeps its `item_id` across opens and after it ends, in the
   stream and in the steps route; a run with no `item_id` takes its whole's
   id when it ends (rule 20):
   `tests/it/trajectory.rs::a_streamed_run_keeps_its_id_across_opens_and_ends_on_its_stored_id`
   and `fold.rs::a_run_without_an_item_id_takes_its_wholes_id_when_it_ends`.
8. An `item_id` names one item (rule 18):
   `fold.rs::an_item_id_names_the_message_its_chunks_and_whole_belong_to`.
9. A closed turn carries its prompt, activity and answer, and no steps
   (rules 9, 13, 15, 17): `fold.rs::a_closed_turn_carries_its_prompt_activity_and_answer_and_no_steps`.
10. A running turn carries its last steps (rule 14):
   `fold.rs::a_running_turn_carries_its_last_steps`.
11. A call's preview, diff stat, subject and detail revision (rules 21, 22):
   `fold.rs::a_call_previews_its_output_and_counts_its_diff`,
   `read.rs::a_cold_read_keeps_the_fields_of_an_intermediate_update`,
   `read.rs::a_cold_read_holds_the_commands_the_agent_offered`.
12. Attention, permission answers and native questions (rules 16, 23, 24):
   `fold.rs::attention_keeps_what_waits_and_the_errors_of_closed_turns`.
13. Lifecycle notes, the prompt-less first turn and an interruption
    (rules 9, 11, 26): `fold.rs::lifecycle_events_are_notes_and_a_restart_interrupts_the_turn`.
14. A compaction (rule 27):
    `fold.rs::a_compaction_is_one_step_that_its_updates_change`.
15. Unknown kinds and payloads without the new fields (rule 29):
    `fold.rs::events_without_the_new_fields_and_unknown_kinds_fold`.
16. A subagent is a step of its parent's turn, and its steps are its lane
    (rules 30, 31, 43): `fold.rs::a_subagent_is_a_step_in_its_parents_turn_and_its_steps_are_its_lane`
    and `tests/it/trajectory.rs::a_subagent_is_a_step_of_its_parent_and_its_steps_are_a_query_away`.
17. Heads and full events count a turn the same (rule 34):
    `fold.rs::heads_and_whole_events_count_a_turn_the_same`.
18. A pruned turn keeps its open question for a later answer (rules 35, 47):
    `fold.rs::a_pruned_turn_keeps_its_open_question_for_a_later_answer`.
19. An open of many turns loads at most the budget, whole turns first
    (rules 36 to 38): `read.rs::the_open_of_a_long_session_reads_a_bounded_count_of_payloads`.
20. An oversized closed turn keeps its answer and exact activity
    (rules 17, 38): `read.rs::an_oversized_closed_turn_keeps_its_answer_and_exact_activity`.
21. A running turn's window is its last steps, whatever events follow them
    (rule 38): `read.rs::a_running_turns_window_is_its_last_steps_whatever_follows_them`.
22. A turn of thousands of events loads the budget at most (rule 36):
    `read.rs::a_turn_of_thousands_of_events_loads_the_budget_at_most`.
23. Attention outside the window keeps its turn, and a later answer finds
    it (rules 16, 35, 39): `read.rs::attention_outside_the_window_keeps_its_turn`.
24. A subagent opened before the window keeps its step and parent
    (rules 30, 40): `read.rs::a_subagent_opened_before_the_window_keeps_its_step_and_parent`.
25. A page before an interrupting prompt keeps the interruption (rule 42):
    `read.rs::a_page_before_an_interrupting_prompt_keeps_the_interruption`.
26. The steps route pages by its cursor and names a turn (rule 44):
    `read.rs::the_steps_route_pages_by_its_cursor_and_names_a_turn` and
    `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`.
27. A page of older whole turns, and a page of steps, over HTTP
    (rules 41, 42, 44):
    `tests/it/trajectory.rs::older_turns_and_the_steps_of_a_turn_come_in_pages`.
28. Two subscribers that open at once receive the same `rev` sequence; two
    cold subscribers share one slot; a subscriber that lags receives `resync`
    (rule 46): `tests/it/trajectory.rs::two_subscribers_receive_the_same_rev_sequence`,
    `http/trajectory.rs::two_cold_subscribers_share_one_slot` and
    `http/trajectory.rs::a_subscriber_that_lags_receives_resync`.
29. A remote session serves the routes through the forwarder (rule 48):
    `far_tasks/source_references.rs::source_trajectory_routes_reach_the_worker_session`.
30. `ui/openapi.json` and `ui/src/api/schema.d.ts` hold every trajectory
    type, every step kind and every op:
    `docs.rs::the_committed_api_pair_holds_every_trajectory_type`.

31. A diagnosis, a blob URL, a plan's id and the session's state
    (rules 25, 28, 32, 33): `fold.rs::a_diagnosis_reads_onto_the_error_it_names`,
    `::a_block_with_bytes_carries_a_blob_url_and_not_the_bytes`,
    `::a_plan_keeps_the_id_of_its_first_event` and
    `::the_session_state_follows_the_row_the_running_turn_and_the_commands`.
32. A read holds nothing above its bound (rule 37):
    `read.rs::a_read_holds_nothing_above_its_bound`.
33. A message whose `item_id` is not its event's id keeps its answer and
    pages by its place (rules 34, 44):
    `read.rs::a_message_with_its_own_item_id_reads_and_pages_by_it`.
34. A stored whole reads its chunks as one text (rule 28):
    `fold.rs::a_stored_whole_reads_its_chunks_as_one_text`.
35. A cold read names a plan by its first event (rules 32, 38):
    `read.rs::a_cold_read_names_a_plan_by_its_first_event`.
36. A turn of more than 20 errors holds the newest 20 from their payloads,
    counts the rest in `attention_more`, and the steps route returns them
    (rules 16, 25):
    `read.rs::a_turn_of_many_errors_holds_the_newest_twenty_and_counts_the_rest`.
37. The running turn's window comes before the subagents (rule 38):
    `read.rs::the_running_turns_window_comes_before_the_subagents`.
38. A question behind more than twenty failed turns stays in the read
    (rule 39): `read.rs::a_question_behind_many_failed_turns_stays_in_the_read`.
39. A subagent query chooses the turns of its lane (rule 43):
    `read.rs::a_subagent_query_chooses_the_turns_of_its_lane`.
40. A page of steps loads the budget at most (rules 36, 44):
    `read.rs::a_page_of_steps_loads_the_budget_at_most`.
41. A query for a subagent the session never had is empty (rule 43):
    `read.rs::a_query_for_a_subagent_the_session_never_had_is_empty`.
42. A page of an interrupted turn shows its call interrupted (rule 44):
    `read.rs::a_page_of_an_interrupted_turn_shows_its_call_interrupted`.
43. Waiting questions past the read limit are counted in `attention_more`
    and paged by the steps route (rules 16, 38):
    `read.rs::waiting_questions_past_the_limit_are_counted_and_paged`.
44. A chunk after its whole changes nothing (rule 19):
    `fold.rs::a_chunk_after_its_whole_changes_nothing`.
45. A warm fold and a cold read hold the same `attention` and
    `attention_more`, and a `turn_upsert` carries them (rule 16):
    `read.rs::a_warm_fold_and_a_cold_read_hold_the_same_attention`.
46. An answer after a cold read refills the attention window as a new cold
    read holds it (rule 16):
    `read.rs::an_answer_refills_the_attention_window_as_a_cold_read_holds_it`.
47. Each shown turn holds its whole attention, errors included, within the
    cap (rule 38):
    `read.rs::each_shown_turn_holds_its_whole_attention_within_the_cap`.
48. An answer and the refill it calls for are one revision, over HTTP
    (rule 16):
    `tests/it/trajectory.rs::an_answer_and_the_refill_it_calls_for_are_one_revision`.
49. A failed refill leaves its rows to load again (rule 16):
    `http/trajectory.rs::a_failed_refill_leaves_its_rows_to_load_again`.

50. A plan event names its plan in its facts, and the upgrade fills the
    facts of the plan events stored before it (rules 3, 6):
    `event_facts.rs::a_plan_update_and_a_removed_plan_name_their_plan` and
    `plan_facts_migration.rs::the_upgrade_fills_the_plan_facts_of_older_plan_events`.
51. The updates of one `plan_id` change one plan in place, an old-form
    `plan` is one fixed plan, a `plan_removed` marks its plan, and a fold
    built again names each plan the same (rules 32, 38):
    `trajectory_acp.rs::the_updates_of_a_plan_change_it_in_place_and_a_removed_plan_is_marked`.
52. A notice is a `note` with its severity, a compaction is one step with
    its status and summary, and a fold built again gives each the same id
    (rules 26, 27):
    `trajectory_acp.rs::a_notice_is_a_note_and_a_compaction_is_one_step_with_its_status_and_summary`.
53. A message with an image gives an `image` block with a blob URL and no
    bytes (rule 28):
    `trajectory_acp.rs::a_message_with_an_image_gives_an_image_block_with_a_blob_url`.
54. A new open shows a running call's newest stored progress, and a
    compaction's newest update (rules 21, 22, 27):
    `trajectory_acp.rs::a_new_open_shows_the_newest_stored_progress_of_a_running_call`.
55. A `tool_call_abandoned` ends its call `interrupted` or `cancelled`
    (rule 21): `trajectory_acp.rs::an_abandoned_call_ends_interrupted_with_its_turn`
    and `::an_abandoned_call_ends_cancelled_with_a_cancelled_turn`.
56. The session state holds the mode, the config options, the title, the
    cost and the commands, and a change is a `session_patch` (rule 33):
    `trajectory_acp.rs::the_session_state_holds_what_the_agent_reports_and_changes_by_patch`
    and `fold.rs::a_display_change_is_a_session_patch_and_a_stored_list_yields_to_it`.
57. The header of a closed turn is exact from its heads, with notices, a
    compaction, a plan and an abandoned call in it (rules 17, 34, 38):
    `trajectory_acp.rs::the_header_of_a_closed_turn_stays_exact`. A turn of
    thousands of events takes the same path:
    `read.rs::a_turn_of_thousands_of_events_loads_the_budget_at_most`.
58. A plan of each kind folds by its lane and `plan_id`
    (rule 32): `fold.rs::a_plan_keeps_the_id_of_its_first_event`.

Rules 10 and 12 hold in the tests above: every fold test puts its events in
the running turn, and
`read.rs::a_running_turns_window_is_its_last_steps_whatever_follows_them`
and `fold.rs::heads_and_whole_events_count_a_turn_the_same` end turns with
their calls.

## Known gap

An agent may hold more questions open at once than a read loads
(`acp_questions.rs` allows it). The read shows the newest that fit and
counts the rest in `attention_more` (rule 16); the steps route shows them.

A diagnosis of an error older than the 20 turns with an error that a hub
holds changes no step the stream sends. A page that holds the error shows the
diagnosis.

The steps route reads the stored events. A message that still streams is in
the stream, and not yet in a page.

A page with `before` holds the subagents and plans of its own read.

## Sources

`crates/ariadne-api/src/trajectory.rs`,
`crates/ariadne-core/src/event_facts.rs`,
`crates/ariadne-store/src/events.rs`,
`crates/ariadne-store/migrations/0031_agent_event_facts.sql`,
`crates/ariadne-store/src/lib.rs` (`Store::open`),
`crates/ariadne-daemon/src/trajectory/`,
`crates/ariadne-daemon/src/http/trajectory.rs`,
`crates/ariadne-daemon/src/http/console.rs` (`Merge`, `open_console`),
`crates/ariadne-daemon/src/http/source_sessions.rs`, and the two folds it
replaces for a client: `crates/ariadne-console/src/transcript.rs` and
`ui/src/features/sessions/console/transcript.ts`.
