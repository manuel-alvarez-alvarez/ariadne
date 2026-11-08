---
name: documentation
description: Use when a feature lands or a page drifts.
---

# Documentation

Write for the reader who has the problem, not the author who has the code. Done when `## Done` holds.

## When not to use

Use `spec-writing` when the goal needs a spec of what the system must do.
Use `coding` when a repository rule binds the page to a code change.

## Steps

1. Name the reader and what they came to do.

   Done when you can state both in one sentence.
2. Answer that first. Put the common case at the top.

   Done when the common case comes first.
3. Show a working example before the options.

   Done when you have run the example yourself.
4. Say what a thing is for before you say what it takes.

   Done when each item states its purpose.
5. Match the format and the tone of the documents already there, and the
   repository rules for that area.

   Done when the document matches its neighbors.
6. Link the page where the reader looks for it: the index, the README, or
   the page above it.

   Done when a reader reaches the page without a search.
7. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

- Describe what the software does now. Delete what is no longer true.
- Describe only features that exist. A promised feature belongs in a plan.
- Give every option a default and a reason to change it.
- Name the errors the reader will see, and what each one means.
- Cut every sentence that carries no information for the reader.

## Never

Never change code. Never write a spec in a manual.

## Do not tell yourself

- "The reader can work this out." -> The reader came because working it out
  failed. Spell it out.
- "The example is close enough." -> An example that fails when pasted
  poisons the whole page. Run it.
- "I will update the old section later." -> A stale page misleads every
  reader until then. Fix it now.

## Done

A reader can do the task from the page alone, and reaches it from the
index. Every example runs. Nothing on the page is out of date.
