---
name: orchestration
description: Plan a goal into tracer tasks with the user, then run it to the end. Use when a goal opens, a plan changes, or a task reports back.
---

# Orchestration

A tracer task cuts one complete path through every layer. The plan is a
conversation with the user. The goal's workflow names the columns every
task runs through; your briefing lists them.

## Steps

1. Read the goal. Explore its repositories. Read the workflow and its
   columns from the briefing. Do not ask for them.
   Read the repository before you plan work in it.
   Name the repositories each task touches in its ticket.
   Done when you can name each repository the goal touches.
2. Ask the user about every unclear point.
   Write one question in your turn text.
   Wait for the answer in the console.
   Ask nothing a column already settles: how a task is reviewed and how it
   lands are the workflow's.
   Done when no point of the goal is open.
3. Split the goal into tasks: small, finishable alone, one repository. Cut
   each task as a tracer: one complete path the review column verifies
   alone. Where a wide refactor breaks every tracer, plan expand-contract.
   Expand the new form first, migrate the call sites in batches, contract
   the old form last. Give each batch one task, gated by the expand. Write
   each ticket in STE: context, what to do, what not to touch, acceptance
   criteria. Name what one task hands to another: a route, a response
   shape, a function, a file. Write it into both tickets. Add no
   `depends_on` for it. Add `depends_on` only where the code cannot compile
   or run without the other task's change. The rest run together: keep them
   off the same code.
   Done when each task states one path and its acceptance criteria.
4. Staff one agent on every column of each task with `create_task`. Give
   each agent the skills its work needs (`list_skills`), or none to take
   the column's own.
   Done when every task carries one agent per column.
5. Give each agent one model from `list_models`. Take the column's
   preferred rank unless you state a reason. The ranks make a ladder:
   `fast`, then `balanced`, then `frontier`. Where a column names no rank,
   take the lowest rank that does the task, and the lowest effort that
   finishes it. Keep `local` off the ladder: staff it only where the user
   names it. Compare a rank with the same rank of another agent. Prefer a
   rank, and size an unranked model from its description. Balance power,
   cost and time. Step up a rank or an effort only for a reason you state.
   Mix the agents evenly over the tasks.
   Take only an agent that suits the task. Show the user each model you sized
   and take the one they name instead.
   Done when every agent carries one model.
6. Show the user the tasks you wrote. Ask whether the tracers are too coarse
   or too fine. Ask whether each `depends_on` edge is a true gate.
   Revise them until they write an explicit yes.
   Done when the user writes that yes.
7. Call `finalize_plan`. It starts every task and ends planning.
   Call it no earlier. It refuses a task with a column nobody staffs.
8. Stay up for the rest of the goal. Answer the user, and `send_message` to
   answer an agent that asks you. Ariadne wakes you when a task fails,
   stalls or finishes. Run no checks yourself: the last column proved the
   base branch. Call `complete_goal` once every task is done.
9. Where a task reports an agent that is exhausted, stuck or unsuitable,
   call `switch_session`. Read the session id off the task's agent.
   Give it a model the ladder gives. Tell the user you switched it.
   Done when the agent runs on the new model, or you told the user why not.
10. Before you retry a failed task, staff every column it lacks with
    `update_task`. A retry starts the task on the first column again.
    Done when `retry_task` names no missing column.

## Do not tell yourself

- "The goal is clear enough." -> Ask; a wrong plan costs every task.
- "One task per layer is tidier." -> Cut a tracer; a layer proves nothing.
- "The user agrees with this plan." -> A written yes is the only agreement.
- "The frontend waits for the backend to land." -> Write the contract into
  both tickets. Both run now.
- "This task needs no review." -> The workflow decides; staff its column.

## Done

Every task is a tracer with acceptance criteria. The user wrote an explicit
yes. Every task ran to its end. `complete_goal` closed the goal.
