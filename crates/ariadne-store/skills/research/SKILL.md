---
name: research
description: Use when a task asks a question or compares options.
---

# Research

Complete the research work. Done when the conditions in `## Done` hold.

A finding without a source is an opinion.

## When not to use

Use `spec-writing` when its triggers apply.

## Steps

1. Write the question in one sentence. Confirm it with the orchestrator
   through `send_message` where the task does not state it.

   Done when the task or the orchestrator states the question.
2. List what an answer needs. Name the sources that can give it.

   Done when answers to every listed need would settle the question, and
   each need names a source.
3. Gather the evidence. Record the source of each fact as you find it.

   Done when each fact on your list has a source.
4. Test the leading answer against the evidence that contradicts it.

   Done when you have hunted the strongest counter-evidence and written
   what you found.
5. Write the report: question, findings, recommendation, confidence.

   Done when the report has all four parts.
6. Call `complete_step` with the column report.

   Done when the report has been submitted.

## Rules

### Rules

- Cite the source of every fact: a link, a file and line, or a command and
  its output.
- Separate what you measured from what you inferred.
- Give the strongest case against your own recommendation.
- Say what you did not check.
- Where the evidence does not settle the question, say so. Report the open
  question as the finding.

## Never

Never change work outside the task.

## Do not tell yourself

- "This is common knowledge." -> Common knowledge is the opinion that
  spreads fastest. Find the source.
- "One good source settles it." -> One source is one perspective. Look for
  the source that disagrees.
- "The report reads thin without more." -> Padding buries the finding. An
  unsettled question is a finding too.

## Done

The report answers the question and names the source of every fact. It
states a recommendation with its confidence, and lists what would change it.
