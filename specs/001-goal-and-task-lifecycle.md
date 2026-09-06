---
id: goal-and-task-lifecycle
status: current
updated: 2026-09-06
areas: [core, store, daemon]
commits: [e4816cf6, c98b83da, ad268ee0, 7bcb30a0, 94486b02, a69b953f, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-core/src/state_machine.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/scheduler_dependencies.rs
  - crates/ariadne-daemon/tests/task_failure.rs
  - crates/ariadne-daemon/tests/goal_delete.rs
---

# Goal and task lifecycle

What a goal and a task are, the states they move through, and who may move
them. Every other spec assumes this vocabulary.

## Scope

In: goal statuses, task statuses, the transition table and its actors, the
audit trail, dependencies, cancellation, failure and retry, and
what deleting a goal takes with it.

Out: how each state is *worked* — planning (003), engineering and review
(004), landing (005) — and how sessions are started for it (008, 009).

## Behavior

1. A goal is `planning`, `active`, `completed` or `cancelled`. `completed`
   and `cancelled` are terminal.
2. A goal opens in `planning` with one orchestrator session and nothing else
   running. `finalize_plan` is what moves it to `active` and starts every
   task at once (003).
3. A goal is `completed` when its orchestrator or the user says so
   (`complete_goal`, 003) — refused while any task is still going — and
   `cancelled` when the user cancels it. Cancelling records the reason on
   every task it takes with it. Whether the goal is *met* is a judgement about
   the work, so the daemon never makes it on anybody's behalf.
4. A task is `pending`, `ready`, `in_progress`, `under_review`,
   `changes_requested`, `approved`, `finished`, `cancelled` or `failed`.
   `finished` and `cancelled` are terminal; `failed` is retryable by the user.
5. Every status change is checked against one transition table
   (`ariadne_core::state_machine`), which names both the move and the actor
   allowed to make it — `orchestrator`, `author`, `reviewer`, `daemon`, `user`.
   The legal moves are:
   - `pending → ready` (daemon), when every dependency has finished
   - `ready → pending` (orchestrator, daemon), when dependencies are added back
   - `ready → in_progress` (daemon), when the author session starts
   - `in_progress → under_review` (author), through `request_review`
   - `under_review → changes_requested` (daemon), on a change request
   - `under_review → approved` (daemon), once every reviewer staffed on the
     task has approved — none of them, for a task staffed with no reviewer.
     A verdict is a message of the kind that closes a round (018)
   - `changes_requested → in_progress` (daemon), when the author resumes
   - `approved → finished` (author), through `finish_task`
   - `approved → under_review` (author), when a published request is revised
   - `failed → ready` (user, orchestrator), which is a retry
6. Two blanket rules sit above that table: the **user** and the
   **orchestrator** cancel a task, and only the **daemon** or the task's own
   **author** fails one. Neither applies to a task that has already ended.
   The orchestrator has both because the daemon wakes it when a task fails,
   and retrying or giving up is the answer it is woken for (003).
7. A refused transition is answered with a sentence naming what would have
   worked, in the API's own status vocabulary, not with a type name.
8. The store validates the transition and writes the audit row in one
   transaction: an illegal transition changes nothing and records nothing.
9. A task ends carrying the reason its ending transition gave —
   `fail_task`'s text, or the cancellation's — and that reason is what
   `ariadne task inspect` shows.
10. Dependencies are declared per task and gate `pending → ready`. Cycles are
    refused. A dependency that ends unmerged is reported as blocking, and a
    dependency that failed or was cancelled fails the task waiting on it.
11. A goal carries no numbers about its plan. How many tasks it takes, and how
    many approvals each of them needs, are settled between the orchestrator
    and the user (003) — the second as however many reviewers the task was
    staffed with (017). A cap written down before that conversation could only
    refuse a plan they had already agreed.
12. A task also carries how it ends — `merge`, `pull_request` or `none` (005)
    — and all three reach `finished`.
13. Only a finished goal can be deleted, and deleting it takes its tasks,
    sessions, events and usage rows with it.

## Acceptance criteria

- Every `(from, to, actor)` triple is legal or refused exactly as the table
  says — checked against a second reading of the table
  (`state_machine.rs::exhaustive_transition_table`).
- An illegal transition leaves no audit row
  (`store.rs::illegal_transitions_are_rejected_and_unaudited`).
- A task walks `pending → … → finished` through the store
  (`store.rs::task_happy_path_to_merged`), and a task with no reviewer is
  approved as soon as its author asks
  (`unreviewed_tasks.rs::a_task_with_no_reviewer_is_approved_as_soon_as_its_author_asks`,
  `::a_task_needs_no_more_approvals_than_it_has_reviewers_to_give`).
- Dependencies gate a task and cycles are refused
  (`store.rs::dependencies_gate_and_reject_cycles`); adding them to a ready
  task downgrades it with an audit row
  (`store.rs::setting_the_dependencies_of_a_ready_task_downgrades_it_with_audit`).
- A failed or cancelled dependency fails its dependent
  (`scheduler_dependencies.rs::a_failed_dependency_fails_the_task_waiting_on_it`,
  `::a_cancelled_dependency_fails_the_task_waiting_on_it`), and a task
  retried after that dependency landed is not failed again
  (`::a_task_retried_after_its_dependency_landed_is_not_failed_again`).
- Cancelling a goal leaves every task cancelled and none failed
  (`scheduler_dependencies.rs::cancelling_the_goal_leaves_every_task_cancelled_and_none_failed`).
- An author fails its own task with the reason on it, and a reviewer may not
  (`task_failure.rs::an_author_fails_its_own_task_with_the_reason_on_it`,
  `::a_reviewer_may_not_fail_the_task_it_is_reviewing`).
- A goal takes as many tasks as its plan calls for
  (`store.rs::a_goal_takes_as_many_tasks_as_its_plan_calls_for`).
- An unfinished goal is refused deletion and keeps everything
  (`goal_delete.rs::an_unfinished_goal_is_refused_and_keeps_everything`); a
  finished one takes its children and reaches the event stream
  (`::deleting_a_finished_goal_takes_its_children_and_reaches_the_stream`).

## Sources

`crates/ariadne-core/src/state_machine.rs` (the authority),
`crates/ariadne-core/src/lib.rs` (`GoalStatus`),
`crates/ariadne-store/src/tasks.rs`, `crates/ariadne-daemon/src/scheduler/`.
