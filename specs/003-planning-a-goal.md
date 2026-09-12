---
id: planning-a-goal
status: current
updated: 2026-09-12
areas: [prompts, daemon, mcp]
commits: [d421e30b, fdd0c5b6, 09955c22, 305ad2fb, 7bcb30a0, 31bb7611, 29e6d84e, 1b09ac10, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/plan_finalize.rs
  - crates/ariadne-daemon/tests/goal_completion.rs
---

# Planning a goal

How a goal becomes a plan of tasks, and what the orchestrator does for the
rest of that goal. It is the one agent type Ariadne defines (017), the one
seat that talks to the user, and the one that outlives its own hand-off.

## Scope

In: the orchestrator session, the conversation that removes the uncertainty
from a goal, writing and staffing the tasks, `finalize_plan`, what the
orchestrator is woken for afterwards, and `complete_goal`.

Out: the landing procedures themselves (005), the model catalog the sizing
reads (011), the skills the staffing names (017), and the MCP tools' shapes
(013).

## Behavior

1. A planned goal opens with one orchestrator session, started in the primary
   checkout of the goal's first repository and briefed with the goal and its
   repositories. No numbers: how many tasks the goal takes is what the
   conversation settles. Outside-session adoption instead opens an active,
   unorchestrated goal whose user completes or cancels it (020)
   (`acp_session_adoption.rs::adoption_into_a_new_goal_creates_and_loads_the_author_without_an_orchestrator`).
2. The orchestrator never writes code. Its whole output is the plan.
3. It asks the user about every unclear point, until nothing about the goal is
   open: one question in plain turn text, then it waits. The user answers in
   the session's terminal (`ariadne goal attach`). An orchestrator waiting on
   an answer is not nudged and shows up wherever Ariadne lists what needs
   attention.
4. It writes one task per unit of work: small, finishable alone, one
   repository. Each ticket carries context, what to do, what not to touch and
   acceptance criteria, in Simplified Technical English (006).
5. `depends_on` is for real dependencies only; tasks that merely touch nearby
   files run together.
6. It staffs one author per task, on the skills that work needs (017), and
   gives every agent one model from the catalog (011) — a model is required,
   so no agent is staffed without one. Across a plan it spreads the agents
   evenly over the tasks, and never onto a task the agent does not suit:
   fit is the first question, and an even mix is the second.
7. Four things are settled with the user rather than decided alone, because
   each is a judgement about the work and not about the code:
   - what the goal actually asks for (3);
   - which tasks are worth a review, and what each review is for — a task
     with nothing to review is staffed with no reviewer (017);
   - how each task ends: `merge`, `pull_request` or `none` (005);
   - what each agent runs on. The orchestrator sizes every one of them from
     the catalog (011) and shows the user what it chose; the model the user
     names instead is the one that is staffed.
8. It writes the tasks into Ariadne before it asks for the yes, not after.
   So what the user is shown is the tasks themselves — to read, and to edit —
   and the yes is given to a plan that already exists. It revises them until
   that yes is explicit.
9. It writes no specification of its own. Where a goal wants one, that is a
   task like any other, staffed with `spec-writing` and reviewed with
   `spec-review`.
10. `finalize_plan` ends planning: it moves the goal to `active` and starts
    every task at once. Only the goal's orchestrator may call it, only out of
    `planning`, and never on a plan with no tasks.
11. Nothing runs while the goal is in `planning`. A task created there stays
    `pending` however often the scheduler is woken about it: the daemon
    reconciles the tasks of active goals alone. That is what makes 8 safe —
    the plan can be written in full, and read, before anybody agrees to it.
12. The plan is a hand-off, not an ending. The orchestrator stays up for the
    rest of the goal: it is what the user talks to about work already running.
    Nothing is sent to it for the hand-off itself.
13. The daemon wakes it when its tasks need a decision no author can make: a
    task that failed, a task that has gone quiet, or a goal with nothing left
    to do. Once per situation, as a prompt of its own. Work in progress is
    what the orchestrator delegated, and it is not woken for that.
14. It is also open to the agents themselves. Any of them can write to it
    about anything it needs to know, and the message arrives as a turn
    (018). It acts on what it is told — the plan is its to change — rather
    than writing back.
15. It answers with `list_tasks`, and then with `retry_task`, `cancel_task`,
    `update_task` or nothing at all.
16. `complete_goal` ends the goal. Whether the goal is *met* is a judgement
    about the work, so the daemon does not make it — but it refuses the call
    while any task is still going, which is the part it can see. The user may
    make the same call, so a goal whose orchestrator will not start is still
    one they can close.

## Acceptance criteria

- The playbook asks before it plans and plans before it starts
  (`defaults.rs::the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts`),
  and it staffs a plan on a mix of agents rather than on one
  (`defaults.rs::the_orchestrator_staffs_a_plan_on_a_mix_of_agents`).
- The orchestrator is briefed to end planning with `finalize_plan` and with no
  other plan call
  (`defaults.rs::the_orchestrator_is_briefed_with_finalize_plan_and_no_other_plan_call`).
- The nudge fits the conversation and the goal under way
  (`defaults.rs::the_orchestrator_nudge_fits_the_conversation_and_the_goal_under_way`).
- The orchestrator's own texts name no forge and no landing procedure
  (`defaults.rs::the_orchestrator_is_told_nothing_of_forges_or_landing`), and
  its briefing names every repository with its base branch
  (`prompts.rs::the_orchestrator_is_briefed_with_every_repository_and_its_base_branch`).
- The tasks are written before the yes and start only with it
  (`plan_finalize.rs::the_tasks_of_a_plan_wait_for_the_yes_that_finalizes_it`).
- `finalize_plan` starts every task
  (`plan_finalize.rs::the_orchestrator_finalizes_the_plan_and_its_tasks_start`),
  only the orchestrator may call it
  (`::only_the_orchestrator_may_finalize_the_plan`), never with no tasks
  (`::a_plan_with_no_tasks_cannot_be_finalized`), and only out of planning
  (`::a_plan_is_finalized_only_out_of_planning`).
- Work waits on the orchestrator until the goal is over
  (`plan_finalize.rs::an_orchestrator_is_the_agent_work_waits_on_until_the_goal_is_over`),
  and it is kept and left alone rather than let go
  (`::a_scheduler_pass_keeps_the_orchestrator_of_an_active_goal_and_types_nothing`,
  `scheduler_attention.rs::an_idle_orchestrator_stays_up_for_the_whole_goal`).
- A failed task wakes it once (`scheduler_attention.rs::a_failed_task_wakes_the_orchestrator_once`),
  a goal with nothing left to do wakes it too
  (`::a_goal_whose_tasks_all_landed_wakes_its_orchestrator`), and running work
  does not (`::a_goal_whose_tasks_are_running_leaves_its_orchestrator_alone`).
- The orchestrator completes the goal it planned
  (`goal_completion.rs::the_orchestrator_completes_the_goal_it_planned`), so
  does the user (`::the_user_completes_a_goal_of_their_own`), an agent below
  the orchestrator may not
  (`::an_agent_below_the_orchestrator_may_not_complete_the_goal`), a task
  still going refuses it by name
  (`::a_goal_with_a_task_still_going_is_refused_by_name`), a cancelled task is
  no bar (`::a_cancelled_task_is_no_bar_to_completing_the_goal`), and it
  happens once (`::a_goal_is_completed_only_out_of_active`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`ORCHESTRATOR_SYSTEM_PROMPT`,
`ORCHESTRATOR_BRIEFING`, `ORCHESTRATOR_RESUME`, `GOAL_ATTENTION`),
`crates/ariadne-daemon/src/agents/prompts.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-daemon/src/scheduler/tasks.rs` (the guard behind 11),
`crates/ariadne-daemon/src/http/goals.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`.
