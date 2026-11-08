---
name: spec-writing
description: Use when a goal has no specification.
---

# Spec writing

Complete the spec-writing work. Done when the conditions in `## Done` hold.

Write what the system does, not how the code does it. Every claim you make
must be one a reader can test.

## When not to use

Use `documentation` when its triggers apply.

## Steps

1. Read the goal and the code it touches. List what you do not know.

   Done when each unknown is written as a question.
2. Ask the orchestrator about each unknown with `send_message`. Ask one
   question at a time.

   Done when no question is open.
3. Write the scope. Name what is in it. Name what is out of it, and say
   where that belongs instead.

   Done when the scope names its boundaries.
4. Write the behavior as numbered claims. State one claim per sentence.

   Done when the behavior is numbered.
5. Write one acceptance criterion per claim. Name the test that proves it.

   Done when every claim has a criterion and every criterion names a test.
6. Follow the format of the specs already in the repository. Where there
   are none, agree a path and a format with the orchestrator first.

   Done when the format matches the repository.
7. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

### Rules

- State each claim once. A claim in two places goes stale in one of them.
- Write what holds now, not what is planned. A spec describes; it does not
  promise.
- Give every number a reason. An unexplained limit is one nobody can change
  safely.
- Keep every claim testable. A claim no test can check is an opinion; cut
  it.

## Never

Never change work outside the task.

## Do not tell yourself

- "The code explains this part." -> The spec is the claim; the code is one
  answer to it. Write the claim.
- "Everyone knows what this means." -> A reader who has to guess will guess
  differently. Say it.
- "I will pin the criteria later." -> A claim without its criterion is the
  one that ships untested.

## Done

The spec names its scope, its claims and its criteria. Every criterion
names a test. The orchestrator agreed to it through `send_message`.
