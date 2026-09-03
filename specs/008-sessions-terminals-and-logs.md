---
id: sessions-terminals-and-logs
status: current
updated: 2026-09-04
areas: [daemon, store]
commits: [e4816cf6, 39937143]
tests:
  - crates/ariadne-daemon/tests/session_logs.rs
  - crates/ariadne-daemon/tests/session_input.rs
  - crates/ariadne-daemon/tests/session_resize.rs
  - crates/ariadne-daemon/tests/managers.rs
  - crates/ariadne-daemon/tests/keystroke_delivery.rs
---

# Sessions, terminals and logs

An agent session is a tmux pane the daemon owns. This is what may be done to
that pane, and how its output reaches a client.

## Scope

In: the session row and its statuses, tmux session naming and lifecycle,
reading a pane as a live log stream, typing into a pane, resizing it, and
confirmed keystroke delivery.

Out: when the daemon decides to type something (009, 010), and what it types
(006).

## Behavior

1. A session belongs to a goal, a role and — for engineers and reviewers — a
   task, and holds the tmux session name, the worktree, the agent kind, the
   model and effort it runs on, and its internal agent id.
2. Sessions are long-lived: one engineer per task, one reviewer per task
   across its rounds, one planner per goal. Restarting one reopens the same
   row, and every launch of it is dated.
3. A launch is refused rather than duplicated: a spawn asks first whether the
   role already has a live session, and counts "tmux could not be asked" as a
   yes — a wrong no would put two agents on one piece of work.
4. tmux session names are stable and short, derived from the goal, the task
   and the role.
5. A session's output is served as a log: a snapshot of the grid the pane
   draws against, then deltas as it writes. A resize under the stream is
   reported as a new grid, and output in flight is replaced rather than
   reordered.
6. Output is withheld once the pane stops answering, and tmux being
   unreachable does not end a session. A pane that cannot be measured or
   captured is not reported as finished.
7. An exited session serves its full log and then ends the stream, and ignores
   a pane that later took over its name.
8. A client may type into a live pane. The bytes reach the pane verbatim,
   control bytes included, and a long paste is split into ordered batches. A
   finished session, or one with no pane, refuses input.
9. Typing into a pane takes down whatever the session was flagged for: an
   answer is an answer, whoever gave it.
10. A client may resize a pane within bounds; a size outside them is rejected
    before tmux sees it, and a finished or pane-less session refuses.
11. Everything the daemon types is delivered under confirmation: the pane is
    read back, and an Enter that was swallowed is pressed again until the
    message goes. A message that never submits is never called delivered, and
    an instruction still sitting in the composer raises the session.

## Acceptance criteria

- A live stream opens with the grid the pane draws against
  (`session_logs.rs::a_live_stream_opens_with_the_grid_the_pane_draws_against`) and
  follows with deltas (`::output_reaches_the_client_as_soon_as_it_is_written`,
  `::a_burst_bigger_than_one_frame_arrives_as_several_deltas`).
- A resize is reported, and in-flight output is replaced rather than reordered
  (`session_logs.rs::a_pane_resized_under_the_stream_reports_its_new_grid`,
  `::output_in_flight_when_the_pane_resizes_is_replaced_rather_than_reordered`).
- tmux being unreachable does not end a session
  (`session_logs.rs::tmux_being_unreachable_does_not_end_a_session`), and a pane
  that cannot be measured or captured is not reported as finished
  (`::a_pane_that_cannot_be_measured_is_not_reported_as_a_finished_session`,
  `::a_pane_that_cannot_be_captured_is_not_reported_as_a_finished_session`).
- Typing reaches the pane byte for byte
  (`session_input.rs::typing_reaches_the_pane_byte_for_byte`), a long paste is
  split into ordered batches (`::a_long_paste_is_split_into_ordered_batches`),
  and a finished or pane-less session refuses
  (`::a_finished_session_refuses_input`, `::a_session_without_a_pane_refuses_input`).
- Typing into a pane takes down what the session was flagged for
  (`session_input.rs::typing_into_a_pane_takes_down_what_the_session_was_flagged_for`).
- A resize sizes the window and leaves a client free to resize again
  (`session_resize.rs::a_resize_sizes_the_window_and_leaves_a_client_free_to_resize_it_again`);
  a size outside the bounds is rejected before tmux sees it
  (`::a_size_outside_the_bounds_is_rejected_before_tmux_sees_it`).
- A swallowed Enter is pressed again until the message goes
  (`keystroke_delivery.rs::a_swallowed_enter_is_pressed_again_until_the_message_goes`),
  and a message that never submits is never called delivered
  (`::a_message_that_never_submits_is_never_called_delivered`).
- Session names are stable and short
  (`managers.rs::session_names_are_stable_and_short`); the tmux lifecycle and a
  plan no command line could carry are covered
  (`::tmux_session_lifecycle`, `::tmux_runs_a_plan_no_command_line_could_carry`).
- Restarting a session reopens the same row
  (`store.rs::restarting_a_session_reopens_the_same_row`), and every launch is
  dated (`::every_launch_of_a_session_is_dated`).

## Sources

`crates/ariadne-daemon/src/tmux.rs`, `crates/ariadne-daemon/src/log/`,
`crates/ariadne-daemon/src/http/session_logs.rs`,
`crates/ariadne-daemon/src/http/pane.rs`,
`crates/ariadne-daemon/src/scheduler/delivery.rs`.
