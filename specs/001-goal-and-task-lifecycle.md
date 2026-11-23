---
id: goal-and-task-lifecycle
status: current
updated: 2026-11-23
areas: [core, store, daemon]
commits: [e4816cf6, c98b83da, ad268ee0, 7bcb30a0, 94486b02, a69b953f, 29e6d84e, 1b09ac10]
tests:
  - crates/ariadne-core/src/state_machine.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/scheduler_dependencies.rs
  - crates/ariadne-daemon/tests/it/task_failure.rs
  - crates/ariadne-daemon/tests/it/goal_delete.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
  - crates/ariadne-daemon/tests/it/linked_goals.rs
  - crates/ariadne-daemon/tests/it/far_tasks.rs
  - crates/ariadne-store/tests/far_task_migration.rs
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
2. A goal with a local orchestrator opens in `planning` with only that
   orchestrator session running. `finalize_plan` moves it to `active` and
   starts every task at once (003).
3. A goal is `completed` when its orchestrator or the user says so
   (`complete_goal`, 003) — refused while any task is not terminal — and
   `cancelled` when the user cancels it. Cancelling records the reason on
   every task it takes with it. Whether the goal is *met* is a judgement
   about the work, so the daemon never makes it on anybody's behalf.
   The check of the tasks and the write of `completed` are one write
   transaction. So a task that a linked goal admits at the same time (rule 15)
   either comes first and refuses the completion, or comes after and is refused.
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
14. `POST /v1/goals` accepts an optional `linked` object with a string
    `origin_goal_id`. This names the goal on the daemon that supplies the tasks.
    `GoalDto.linked` carries the same object, or null for other goals.
    OpenAPI describes the object on the request and the response.
    A linked goal skips planning (003 rule 17).
    A create whose origin goal has an `active` linked goal answers `200` with the
    oldest such goal, and adds to it each of `repository_ids` that it lacks. A create
    whose origin goal has only linked goals that ended answers `409`, names the last
    one, and makes no goal: an ended linked goal is not replaced. The lookup and the
    insert are one write transaction, so creates that overlap make one goal. An
    upgrade keeps every linked goal that an older daemon made, two active ones of
    one origin goal too.
15. The origin daemon holds each task until its dependencies merge.
    A linked goal refuses a nonempty `depends_on` list on task creation or edit.
    `CreateTaskRequest.origin_task_id` names the far task on the origin daemon that the
    task runs, and only a linked goal takes it. A daemon holds one task for each origin
    task, in whatever goal and status: a repeated create answers `200` with the task it
    holds, and starts nothing. A linked goal that is not `active` takes no new task: the
    create answers `409`. The check is in the write transaction of the insert. `GET /v1/tasks?origin_task_id=<id>` lists that task alone.
    A create with `origin_task_id` and a `host` other than `local` answers `400`.
16. A new task branch for a linked goal starts at the fetched remote base.
    The daemon fetches from the repository's forge remote, with `origin` as the default.
    The commit gate and the task diff compare against that fetched base, not the local base branch.
17. `CreateTaskRequest` takes an optional string `host`. An omitted host or `local` creates
    a task on the goal's own daemon, as before. Any other name creates a far task on that
    host (037, rules 95 to 100).
18. `TaskDto.host` is `local` for a task on the goal's own daemon, else the name of its host.
    `TaskDto.host_unreachable` is true while the host of a far task is not `ready` (037 rule 99).
    It is always false for a `local` task. OpenAPI carries both fields and `CreateTaskRequest.host`.
19. A far task stays `pending` until every task in its `depends_on` has merged, on any host.
    A far task merges when its host reports it `finished` (037 rule 98). A task that
    depends on a far task then starts as for any other dependency. A dependency that failed
    or was cancelled fails a pending far task before it goes to its host, as rule 10 fails
    any waiting task. A far task cancelled here, or in a goal that rules 3 and 4 ended,
    keeps that state whatever its host reports (037 rule 98).
20. A write to a sent far task goes to its host. The goal's daemon mirrors the
    result. The host of an unheld far task must be `ready`, even before dispatch;
    a refusal names the host and its status.
    A task still held by dependencies is changed only on the goal's daemon.
    A far agent's session can be switched on its host.
21. When a goal completes or is cancelled, its daemon sends the same ending
    to its linked goals. If a host is down, its worker closes the linked
    goals when that host next becomes `ready`.

## Acceptance criteria

