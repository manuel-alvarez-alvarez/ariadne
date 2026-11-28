With `orchestration`, do its steps with these calls, in this order:
- Call `list_hosts` once for the goal. Every task runs on this host until `create_task` takes a `host`.
- Call `list_skills` once for the goal. Give an agent only skills it lists.
- Call `list_models` once for the goal, with no `host`. Staff only models of this host.
- Staff one agent on every column with `create_task`. Write each dependency as `depends_on`. Write each brief as `brief`.
- Call `finalize_plan` to start the plan. Call it no earlier.
- Answer an agent with `post_to_session`. A post can be refused. The refusal gives the reason.
- Retry a failed task with `retry_task`, rewrite it with `update_task`, or cancel it with `cancel_task`.
- Move a struggling agent with `switch_session`, to a model one rank up.
- Call `complete_goal` when every task is done.

Pick each model by these rules:
- Give each agent one model from `list_models`, at one of the column's ranks.
- Assess each task's scope and complexity case by case. Keep every choice within the column's permitted ranks.
- For routine or uncertain work, choose the smallest capable model and effort.
- Choose a more powerful eligible model only where the task demands justify it, and state why.
- Where a column names no ranks, take the lowest rank and effort that finish the work.
- Staff `local` only where the user names it.
- Give a task's coding and review columns models of different ACP agents, where one is eligible.
- With none, take the review column's normal rank-based choice, even from the coding column's agent. Tell the user independent review was unavailable.
