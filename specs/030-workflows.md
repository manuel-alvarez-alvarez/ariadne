---
id: workflows
status: current
updated: 2026-10-10
areas: [core, store, api, daemon, prompts]
commits: []
tests:
  - crates/ariadne-core/src/workflow.rs
  - crates/ariadne-core/src/state_machine.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/it/workflows.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/plan_finalize.rs
  - crates/ariadne-daemon/tests/it/repositories.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
---

# Workflows

A workflow is a linear kanban of columns. Every goal runs on one: the goal
snapshots the columns, every task is staffed one agent per column, and a task
walks the columns on one branch in one worktree until the last column's gate
lets it finish. This spec is the whole account of that engine: the document
that defines a workflow, the catalog Ariadne ships, the routes over it, the
staffing, the step moves, the gates, the prompts, the pull request column, and
the schema that replaced the fixed author/reviewer/landing pipeline.

## Scope

In: the workflow document's syntax, the catalog rules it follows (017 rules 5
to 9), the `merge` skill, the daemon routes over the catalog, goal snapshots,
column staffing, step moves and their gates, the shared worktree, prompt
delivery, retries, the `pr` column, and the schema that replaced the fixed
pipeline.

Out: the transition table the step moves sit inside (001), the orchestrator
that writes and staffs the tasks (003), the channel the agents talk on
(018), what the forge holds of a request (026), the CLI (014) and the
desktop (015).

## The document syntax

```
workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, run the whole suite, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged
```

1. The first non-blank line is `workflow <name>`. The name is kebab-case.
2. A column line is `<id>[<Title>]`. The id is kebab-case. Ids are unique in
   one document.
3. Every non-blank line after a column line and before the next column line
   belongs to that column.
4. A body line `skills: a, b` names the skills, comma-separated. `rank:
   <word>` names one of `fast`, `balanced`, `frontier`, `local`. `gate:
   <word>` names one of `committed`, `pushed`, `merged`, `request-merged`.
   Each key appears at most once per column. Every other body line is a
   description line. The description joins its lines with one space.
   Skills, rank, gate and description are all optional.
5. Blank lines are ignored. Indentation is ignored. At least one column is
   required.
6. A document that breaks a rule is refused with the line number and one
   sentence.

`ariadne_core::workflow::parse` reads this syntax into a `Workflow { name,
steps: Vec<WorkflowStep> }`, each step carrying its `id`, `title`,
`description`, `skills`, an optional `rank` (`ariadne_core::models::ModelRank`)
and an optional `gate` (`ariadne_core::workflow::StepGate`). The document
spells a gate's fourth word with a hyphen, `request-merged`, like every other
kebab-case word in it; the wire spells the same gate `request_merged`, like
every other wire enum (011).

## The catalog

1. A workflow is stored the way a skill is (017 rules 5 to 8): a `NULL`
   document while it runs on the text Ariadne ships, seeded by name on every
   open, a built-in reset rather than deleted, and a workflow of the user's
   own deleted rather than reset. The same open adopts a user workflow under
   a name the catalog gains, and prunes a built-in the catalog drops that
   holds nothing of anybody's.
2. Ariadne ships two: `develop-review-merge`, which lands a task on the base
   branch itself (its `merge` column stages the `merge` skill), and
   `develop-review-pr`, whose `develop` and `review` columns are the same and
   whose third column, `pr[Pull request]`, stages `pr-babysit` and gates on
   `request-merged`. `develop-review-merge` is the default of every
   repository that names none (`DEFAULT_WORKFLOW`).
3. A save — a create or a document write — parses the document and refuses a
   document that breaks a syntax rule, naming the line. It also refuses a
   document whose `workflow <name>` line does not equal the row's name, and
   a document whose `skills` name a skill no row answers to, or the
   orchestrator's skill (`orchestration`) or the one a reviewer pull request
   session loads (`pr-reviewer`) — neither staffs a task agent, which is
   what a workflow column does. `pr-babysit` is allowed: the `pr` column of
   `develop-review-pr` stages it.
4. The `merge` skill is shipped alongside the two workflows
   (`crates/ariadne-store/skills/merge/SKILL.md`): rebase the task branch
   onto the base, run the whole suite once, squash onto the merge base,
   check `git diff --stat`, fast-forward the base branch in the primary
   checkout, push where a remote exists, then call `complete_step` with the
   base branch's sha as `merge_commit`. The squash goes onto the merge base
   rather than the base branch's name, since the base can move while the
   suite runs; a `git diff --stat` that lists a file the task did not touch,
   or a fast-forward that fails, says the base moved, and the agent starts
   again from the fetch. No skill merges a request: a human does. It obeys
   the STE rules and size caps of 006 and the shipped-skill tests of
   `defaults.rs`, the same as every other skill in the catalog.
