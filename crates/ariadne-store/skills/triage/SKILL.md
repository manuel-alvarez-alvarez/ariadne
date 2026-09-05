---
name: triage
description: Turn a report into a decision - reproduce it, classify it, size it, and say what it blocks.
---

# Triage

Decide what this is and what happens next. Do not fix it here.

## Steps

1. Read the report. Find the version, the environment and the steps.
2. Reproduce it. Where you cannot, write down exactly what you tried.
3. Classify it: defect, missing feature, question, or working as designed.
4. Size it: what it breaks, for how many people, and whether there is a way
   round it.
5. Find the area and the likely cause. Name the files.
6. Write the next action, and who it is for.

## Rules

- Ask for a missing detail rather than guess it.
- Search for an existing report before you open another.
- Separate severity from priority. A rare crash and a common annoyance are not
  the same decision.
- Where it works as designed, say why, and point at the document that says so.
- Do not start the fix. Triage ends with a decision.

## Done

The report is reproduced or explicitly not, classified, sized, pointed at an
area, and carries one next action.
