---
name: spec-writing
description: Writes the spec that later tasks build against, as claims with a test each. Use when a goal needs a spec before its code exists.
---

# Spec writing

Write what the system will do, so that a reader can test every claim.

## When not to use

Use `coding` when the code exists or lands in the same task: it changes the
spec in its commit. Use `documentation` when the page teaches a reader
software that exists. Use `research` when the goal asks which option to pick.

## Steps

1. Read the goal and the code it touches. List what you do not know.

   Done when each unknown is written as a question.
2. Read the repository's spec rules and the specs already there. Where
   there are none, add the path and the format to your questions.

   Done when you can name the format, or the question that settles it.
3. Send every open question to the orchestrator in one `send_message`.
   Give each question its options and your pick. The orchestrator asks the
   user.

   Done when every question has an answer.
4. Write the scope in that format. Name what is in it. Name what is out of
   it, and say where that belongs instead.

   Done when the scope names its boundaries.
5. Write the behavior as numbered claims. State one claim per sentence.

   Done when the behavior is numbered.
6. Write one acceptance criterion per claim. Name the test that proves it:
   its file and its name. The coding task writes that test. Mark each test
   that does not exist yet, in the form the repository uses for an unbuilt
   part.

   Done when every criterion names a test and every unbuilt test is marked.
7. Send the finished spec to the orchestrator with `send_message`. Ask
   it to agree.

   Done when the orchestrator has agreed to the spec.
8. Commit the spec once, under the repository's commit rules.

   Done when the branch holds the spec in one commit.
9. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

- State each claim once. A claim in two places goes stale in one of them.
- Give every number a reason. An unexplained limit is one nobody can change
  safely.
- Cut a claim no test can check. It is an opinion.

## Never

Never write the code or the test a criterion names.

## Do not tell yourself

- "The code explains this part." -> The spec is the claim; the code is one
  answer to it. Write the claim.
- "Everyone knows what this means." -> A reader who has to guess will guess
  differently. Say it.
- "I will pin the criteria later." -> A claim without its criterion is the
  one that ships untested.
