---
name: documentation
description: Writes or repairs a prose page for the reader who has the problem. Use when a task changes a page and no code.
---

# Documentation

Write for the reader who has the problem, not the author who has the code.

## When not to use

Use `coding` when the task changes code: it changes the pages that code
binds in its own commit. Use `spec-writing` when the page is a spec of what
the system will do. Use `research` when the task asks a question.

## Steps

1. Read the task. Name the reader and what they came to do.

   Done when you can state both in one sentence.
2. Read the rules of the page's area: the nearest `AGENTS.md`, `CLAUDE.md`
   or `CONTRIBUTING.md`, and the pages beside it.

   Done when you can name the page's index, its commit type and its checks.
3. Read what the software does. Run the command, read the code and the spec
   that the page describes.

   Done when each claim on the page has a source you read or ran.
4. Write the page. Answer the reader first and put the common case at the
   top. Show a working example before the options. Say what a thing is for
   before what it takes. Match the format and tone of the pages beside it.
   Run each example yourself.

   Done when each example runs and the page reads front to back.
5. Link the page where the reader looks for it: the index, the README, or
   the page above it.

   Done when a reader reaches the page without a search.
6. Run the checks the repository names for the page, such as its link check.
   Commit once, under the repository's commit rules.

   Done when the checks pass and the branch holds the page in one commit.
7. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

- Describe what the software does now. Delete what is no longer true.
  A promised feature belongs in a plan.
- Give every option a default and a reason to change it.
- Name the errors the reader will see, and what each one means.
- Cut every sentence that carries no information for the reader.

## Never

Never change code. Never write a spec in a page.

## Do not tell yourself

- "The reader can work this out." -> The reader came because working it out
  failed. Spell it out.
- "The example is close enough." -> An example that fails when pasted
  poisons the whole page. Run it.
- "I will update the old section later." -> A stale page misleads every
  reader until then. Fix it now.
- "I know what the software does." -> The page you remember is the one
  that drifted. Run it and read it.
