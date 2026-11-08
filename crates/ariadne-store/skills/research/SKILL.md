---
name: research
description: Use when a task asks a question or compares options.
---

# Research

Answer the question from evidence, with a recommendation. Done when `## Done` holds.

## When not to use

Use `spec-writing` when the answer becomes a spec. Use `coding` when the
task asks for a change, not an answer.

## Steps

1. Write the question in one sentence, with its scope and the evidence that
   settles it. Confirm it with the orchestrator through `send_message` where
   the task does not state it.

   Done when the task or the orchestrator states the question.
2. List what an answer needs. Name the sources that can give it.

   Done when each need names a source.
3. Gather the evidence. Record the source of each fact as you find it.

   Done when each fact on your list has a source.
4. Test the leading answer against the evidence that contradicts it.

   Done when you have hunted the strongest counter-evidence and written
   what you found.
5. Write the report: question, findings, recommendation, confidence, and
   what you did not check.

   Done when the report has all five parts.
6. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

- Cite the source of every fact: a link, a file and line, or a command and
  its output.
- Separate what you measured from what you inferred.
- Give the strongest case against your own recommendation.
- Where the evidence does not settle the question, say so. Report the open
  question as the finding.

## Never

Never edit the repository. The report is the product.

## Do not tell yourself

- "This is common knowledge." -> Common knowledge is the opinion that
  spreads fastest. Find the source.
- "One good source settles it." -> One source is one perspective. Look for
  the source that disagrees.
- "The report reads thin without more." -> Padding buries the finding. An
  unsettled question is a finding too.

## Done

The report answers the question and cites a source for every fact. It states
a recommendation with its confidence, names what would change it, and lists
what it did not check.
