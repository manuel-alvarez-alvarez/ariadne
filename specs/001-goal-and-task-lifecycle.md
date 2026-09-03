---
id: goal-and-task-lifecycle
status: current
updated: 2026-09-04
areas: [core, store, daemon]
commits: [e4816cf6, c98b83da, ad268ee0, 7bcb30a0, 94486b02]
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
audit trail, dependencies, `max_tasks`, `required_approvals`, cancellation,
failure and retry, and what deleting a goal takes with it.

Out: how each state is *worked* — planning (003), engineering and review
(004), landing (005) — and how sessions are started for it (008, 009).

## Behavior

1. A goal is `planning`, `active`, `completed` or `cancelled`. `completed`
   and `cancelled` are terminal.
2. A goal opens in `planning` with one planner session and nothing else
   running. `finalize_plan` is what moves it to `active` and starts every
   task at once (003).
3. A goal is `completed` when every task it holds has landed, and
   `cancelled` when the user cancels it. Cancelling records the reason on
   every task it takes with it.
4. A task is `pending`, `ready`, `in_progress`, `under_review`,
   `changes_requested`, `approved`, `merged`, `cancelled` or `failed`.
   `merged` and `cancelled` are terminal; `failed` is retryable by the user.
5. Every status change is checked against one transition table
   (`ariadne_core::state_machine`), which names both the move and the actor
   allowed to make it — `planner`, `engineer`, `reviewer`, `daemon`, `user`.
   The legal moves are:
   - `pending → ready` (daemon), when every dependency has merged
   - `ready → pending` (planner, daemon), when dependencies are added back
   - `ready → in_progress` (daemon), when the engineer session starts
   - `in_progress → under_review` (engineer), through `request_review`
   - `under_review → changes_requested` (daemon), on a change request
   - `under_review → approved` (daemon), on enough approvals
   - `changes_requested → in_progress` (daemon), when the engineer resumes
   - `approved → merged` (engineer), through `mark_merged`
   - `approved → under_review` (engineer), when a published request is revised
   - `failed → ready` (user), which is a retry
6. Two blanket rules sit above that table: only the **user** cancels a task,
   and only the **daemon** or the task's own **engineer** fails one. Neither
   applies to a task that has already ended.
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
11. A goal may cap its task count (`max_tasks`) and sets how many approvals a
    task needs (`required_approvals`, default 1).
12. Only a finished goal can be deleted, and deleting it takes its tasks,
    sessions, events and usage rows with it.

## Acceptance criteria

- Every `(from, to, actor)` triple is legal or refused exactly as the table
  says — checked against a second reading of the table
  (`state_machine.rs::exhaustive_transition_table`).
- An illegal transition leaves no audit row
  (`store.rs::illegal_transitions_are_rejected_and_unaudited`).
- A task walks `pending → … → merged` through the store
  (`store.rs::task_happy_path_to_merged`).
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
- An engineer fails its own task with the reason on it, and a reviewer may not
  (`task_failure.rs::an_engineer_fails_its_own_task_with_the_reason_on_it`,
  `::a_reviewer_may_not_fail_the_task_it_is_reviewing`).
- `max_tasks` is enforced (`store.rs::max_tasks_is_enforced`).
- An unfinished goal is refused deletion and keeps everything
  (`goal_delete.rs::an_unfinished_goal_is_refused_and_keeps_everything`); a
  finished one takes its children and reaches the event stream
  (`::deleting_a_finished_goal_takes_its_children_and_reaches_the_stream`).

## Sources

`crates/ariadne-core/src/state_machine.rs` (the authority),
`crates/ariadne-core/src/lib.rs` (`GoalStatus`),
`crates/ariadne-store/src/tasks.rs`, `crates/ariadne-daemon/src/scheduler/`.
