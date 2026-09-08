---
name: triage
description: Turn a report into a clear decision. Use when report facts arrive, defect scope changes, or priority needs evidence.
---

# Triage

Make one decision from the report evidence. End triage with a decision.

## Steps

1. Record the report version, environment and steps.
   Done when the decision record holds each reported fact.
2. Reproduce the report behavior.
   Done when reproduction succeeds or records every attempt and result.
3. Classify the report as a defect, feature, question or designed behavior.
   Done when the decision record names one classification.
4. Size the report impact, scope and workaround.
   Done when the decision record names each affected user group.
5. Find the area and likely cause. Name the relevant files.
   Done when the decision record points to the affected area.
6. Write the next action and its owner.
   Done when the decision record assigns one action to one owner.

## Rules

- Ask for every missing report fact.
- Search for an existing report before you create another.
- Assess severity separately from priority.
- Cite the governing document for designed behavior.
- Hand fix work to a follow-up task after the decision.

## Do not tell yourself

- "Missing logs will not matter." -> Ask for the missing report facts.
- "We can fix it now." -> Record the decision, then hand off fix work.
- "One report means one defect." -> Search existing reports first.

## Done

The decision record states the reproduction result, classification, impact,
area, action and owner.
