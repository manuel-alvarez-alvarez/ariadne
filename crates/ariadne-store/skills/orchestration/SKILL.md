---
name: orchestration
description: Plan a goal into tracer tasks with the user, then run it to the end. Use when a goal opens, a plan changes, or a task reports back.
---

# Orchestration

Each tracer task is one cohesive outcome and one complete path. Plan with the
user. The workflow columns in your briefing are settled: do not ask for them.

## Steps

1. Read the goal and its columns. Explore each repository it names.
   Done when you can name each repository the goal touches.
2. Ask the user every open point, one question at a time. Ask one question
   in a turn, then stop and wait for the answer. Ask the next question only
   after the answer. Put no second question in the context or in the
   options. Give each question five parts, in this order:
   - A title with its position, for example "Question 2 of 3: <topic>".
   - Context: the facts the user needs to answer. Include files, behavior,
     and constraints. Do not require an earlier turn.
   - Options: two or more. Label each one and state its consequence.
   - A recommendation, with one reason.
   - One direct question, at the end.
   Ask nothing a column settles, such as review or landing.
   Done when no point is open.
3. Split the goal into tracer tasks. Give each one cohesive outcome, one
   repository, and a complete path through every layer it touches. Group
   behaviors that share context and verification. Minimize dependencies.
   For a wide refactor, plan expand-contract: expand, migrate callers in
   batches, then contract. Gate each batch on the expand.
   Done when each task states one outcome.
4. Write each ticket in STE: repository, context, what to do, what not to
   touch, acceptance criteria. Write each criterion as an outcome. Leave
   test cases and verification to the coding agent. Write each contract
   between two tasks into both tickets: a route, a response shape, a
   function, a file. Add `depends_on` only for a genuine compilation,
   execution, or acceptance prerequisite. Do not add one only because two
   tasks touch nearby files.
   Done when each ticket's criteria are outcomes.
5. Staff one agent on every column with `create_task`. Give it the skills
   its work needs from `list_skills`, or none for the column's own.
6. Give each agent one model from `list_models`, at the column's rank.
   Where a column names no rank, take the lowest rank and effort that
   finish the work. Staff `local` only where the user names it. State a
   reason for each step up.
   Done when every agent has one model.
7. Show the user the tasks and the models. Ask about the plan one question at
   a time, with its five parts. Ask about tracer size and each `depends_on`.
   Revise until the user writes an explicit yes.
8. Call `finalize_plan`. Call it no earlier.
9. Stay up until the goal ends. Answer the user, and answer agents with
   `send_message`. Run no checks yourself: the last column proved the work.
10. When a task fails, retry it, rewrite it with `update_task`, or cancel
    it. When an agent is exhausted, stuck or unsuitable, call
    `switch_session` with a model one rank up. Tell the user.
11. Call `complete_goal` when every task is done.

## Do not tell yourself

- "The goal is clear enough." -> Ask; a wrong plan costs every task.
- "Two short questions fit in one turn." -> Ask one; the user answers each
  one with its full context.
- "One task per layer is tidier." -> Cut a tracer; a layer proves nothing.
- "The user agrees with this plan." -> A written yes is the only agreement.
- "The frontend waits for the backend to land." -> Write the contract into
  both tickets. Both run now.
- "This task needs no review." -> The workflow decides; staff its column.

## Done

Every task is a tracer with outcome criteria. The user wrote an explicit yes.
Every task ran to its end. `complete_goal` closed the goal.
