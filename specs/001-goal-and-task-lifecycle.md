---
id: goal-and-task-lifecycle
status: current
updated: 2026-10-10
areas: [core, store, daemon]
commits: [e4816cf6, c98b83da, ad268ee0, 7bcb30a0, 94486b02, a69b953f, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-core/src/state_machine.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/scheduler_dependencies.rs
  - crates/ariadne-daemon/tests/it/task_failure.rs
  - crates/ariadne-daemon/tests/it/goal_delete.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
---

# Goal and task lifecycle

What a goal and a task are, the states they move through, and who may move
them. Every other spec assumes this vocabulary.

Every goal runs on a workflow. [030](030-workflows.md) settles the columns, the
staffing and the step moves inside `in_progress`.

## Scope

In: goal statuses, task statuses, the transition table and its actors, the
audit trail, dependencies, cancellation, failure and retry, and what deleting
a goal takes with it.

Out: how each state is *worked* — planning (003), the columns of the workflow
(030) — and how sessions are started for it (008, 009).

## Behavior

1. A goal is `planning`, `active`, `completed` or `cancelled`. `completed`
   and `cancelled` are terminal.
2. A goal opens in `planning` with one orchestrator session and nothing else
   running. `finalize_plan` moves it to `active` and starts every task at
   once (003). Resuming an outside session creates no goal or task (020).
3. A goal is `completed` when its orchestrator or the user says so
   (`complete_goal`, 003) — refused while any task is not terminal — and
   `cancelled` when the user cancels it. Cancelling records the reason on
   every task it takes with it. Whether the goal is *met* is a judgement
   about the work, so the daemon never makes it on anybody's behalf.
4. A task is `pending`, `ready`, `in_progress`, `finished`, `cancelled` or
   `failed`. `finished` and `cancelled` are terminal; `failed` is retryable.
   A task is `in_progress` from its first column to its last: the column it
   is on is the task's `step`, not a status (030).
5. Every status change is checked against one transition table
   (`ariadne_core::state_machine`), which names both the move and the actor
   allowed to make it — `orchestrator`, `agent`, `daemon`, `user`. The legal
   moves are:
   - `pending → ready` (daemon), when every dependency has finished
   - `ready → pending` (orchestrator, daemon), when dependencies are added
     back, and (daemon) when a retried task has a dependency that has not
     finished: it waits for it as any pending task does, so a task failed by
     its dependency is retried with that dependency rather than cancelled
     and created again
   - `ready → in_progress` (daemon), when the first column's agent starts
   - `in_progress → finished` (agent, daemon), through `complete_step` on
     the last column, or by the daemon once a merged request's column is
     last and its agent fell quiet (030)
   - `failed → ready` (user, orchestrator), which is a retry
6. Two blanket rules sit above that table: the **user** and the
   **orchestrator** cancel a task, and only the **daemon** or one of the
   task's own **agents** fails one. Cancellation is refused only after a task
   is finished or cancelled. The orchestrator has both because the daemon wakes it when
   a task fails, and retrying or giving up is the answer it is woken for
   (003).
7. A refused transition is answered with a sentence naming what would have
   worked, in the API's own status vocabulary, not with a type name.
8. The store validates the transition and writes the audit row in one
   transaction: an illegal transition changes nothing and records nothing.
   No status moves to itself, so two writers that race to the same status
   leave one of them refused.
9. A failed or cancelled task carries its ending reason, which `ariadne task inspect` shows.
10. Dependencies are declared per task and gate `pending → ready`. Cycles are
    refused. A dependency that ends unmerged is reported as blocking, and a
    dependency that failed or was cancelled fails the task waiting on it.
11. A goal carries no numbers about its plan. How many tasks it takes is
    settled between the orchestrator and the user (003); how each task is
    checked is settled by the columns of the goal's workflow (030). A cap
    written down before that conversation could only refuse a plan they had
    already agreed.
12. A retry puts a failed task back on its first column. The daemon refuses
    the retry of a task with a column nobody staffs, and names the column
    (030); the orchestrator staffs it with `update_task` first. A task the
    scheduler finds ready with a column nobody staffs fails before it
    starts, naming the column the same way.
13. A terminal goal can be deleted, and deleting it takes its tasks,
    sessions, events and usage rows with it.

## Acceptance criteria

- Every `(from, to, actor)` triple of the six statuses and four actors is
  legal or refused exactly as the table says — checked against a second
  reading of the table (`state_machine.rs::exhaustive_transition_table`),
  and a refusal names the command that was refused in status words
  (`state_machine.rs::human_messages_name_the_command_that_was_refused`,
  `::human_messages_never_leak_pascal_case`). Terminal states are frozen
  (`::terminal_states_are_frozen`), an agent fails the task it works on
  (`::an_agent_may_fail_the_task_it_works_on`), and a step move reaches only
  an adjacent column (`::step_moves_only_reach_adjacent_columns`).
- An illegal transition leaves no audit row
  (`store.rs::illegal_transitions_are_rejected_and_unaudited`).
- A task walks `pending → ready → in_progress → finished` through the store
  (`store.rs::task_happy_path_to_finished`).
- Dependencies gate a task and cycles are refused
  (`store.rs::dependencies_gate_and_reject_cycles`); adding them to a ready
  task downgrades it with an audit row
  (`store.rs::setting_the_dependencies_of_a_ready_task_downgrades_it_with_audit`).
- A failed or cancelled dependency fails its dependent
  (`scheduler_dependencies.rs::a_failed_dependency_fails_the_task_waiting_on_it`,
  `::a_cancelled_dependency_fails_the_task_waiting_on_it`), and a task
  retried after that dependency landed is not failed again
  (`::a_task_retried_after_its_dependency_landed_is_not_failed_again`); one
  retried before that dependency finished waits for it, then starts
  (`::a_task_retried_before_its_dependency_finished_waits_for_it`).
- Cancelling a goal leaves every task cancelled and none failed
  (`scheduler_dependencies.rs::cancelling_the_goal_leaves_every_task_cancelled_and_none_failed`).
- A column's agent fails its own task with the reason on it, the
  orchestrator may not, and a task that has not ended carries no reason
  (`task_failure.rs::a_column_agent_fails_its_own_task_with_the_reason_on_it`,
  `::the_orchestrator_may_not_fail_a_task_and_the_task_stays_where_it_was`,
  `::a_task_that_has_not_ended_carries_no_reason`).
- A retry of a task with an unstaffed column is refused by the column's name,
  and a ready task with one fails before it starts
  (`workflow_steps.rs::a_retry_refuses_a_task_with_an_unstaffed_column_by_name`,
  `::a_runnable_task_with_an_unstaffed_column_fails_before_it_starts`).
- A goal takes as many tasks as its plan calls for
  (`store.rs::a_goal_takes_as_many_tasks_as_its_plan_calls_for`).
- An unfinished goal is refused deletion and keeps everything
  (`goal_delete.rs::an_unfinished_goal_is_refused_and_keeps_everything`); a
  finished one takes its children and reaches the event stream
  (`::deleting_a_finished_goal_takes_its_children_and_reaches_the_stream`).

## Sources

`crates/ariadne-core/src/state_machine.rs` (the authority),
`crates/ariadne-core/src/lib.rs` (`GoalStatus`, `TaskStatus`, `Seat`),
`crates/ariadne-store/src/tasks.rs`, `crates/ariadne-daemon/src/scheduler/`.