5. The daemon exposes the catalog at `/v1/workflows`, mirroring the skill
   routes (017) and their error codes:
   - `GET /v1/workflows` — list every workflow, shipped and written.
   - `POST /v1/workflows` — create one of the user's own; 201, or 409 on a
     taken name or an invalid document.
   - `GET /v1/workflows/{name}` — 404 `workflow_not_found`.
   - `PUT /v1/workflows/{name}` — write a new document over one's; 404 or
     409.
   - `POST /v1/workflows/{name}/reset` — put a built-in back on its shipped
     text; 409 on a workflow of the user's own.
   - `DELETE /v1/workflows/{name}` — 204, or 409 on a built-in, or 409
     `workflow_in_use` on a workflow a goal or a repository names.
   - `POST /v1/workflows/parse` — parse a document without saving it
     anywhere; 200 with its name and steps, or 400 `workflow_invalid` with
     `details.line` and the message.

   `WorkflowDto.document` is the effective text: the override, or the
   shipped one. Every write publishes a fat domain event —
   `workflow_created`, `workflow_updated`, `workflow_deleted` — on
   `/v1/events/stream`, carrying the whole `WorkflowDto` the way a skill
   event does. The desktop's workflow editor and its New workflow dialog
   alike write a draft against this same `parse` route for their live
   preview and their editor's error marks; the UI rules for both are in
   015, not here.

## Running a goal

1. A goal takes its named workflow, else its first repository's default
   (`repositories.default_workflow`, `develop-review-merge` where none was
   set). `goals.workflow` is never null. The first repository follows the
   existing path and base branch ordering. Creation copies each column into
   `goal_steps` in order — id, title, description, skills, rank, gate — so
   later catalog edits affect later goals only. `GoalDto.workflow` and
   `GoalDto.steps` carry the snapshot; `RepositoryDto.default_workflow` the
   default. A repository's default changes no existing goal.
2. A task is staffed one `agent` per column, each with its own model, effort,
   skills and brief (`AgentAssignment { step, skills, model, effort, brief
   }`; `CreateTaskRequest.agents`, `UpdateTaskRequest.agents`). An agent
   naming a column the goal's workflow has none of, or a column staffed
   twice, is refused by name. Empty skills inherit the column's. A step agent
   can load `pr-babysit`; `orchestration` and `pr-reviewer` are refused
   (017 rule 10). While the goal is in `planning` a task may leave columns
   unstaffed, so the orchestrator can staff in several calls; `finalize_plan`
   refuses a plan with a task that has a column nobody staffs, naming the
   task and the column (003 rule 10). Once the goal is active a task is
   runnable the moment it is written, so a create or an edit that leaves a
   column unstaffed is refused by name, and a retry of such a task is
   refused the same way (rule 7). A task the scheduler finds runnable with a
   column unstaffed all the same — one written while the goal was still
   planned, left that way when the goal went active — fails before it
   starts, naming the column, and spends no launch. `update_task.agents` replaces the complete staffing while the
   task is `pending`, `ready` or `failed`: an agent already on its column
   keeps its row, and with it the sessions, messages and usage that name it;
   a column staffed for the first time gets a new row; two rows on one
   column fold into the first; a column the edit leaves out loses its agent
   only while nothing names it, and is refused by name otherwise.
3. All columns share the task's branch and worktree (002). The task starts
   in the first column and stays `in_progress` between columns: the column
   it is on is `TaskDto.step`. Only the current column's agent receives work
   or watchdog attention (009 rule 5). Other column agents remain idle until
   their column becomes current, and a message to one waits for its column
   (018 rule 9).
4. `POST /v1/tasks/{id}/step/complete` takes `reason` and optional
   `merge_commit`. It moves forward one column, or finishes the task from
   the last column. `POST /v1/tasks/{id}/step/fail` takes `reason`. It moves
   back one column, or fails the task from the first column. Every call
   needs a nonempty reason. Only the current column's agent can call either
   route; another column's agent, the orchestrator and the user receive 403.
   Every move writes its source column, target column and reason into one
   transition (`TransitionDto.from_step`, `to_step`). A move that reaches a
   status — `finished`, `failed` — is a transition of the table (001 rule
   5), by actor `agent`.
