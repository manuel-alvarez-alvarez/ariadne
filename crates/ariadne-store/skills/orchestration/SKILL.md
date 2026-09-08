---
name: orchestration
description: Turn one goal into a plan of small tasks with the user, staff each one, and start them only on an explicit yes.
---

# Orchestration

The plan is a conversation with the user. Run the steps in order.

## Steps

1. Read the goal. Explore its repositories.
2. Ask the user about every unclear point, until nothing about the goal is open. Write one question in your turn text. Wait for the answer in the terminal.
3. Split the goal into tasks: small, finishable alone, one repository. Write each ticket in STE: context, what to do, what not to touch, acceptance criteria. Add `depends_on` only for a real dependency. The rest run together: keep them off the same code.
4. Staff one author per task with `create_task`. Give each agent the skills its work needs (`list_skills`). It knows only its task and its skills.
5. Ask the user which tasks are worth a review, and what each review is for. Staff those reviewers. Staff none on the rest.
6. Ask the user how each task ends. `merge` puts it on the base branch. `pull_request` opens a request and sees it through. `none` lands nothing.
7. Give each agent one model from `list_models`. Size it: shape from `best_for` and `avoid_for`, risk from `cost`, routine from `speed`, effort from its description. Give a top effort only where the task earns it, `tier: unknown` only on request. Mix the agent CLIs evenly over the tasks. Take only a CLI that suits the task. Show the user what each agent runs on and take the model they name instead.
8. Show the user the tasks you wrote. Revise them until they write an explicit yes.
9. Call `finalize_plan`. It starts every task and ends planning. Call it no earlier.
10. Stay up for the rest of the goal. Answer the user, and `send_message` to answer an agent that asks you. Ariadne wakes you when a task fails, stalls or finishes. Call `complete_goal` once every task is done.

## Done

The user wrote an explicit yes, every task ran to its end, and `complete_goal` closed the goal.
