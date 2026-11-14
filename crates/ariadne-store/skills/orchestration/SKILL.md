---
name: orchestration
description: Use when goals open, plans change, or tasks report.
---

# Orchestration

Turn a goal into a plan the user accepted, and run it to its end. Done when the conditions in `## Done` hold.

The workflow columns in your briefing are settled: do not ask for them.

## When not to use

Use `research` when the user asks a question and wants no plan.

## Steps

1. Read the goal and its columns. Explore each repository it names.

   Done when you can name each repository the goal touches.
2. Ask the user every open point, one question at a time. Ask only where
   the answer changes what gets built. Ask one question in a turn, then stop
   and wait for the answer. Ask the next question only after the answer. Put
   no second question in the context or in the options. Give each question
   five parts, in this order:
   - A title with its position, for example "Question 2 of 3: <topic>".
   - Context: the facts the user needs to answer, in the user's terms.
     Include files, behavior, and constraints.
   - Options: two or more. Label each one and state its consequence.
   - A recommendation, with one reason.
   - One direct question, at the end. Word it so a "yes" accepts the
     recommendation.

   Put the title, context and question in the text. Put each option, with
   its consequence, in `options`. Put the pick in `recommended_option_id`,
   and the reason in `recommendation_reason`.

   Ask nothing a column settles, such as review or landing.

   Done when no point is open.
3. Split the goal into tracer tasks. A task is the smallest unit that one
   review accepts or rejects alone. Give each one cohesive outcome, one
   repository, and a complete path through every layer it touches.
   For a wide refactor, plan expand-contract: expand, migrate callers in
   batches, then contract.

   Done when each task has one outcome.
4. Write each ticket in STE, with four sections in order.
   `## Objective` states one outcome.
   `## Criteria` is numbered. Each item is one outcome.
   `## Interfaces` names each contract with another task, as a route, a
   response shape, a function or a file. Write it in both tickets. Write
   "None" where there is none.
   `## Out of scope` states the task's boundaries.
   Carry no pasted history. Leave test cases and verification to the
   coding agent. Add `depends_on` only for a genuine compilation,
   execution, or acceptance prerequisite. Do not add one only because
   two tasks touch nearby files. Keep tasks that run together off the
   same code.

   Done when each ticket has the four sections.
5. List skills and models once for the goal, not once per task.
   Staff one agent on every column with `create_task`. Give it the
   skills its work needs from `list_skills`, or none for the column's
   own.

   Done when each column has an agent.
6. Give each agent one model from `list_models`, at one of the column's ranks.
   Assess each task's scope and complexity case by case. Keep every choice
   within the column's permitted ranks. For routine or uncertain work, choose
   the smallest capable model and effort. Choose a more powerful eligible
   model only where the task demands justify it, and state why. Where a
   column names no ranks, take the lowest rank and effort that finish the
   work. Staff `local` only where the user names it. Give a task's coding and
   review columns models of different ACP agents, where one is eligible. With
   none, take the review column's normal rank-based choice, even from the
   coding column's agent. Tell the user independent review was unavailable.

   Done when every agent has one model.
7. Show the user the tasks and the models. Ask about the plan one question at
   a time, with its five parts. Ask about tracer size and each `depends_on`.
   Revise until the user writes an explicit yes.

   Done when the user has written an explicit yes.
8. Call `finalize_plan`. Call it no earlier.

   Done when the plan is finalized.
9. Stay up until the goal ends. Answer the user, and answer agents with
   `send_message`.

   Done when every task has ended.
10. When a task fails, retry it, rewrite it with `update_task`, or cancel
    it. When an agent is exhausted, stuck or unsuitable, call
    `switch_session` with a model one rank up. Tell the user.

    Done when each failed task has a disposition.
11. Call `complete_goal` when every task is done.

    Done when the goal is closed.

## Never

Never run checks yourself: the last column proved the work.

## Do not tell yourself

- "The goal is clear enough." -> Ask; a wrong plan costs every task.
- "Two short questions fit in one turn." -> Ask one; the user answers each
  one with its full context.
- "The user agrees with this plan." -> A written yes is the only agreement.
- "The frontend waits for the backend to land." -> Write the contract into
  both tickets. Both run now.
- "This task needs no review." -> The workflow decides; staff its column.
- "A plan this long is thorough." -> A plan longer than the code it
  describes has written the code.

## Done

Every task is a tracer with outcome criteria. The user wrote an explicit yes.
Every task ran to its end. `complete_goal` closed the goal.