5. Completion checks the column's gate before moving:
   - `committed`: at least one commit past the base, with a clean worktree;
   - `pushed`: the remote holds the branch tip;
   - `merged`: the supplied merge commit and the task branch are ancestors
     of the base branch;
   - `request_merged`: a fresh forge read says the task's request merged.
   A column with no gate moves on the reason alone. A gate that fails
   returns 409 `step_gate_failed` and leaves the task in place. A stored
   gate word no `StepGate` answers to refuses the completion without moving
   the task. The current agent can open the task's request before completing
   its column.
6. First entry to a column uses `StepBriefing`, carrying the task, the
   column's title, description and skills, the base branch and the summary
   the previous column completed with. A later entry uses `StepReturn`, with
   `forward`, `back` or `retry` and the reason of the move. `AgentResume`
   nudges the current column alone. Each entry's transition owns its
   delivery mark (`step_briefed_at`): the runtime claims that mark right
   before it writes the prompt and releases an unwritten claim, so a queued
   prompt counts only once written and repeated scheduler passes queue it
   once (009 rule 6, 018 rule 8). A stale entry is skipped if another column
   already owns the task.
7. Any step agent can fail the task with `fail_task` (001 rule 6). A retry
   puts the task back on the first column and reuses its agent's
   conversation where one exists on the pin the agent still has; an agent
   whose model or effort an edit moved gets a fresh session on the new pin,
   and the old session stays as history (011 rule 11). The daemon refuses a
   retry of a task with an unstaffed column, naming the column, until
   `update_task` staffs it.
   Finished, failed and cancelled tasks stop every task agent. A finished
   task uses the configured worktree cleanup; a failed or cancelled task
   keeps its worktree, so the work is recoverable.
