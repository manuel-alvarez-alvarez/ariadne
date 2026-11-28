---
name: orchestration
description: Turns a goal into a plan the user accepted and runs it to its end. Use when goals open, plans change, or tasks report.
---

# Orchestration

Turn a goal into a plan the user accepted, and run it to its end.

The workflow stages in your instructions are settled: do not ask for them.

## When not to use

Use `research` when the user asks a question and wants no plan.

## Steps

1. Read the goal and its stages. Explore each repository it names.

   Done when you can name each repository the goal touches.
2. Ask the user every open point, one question at a time. Ask only where
   the answer changes what gets built. Ask each question with the question
   tool, not in turn text. Ask one question in a turn, then stop
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

   Put the title, the context and the direct question in the question text. Give each option
   its consequence. Put the recommended option first. End its label with
   "(Recommended)", and give the reason in its description.

   Ask nothing a stage settles, such as review or landing.

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
   coding agent. Add a dependency only for a genuine compilation,
   execution, or acceptance prerequisite. Do not add one only because
   two tasks touch nearby files. Keep tasks that run together off the
   same code.

   Done when each ticket has the four sections.
5. Read the skill and model catalogs once for the goal, not once per task.
   Staff one agent on every stage. Give it the skills its work needs, or
   none for the stage's own. Give an agent a brief for what it alone needs
   and the ticket does not say. A brief carries sources, a work order, or
   what to judge. Give none where the ticket says everything.

   Done when each stage has an agent.
6. Give each agent one model and one effort that fit its work.

   Done when every agent has one model.
7. Show the user the tasks and the models. Ask about the plan one question at
   a time, with its five parts. Ask each question with the question tool, not
   in turn text. Ask about tracer size once. Then ask about
   every dependency as one list, each with its reason.
   Revise until the user writes an explicit yes.

   Done when the user has written an explicit yes.
8. Start the plan after the yes, and no earlier.

   Done when the plan has started.
9. Stay up until the goal ends. Answer the user and each agent.

   Done when every task has ended.
10. When a task fails, retry it, rewrite it, or cancel it. When an agent is
    exhausted, stuck or unsuitable, move it to a stronger model. Tell the
    user.

    Done when each failed task has a disposition.
11. Close the goal when every task is done.

    Done when the goal is closed.

## Rules

- A task that is a conversation with the user holds one open question.
  Chain such tasks with dependencies, so the user holds one at a time.

## Never

Never run checks yourself: the last stage proved the work.

## Do not tell yourself

- "The goal is clear enough." -> Ask; a wrong plan costs every task.
- "Two short questions fit in one turn." -> Ask one; the user answers each
  one with its full context.
- "The user agrees with this plan." -> A written yes is the only agreement.
- "The frontend waits for the backend to land." -> Write the contract into
  both tickets. Both run now.
- "This task needs no review." -> The workflow decides; staff its stage.
- "A plan this long is thorough." -> A plan longer than the code it
  describes has written the code.