- Rule 20: editing, retrying and cancelling a sent task reach its host
  (`far_tasks.rs::editing_a_sent_far_task_edits_it_on_its_host`,
  `::retrying_a_sent_far_task_retries_it_on_its_host`,
  `::cancelling_a_sent_far_task_cancels_it_on_its_host`). A down host is named
  (`::a_write_to_a_sent_far_task_refuses_a_down_host`,
  `::an_unsent_unheld_far_task_refuses_a_write_while_its_host_is_down`), while a task held by
  dependencies can be cancelled locally (`::a_dependency_held_far_task_can_be_cancelled_while_its_host_is_down`).
  The host switches its agent (`::a_far_agent_session_can_be_switched_on_its_host`).
- Rule 21: completion and cancellation close the linked goal
  (`far_tasks.rs::completing_a_goal_completes_its_linked_goal`,
  `::cancelling_a_goal_cancels_its_linked_goal`); a host that returns closes it
  then (`::a_cancelled_goal_closes_its_linked_goal_when_the_host_returns`).

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
- A linked goal keeps its origin ID in the goal API. An ordinary goal carries null
  (`linked_goals.rs::a_linked_goal_keeps_its_origin_id_in_the_goal_api`).
  OpenAPI describes the link on creation and inspection
  (`::the_openapi_schema_describes_linked_goals`).
- A linked goal refuses task dependencies on creation and edit
  (`linked_goals.rs::a_linked_goal_refuses_task_dependencies`,
  `::a_linked_task_refuses_dependencies_on_edit`).
- A linked task starts at the remote base after another host advances it
  (`linked_goals.rs::a_linked_task_starts_at_the_fetched_remote_base`).
- A linked task's diff holds only its own commits after another host advances the base
  (`linked_goals.rs::a_linked_task_compares_its_work_against_the_fetched_remote_base`).
- Rule 17: a task with no host or with `local` runs on the goal's own daemon
  (`far_tasks.rs::a_task_without_a_host_runs_on_the_goals_own_daemon`).
- Rule 18: a far task carries its host and clears `host_unreachable` while its host is
  `ready` (`far_tasks.rs::a_far_task_mirrors_the_state_its_host_reports`). OpenAPI carries
  both fields and the request's `host`
  (`far_tasks.rs::the_openapi_document_carries_the_host_of_a_task`).
- Rule 19: a far task waits for a far dependency and a local one, and goes to its host
  once both merged (`far_tasks.rs::a_far_task_waits_until_its_dependencies_merge_on_any_host`).
  A dependency that failed fails it, and it never goes to its host
  (`far_tasks.rs::a_far_task_fails_when_a_dependency_ends_unmerged`). A cancellation here holds
  against the host's updates and its read on return
  (`far_tasks.rs::a_far_task_cancelled_here_stays_cancelled_whatever_its_host_reports`).
- Rules 14 and 15: a repeated create of the linked goal and of its task, after a lost
  answer, makes no second goal or task
  (`far_tasks.rs::a_lost_answer_of_the_host_makes_no_second_goal_or_task`), and a repeated
  create of the goal adds the repository it lacks
  (`far_tasks.rs::a_repository_set_up_later_joins_the_same_linked_goal`). The task is found
  again after its linked goal ended
  (`far_tasks.rs::a_task_the_host_made_is_found_again_after_its_linked_goal_ended`). A host
  beside an origin task is refused
  (`far_tasks.rs::a_linked_task_takes_no_host_beside_its_origin_task`). Creates that overlap
  make one goal (`far_tasks.rs::linked_goal_creates_at_once_make_one_goal`). An ended linked
  goal is not replaced (`far_tasks.rs::a_linked_goal_that_ended_takes_no_more_tasks`), and takes
  no new task while it keeps its own
  (`far_tasks.rs::a_linked_goal_that_ended_takes_no_new_task_and_keeps_its_own`).
- Rules 3 and 15, the two orders: a task admitted first refuses the completion and names the
  task (`far_tasks.rs::a_linked_goal_with_an_admitted_task_cannot_complete`), and a completion
  first refuses the task (`::a_linked_goal_that_ended_takes_no_new_task_and_keeps_its_own`). An
  upgrade keeps two active linked goals of one origin goal, and a create answers the oldest
  (`crates/ariadne-store/tests/far_task_migration.rs::an_upgrade_keeps_two_active_linked_goals_of_one_origin`).

## Sources

`crates/ariadne-core/src/state_machine.rs` (the authority),
`crates/ariadne-core/src/lib.rs` (`GoalStatus`, `TaskStatus`, `Seat`),
`crates/ariadne-store/src/tasks.rs`, `crates/ariadne-daemon/src/scheduler/`.