8. The orchestrator briefing names the workflow and one line per column — its
   id, title, skills, rank and gate — where it named the landing before
   (003 rule 1, `ORCHESTRATOR_BRIEFING`'s `{workflow}` and `{columns}`).
   Step sessions carry seat `agent`, share the task branch, and load their
   assigned skill documents. `TaskUsageDto { total, agents }` names each
   agent's column; `GoalUsageDto { total, orchestrator, agents }` splits the
   goal's. Session facts carry seat `agent` and `data.step`, and the
   `task_ended` fact carries `step` (023).
9. The schema holds no pipeline for this spec to replace: `goals.workflow`
   is `NOT NULL REFERENCES workflows (name)`, and
   `repositories.default_workflow` is `NOT NULL DEFAULT
   'develop-review-merge'`. `task_agents` keys an agent by `step`, not by a
   seat; `agent_sessions.seat` is `orchestrator`, `agent` or `reviewer`;
   `messages.kind` is `message` alone; `task_transitions.actor` is
   `orchestrator`, `agent`, `daemon` or `user`. There is no `task_picks`
   table, no `tasks.picked_agent_id` and no `goal_repositories.goal_branch`.
   A database on a schema this release does not ship — the fixed
   author/reviewer/landing pipeline this spec replaced included — is
   refused at open with the pre-squash message, naming the file to delete:
   Ariadne is pre-1.0, so such a database is recreated rather than migrated
   (016 rule 9).
10. The `pr` column of `develop-review-pr` stages `pr-babysit`. Its agent
    pushes the branch and opens the request through `open_pull_request`
    once; a repeat returns the same URL. The request's news goes to that
    agent (026): its session is the request DTO's `session_id`, its tools
    accept that session, and an idle agent waits on the forge without a
    quiet nudge (009 rule 41). A ready report raises `waiting_user` on that
    session; a later false report clears it. The agent makes requested
    fixes as new commits, runs the changed tests and lint, and pushes. On
    merge, it brings the remote base into the checkout before completing
    the step. An open request fails the `request_merged` gate with 409 at
    `complete_step`; a fresh forge read that says merged lets the step
    finish. A close is news and finishes nothing: the agent fails the task.
    The daemon advances a merged request column itself if its agent falls
    quiet for the quiet nudge, and finishes the task when that column is
    last; a later column runs when one follows. Cleanup stops every agent,
    removes the worktree and branch, and removes the request row and its
    marks.

## Acceptance criteria

- The parser accepts the two shipped documents and refuses a document with
  no `workflow` line, a duplicate column id, a repeated key, an unknown
  rank, an unknown gate, or no column, each refusal naming its line
  (`workflow.rs::the_shipped_develop_review_merge_document_parses`,
  `::the_shipped_develop_review_pr_document_parses`,
  `::a_document_with_no_workflow_line_is_refused_naming_its_line`,
  `::a_duplicate_column_id_is_refused_naming_its_line`,
  `::a_repeated_key_is_refused_naming_its_line`,
  `::an_unknown_rank_is_refused_naming_its_line`,
  `::an_unknown_gate_is_refused_naming_its_line`,
  `::a_document_with_no_column_is_refused`,
  `::blank_lines_and_indentation_are_ignored`).
- `request-merged` is spelled with a hyphen in the document and
  `request_merged` on the wire
  (`workflow.rs::request_merged_is_spelled_with_a_hyphen_in_the_document_and_an_underscore_on_the_wire`).
- A fresh database holds both shipped workflows on their shipped text, and a
  reopen reseeds no row the database holds
  (`store.rs::a_fresh_database_is_seeded_with_every_shipped_workflow_on_its_own_text`,
  `::a_reopen_reseeds_no_workflow_row_the_database_already_holds`).
- A built-in is reset and never deleted; a workflow of the user's own is
  deleted and never reset
  (`store.rs::workflow_crud_and_the_two_refusals_that_tell_them_apart`).
- A save that names an unknown skill, or the orchestrator's skill, or the
  reviewer pull request session's skill, is refused and names the skill;
  `pr-babysit` is allowed
  (`store.rs::a_workflow_save_refuses_an_unknown_skill_naming_it`,
  `::a_workflow_save_refuses_the_orchestrators_skill_and_the_reviewer_sessions_skill`).
- Every route round-trips through the daemon and emits one fat event per
  write; the parse route answers the steps of a good document and the line
  of a bad one
  (`tests/it/workflows.rs::every_route_round_trips_and_emits_one_fat_event_per_write`,
  `::the_parse_route_answers_the_steps_of_a_good_document_and_the_line_of_a_bad_one`,
  `::a_missing_workflow_is_a_404_naming_workflow_not_found`,
  `::a_taken_name_is_a_409_and_a_built_in_refuses_delete_and_a_user_one_refuses_reset`,
  `::resetting_a_built_in_drops_the_override`).
- Every shipped workflow parses and stages shipped skills, and the `merge`
  skill passes the shipped-skill tests: named once, describes itself, STE,
  within its cap, lands in order and merges no request itself
  (`defaults.rs::every_shipped_workflow_parses_and_stages_shipped_skills`,
  `::every_shipped_skill_is_named_once_and_describes_itself`,
  `::skill_size_caps_hold`,
  `::every_default_text_is_simplified_technical_english`,
  `::the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself`,
  `::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`).
- A goal created with no workflow takes its first repository's default and
  snapshots its columns; a repository's default changes no existing goal
  (`store.rs::a_goal_created_with_no_workflow_takes_its_first_repositorys_default`,
  `workflow_steps.rs::a_goal_takes_the_repository_default_and_snapshots_its_workflow`,
  `repositories.rs::a_repository_defaults_new_goals_without_changing_existing_ones`,
  `plan_finalize.rs::a_goal_created_without_a_workflow_runs_its_first_repositorys_default`).
- A staffing names the goal's columns once each, inherits skills, refuses an
  unknown or doubled column by name, may leave columns for later while the
  goal is planned, and is refused by name once the goal is active; an edit
  replaces the whole staffing and keeps the row, the sessions and the
  messages of an agent already on its column
  (`store.rs::a_staffing_names_the_goals_columns_once_each_and_may_leave_some_for_later`,
  `::an_edit_replaces_the_whole_column_staffing`,
  `tools.rs::a_workflow_task_staffs_one_agent_for_each_column`).
- A plan with a task that has an unstaffed column is refused by the column's
  name, and so is the retry of such a task; a runnable task the scheduler
  finds unstaffed fails before it starts, naming the column, and an active
  goal refuses the create or the edit that would leave one
  (`plan_finalize.rs::finalize_refuses_a_task_with_an_unstaffed_column_by_name_until_it_is_staffed`,
  `workflow_steps.rs::a_retry_refuses_a_task_with_an_unstaffed_column_by_name`,
  `::a_runnable_task_with_an_unstaffed_column_fails_before_it_starts`).
- Repository defaults, immutable snapshots, deletion protection and
  `pr-babysit` work together
  (`store.rs::workflow_snapshots_keep_their_columns_and_references_prevent_deletion`).
- A task walks forward, returns for changes, and finishes through its gates
  on one worktree. Each move records both columns and its reason, and
  finishing stops every agent
  (`workflow_steps.rs::a_task_walks_develop_review_merge_with_one_agent_per_column`).
- A first-column failure records the reason, and a retry reuses the first
  agent with a retry briefing; a retry after an edit of the agent's pin runs
  a new session on that pin and keeps the old one
  (`workflow_steps.rs::a_first_column_failure_retries_on_develop_with_the_same_session`,
  `resume.rs::a_retry_after_a_staffing_edit_starts_a_new_session_on_the_new_pin`).
- Another column and the orchestrator receive 403
  (`workflow_steps.rs::another_column_and_the_orchestrator_cannot_move_a_step`).
- Only the current column is nudged, and cancellation stops all task agents
  (`workflow_steps.rs::only_the_current_column_is_nudged_and_idle_columns_raise_no_attention`).
- A failed prompt hand-off retains its delivery and retries once
  (`workflow_steps.rs::a_step_briefing_survives_a_closed_prompt_channel`).
- A push gate reads the remote tip, and a request gate reads the forge at
  each call
  (`workflow_steps.rs::the_push_gate_requires_the_current_tip_on_the_remote`,
  `::a_step_agent_opens_its_request_and_completion_reads_the_forge_now`).
- The `pr` column opens and keeps one request. Its agent gets comments and
  failed checks once, reports ready, enforces the merge gate, and cleans up
  after a merge. A close ends nothing. The daemon finishes the task when its
  agent falls quiet after merge news
  (`workflow_pull_request.rs::the_pr_column_opens_the_request_once_and_keeps_it`,
  `::a_close_is_told_to_the_pr_agent_and_finishes_nothing`,
  `::a_quiet_pr_agent_is_finished_after_the_merge`).
- A merged request column advances to a later column after its agent falls
  quiet, and keeps the task, worktree and request until later work finishes
  (`workflow_pull_request.rs::a_quiet_pr_agent_advances_to_the_column_after_the_merge`).
- Any column can fail the task, and usage and facts name its column
  (`workflow_steps.rs::any_column_can_fail_the_task_and_usage_and_facts_name_the_column`,
  `store.rs::a_tasks_usage_groups_every_session_of_an_agent_together`).
- The orchestrator sees each column, and agents load their assigned models
  and skill documents
  (`workflow_steps.rs::the_orchestrator_reads_the_workflow_and_agents_load_their_own_skills`).
- OpenAPI exposes both step routes, their responses, and every workflow
  field, and nothing of a landing, an author, a reviewer or a pick
  (`workflow_steps.rs::openapi_describes_the_step_routes_and_workflow_fields`).
- Invalid stored skills, ranks and gates return errors without panics or
  step changes
  (`store.rs::invalid_stored_column_skills_refuse_staffing_without_writing_a_task`,
  `workflow_steps.rs::an_invalid_stored_gate_refuses_completion_without_moving_the_task`,
  `::invalid_stored_columns_return_goal_errors`).
- The orchestrator briefing names the workflow and one line per column
  (`prompts.rs::the_orchestrator_is_briefed_with_the_workflow_and_its_columns`,
  `defaults.rs::the_orchestrator_briefing_names_the_workflow_and_its_columns`).
- Step moves accept adjacent columns only, and an agent finishes or fails
  its task (`state_machine.rs::step_moves_only_reach_adjacent_columns`,
  `::an_agent_finishes_work_or_fails_an_unfinished_task`).
- Default texts meet their caps and STE, and builders fill every declared
  placeholder
  (`defaults.rs::size_caps_hold`, `::every_default_text_is_simplified_technical_english`,
  `prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).

## Sources

`crates/ariadne-core/src/workflow.rs` (`Workflow`, `WorkflowStep`, `StepGate`,
`WorkflowParseError`, `parse`), `crates/ariadne-core/src/state_machine.rs`,
`crates/ariadne-store/workflows/` (the shipped documents),
`crates/ariadne-store/skills/merge/SKILL.md`,
`crates/ariadne-store/skills/pr-babysit/SKILL.md`,
`crates/ariadne-store/src/workflows.rs`, `crates/ariadne-store/src/defaults.rs`
(`BUILTIN_WORKFLOWS`, `DEFAULT_WORKFLOW`, `PULL_REQUEST_WORKFLOW`,
`default_workflow_document`), `crates/ariadne-store/src/entities.rs`
(`Workflow`, `GoalStep`, `TaskAgent`), `crates/ariadne-store/src/tasks.rs`
(`check_workflow_staffing`, `unstaffed_columns`),
`crates/ariadne-store/src/task_agents.rs`, `crates/ariadne-store/src/goals.rs`
(`goal_workflow`), `crates/ariadne-api/src/workflows.rs`,
`crates/ariadne-api/src/tasks.rs` (`AgentAssignment`),
`crates/ariadne-daemon/src/http/workflows.rs`,
`crates/ariadne-daemon/src/http/steps.rs`,
`crates/ariadne-daemon/src/http/channel.rs` (`open_pull_request`),
`crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-daemon/src/scheduler/pull_requests.rs`,
`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/agents/prompts.rs`,
`crates/ariadne-store/migrations/0001_init.sql`.
