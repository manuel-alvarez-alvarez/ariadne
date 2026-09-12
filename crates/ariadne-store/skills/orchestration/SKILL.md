---
name: orchestration
description: Plan a goal into tracer tasks with the user, then run it to the end. Use when a goal opens, a plan changes, or a task reports back.
---

# Orchestration

A tracer task cuts one complete path through every layer. The plan is a
conversation with the user.

## Steps

1. Read the goal. Explore its repositories.
   Done when you can name each repository the goal touches.
2. Ask the user about every unclear point, until nothing about the goal is
   open. Write one question in your turn text.
   Wait for the answer in the console.
   Done when no point of the goal is open.
3. Split the goal into tasks: small, finishable alone, one repository. Cut
   each task as a tracer: one complete path a reviewer verifies alone.
   Where a wide refactor breaks every tracer, plan expand-contract. Expand
   the new form first, migrate the call sites in batches, contract the old
   form last. Give each batch one task, gated by the expand. Write each
   ticket in STE: context, what to do, what not to touch, acceptance
   criteria. Add `depends_on` only for a real dependency. The rest run
   together: keep them off the same code.
   Done when each task states one path and its acceptance criteria.
4. Staff the authors of each task with `create_task`. Most tasks take one
   author. Where a task is hard, staff several authors, each on a different
   model. Each author then writes the task alone, and the reviewers pick the
   one change that lands. A task with several authors needs at least one
   reviewer. Give each agent the skills its work needs (`list_skills`). It
   knows only its task and its skills.
   Done when every task carries at least one author.
5. Ask the user which tasks are worth a review, and what each review is
   for. Staff those reviewers. Staff none on the rest.
   Done when every task carries a review answer.
6. Ask the user how each task ends. `merge` puts it on the base branch.
   `pull_request` opens a request and sees it through. `none` lands nothing.
   Done when every task carries one ending.
7. Give each agent one model from `list_models`. Size it from the model's
   description and the task, and the effort from what each effort buys.
   Give a top effort only where the task earns it.
   Mix the agents evenly over the tasks.
   Take only an agent that suits the task. Show the user what each agent runs
   on and take the model they name instead.
   Done when every agent carries one model.
8. Show the user the tasks you wrote. Ask whether the tracers are too coarse
   or too fine. Ask whether each `depends_on` edge is a true gate.
   Revise them until they write an explicit yes.
   Done when the user writes that yes.
9. Call `finalize_plan`. It starts every task and ends planning.
   Call it no earlier.
10. Stay up for the rest of the goal. Answer the user, and `send_message` to
    answer an agent that asks you. Ariadne wakes you when a task fails,
    stalls or finishes. Call `complete_goal` once every task is done.

## Do not tell yourself

- "The goal is clear enough." -> Ask; a wrong plan costs every task.
- "One task per layer is tidier." -> Cut a tracer; a layer proves nothing.
- "The user agrees with this plan." -> A written yes is the only agreement.

## Done

Every task is a tracer with acceptance criteria. The user wrote an explicit
yes. Every task ran to its end. `complete_goal` closed the goal.
