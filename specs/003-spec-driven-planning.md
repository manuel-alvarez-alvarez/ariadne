---
id: spec-driven-planning
status: current
updated: 2026-09-04
areas: [prompts, daemon, mcp]
commits: [d421e30b, fdd0c5b6, 09955c22, 305ad2fb, 7bcb30a0, 31bb7611]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/plan_finalize.rs
  - crates/ariadne-daemon/tests/profile_system_prompt.rs
---

# Spec-driven planning

How a goal becomes an approved written spec, then a plan of tasks. The
planner is the one role that talks to the user, and the one that lands a
change without owning a task.

## Scope

In: the planner session, the spec conversation, the spec's folder and format,
landing the spec, writing the tasks, sizing each slot, and `finalize_plan`.

Out: the landing procedures themselves (005), the model catalog the sizing
reads (011), and the MCP tools' shapes (013).

## Behavior

1. A goal opens with one planner session, started in the primary checkout of
   the goal's first repository, briefed with the goal, its repositories,
   `max_tasks`, `required_approvals` and the procedure for landing a spec.
2. The planner drafts a spec from the goal — scope, behavior, acceptance
   criteria — and never writes code.
3. It asks the user about each unclear point: one question in plain turn text,
   then it waits. The user answers in the session's terminal
   (`ariadne goal attach`). A planner waiting on an answer is not nudged and
   shows up wherever Ariadne lists what needs attention.
4. It revises after each answer and asks again until the user writes an
   explicit yes. No task is created before that yes.
5. The spec file goes in the folder the repository keeps specs or docs in, and
   follows the format of the specs already there. Where the repository keeps
   none, the planner agrees **both a path and a format** with the user before
   writing the first one.
6. The planner then lands the spec itself, with the procedure of its
   repository's merge strategy (005), and creates no task until it merges. So
   every engineer branches off a base that already carries the spec.
7. It writes one task per unit of work: small, mergeable alone, one
   repository. Each ticket carries context, what to do, what not to touch and
   acceptance criteria, in Simplified Technical English (006), and names the
   merged spec path.
8. `depends_on` is for real dependencies only; tasks that merely touch nearby
   files run together.
9. The planner names an engineer profile and one or more reviewer profiles per
   task, and sizes the model and effort of each slot from the model catalog
   (011). The user's later choice overrides it.
10. `finalize_plan` ends planning: it moves the goal to `active` and starts
    every task at once. Only the goal's planner may call it, only out of
    `planning`, and never on a plan with no tasks.
11. Nothing runs while the goal is in `planning`, so the user can read the
    tasks and edit them before the work starts.
12. The planner's work ends with the plan: once the goal is past `planning`,
    its idle session is compacted (010) and then let go.

## Acceptance criteria

- The playbook orders its phases: read the goal, draft, ask, wait for an
  explicit yes, find the folder and format, land the spec, then the tasks
  (`defaults.rs::the_planner_playbook_orders_the_spec_phases_before_the_tasks`).
- The yes gates the tasks, in as many words: "Create no task before it."
  (same test).
- A planner nobody has edited is briefed with all of it
  (`profile_system_prompt.rs::a_planner_on_the_default_prompt_is_briefed_to_write_a_spec_and_size_its_slots`).
- The nudge fits whichever phase the goal stands in
  (`defaults.rs::the_planner_nudge_fits_the_spec_conversation_and_the_breakdown`).
- The planner's own texts name no forge and no merge strategy; the procedure
  reaches it as a rendered value
  (`defaults.rs::the_planner_is_told_nothing_of_forges_or_landing`,
  `prompts.rs::the_planner_lands_the_spec_the_way_its_repository_takes_a_change`).
- `finalize_plan` starts every task
  (`plan_finalize.rs::the_planner_finalizes_the_plan_and_its_tasks_start`),
  only the planner may call it (`::only_the_planner_may_finalize_the_plan`),
  never with no tasks (`::a_plan_with_no_tasks_cannot_be_finalized`), and only
  out of planning (`::a_plan_is_finalized_only_out_of_planning`).
- Work waits on the planner until it finalizes
  (`plan_finalize.rs::a_planner_is_the_agent_work_waits_on_until_it_finalizes`),
  and the idle planner of an active goal is ended
  (`::a_scheduler_pass_ends_the_idle_planner_of_an_active_goal`).

## Sources

`crates/ariadne-store/src/defaults.rs` (`PLANNER_SYSTEM_PROMPT`,
`PLANNER_BRIEFING`, `PLANNER_RESUME`, `SPEC_LANDING_*`),
`crates/ariadne-daemon/src/agents/prompts.rs`,
`crates/ariadne-daemon/src/scheduler/goals.rs`,
`crates/ariadne-cli/src/commands/mcp.rs`.
