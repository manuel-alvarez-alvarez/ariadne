---
id: workflows
status: current
updated: 2026-10-09
areas: [core, store, api, daemon]
commits: []
tests:
  - crates/ariadne-core/src/workflow.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/tests/it/workflows.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
---

# Workflows

A workflow is a linear kanban of columns that replaces the fixed author,
reviewer and landing pipeline. This spec settles the document that defines
one, the catalog Ariadne ships, and the routes that manage it.

## Scope

In: the workflow document's syntax, the catalog rules it follows (017 rules 5
to 9), the `merge` skill the shipped `develop-review-merge` workflow stages,
and the daemon routes over the catalog.

In: goal snapshots, column staffing, step gates, shared worktrees, prompt delivery, and retries.

Out: the desktop, CLI commands, and MCP step tools. Goals without a workflow keep the existing lifecycle.

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

## Behavior

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
   `request-merged`.
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
   base branch's sha as `merge_commit`. A moved base starts again from the
   fetch. It obeys the STE rules and size caps of 006 and the shipped-skill
   tests of `defaults.rs`, the same as every other skill in the catalog.
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
   - `DELETE /v1/workflows/{name}` — 204, or 409 on a built-in.
   - `POST /v1/workflows/parse` — parse a document without saving it
     anywhere; 200 with its name and steps, or 400 `workflow_invalid` with
     `details.line` and the message.

   `WorkflowDto.document` is the effective text: the override, or the
   shipped one. Every write publishes a fat domain event —
   `workflow_created`, `workflow_updated`, `workflow_deleted` — on
   `/v1/events/stream`, carrying the whole `WorkflowDto` the way a skill
   event does.

## Running a goal

1. A goal takes its named workflow, else its first repository's default, else no workflow.
   The first repository follows the existing path and base branch ordering.
   Creation copies each column into `goal_steps` in order.
   Later catalog edits affect later goals only.
   A request cannot set both `workflow` and `landing`.
   A workflow referenced by a goal or repository cannot be deleted: return 409 `workflow_in_use`.
2. A stepped task staffs one `agent` per column, each with its own model, effort, skills, and brief.
   Missing, duplicate, unknown, author, and reviewer assignments are refused.
   Empty skills inherit the column's skills. A step agent can load `pr-babysit`.
   An unstepped task cannot staff an `agent`.
   `update_task.agents` replaces the complete staffing while the task is pending or ready.
3. All columns share the task's branch and worktree.
   The task starts in the first column and stays `in_progress` between columns.
   Only the current column's agent receives work or watchdog attention.
   Other column agents remain idle until their column becomes current.
4. `POST /v1/tasks/{id}/step/complete` takes `reason` and optional `merge_commit`.
   It moves forward one column, or finishes the task from the last column.
   `POST /v1/tasks/{id}/step/fail` takes `reason`.
   It moves back one column, or fails the task from the first column.
   Every call needs a nonempty reason.
   Only the current column's agent can call either route. Other callers receive 403.
   Every move writes its source column, target column, and reason into one transition.
5. Completion checks the column's gate before moving:
   - `committed`: at least one commit past the base, with a clean worktree.
   - `pushed`: the remote holds the branch tip.
   - `merged`: the supplied merge commit and task branch are ancestors of the base branch.
   - `request_merged`: a fresh forge read says the task request merged.
   Failure returns 409 `step_gate_failed` and leaves the task in place.
   The current agent can open the task request before completing its column.
6. First entry uses `StepBriefing`, carrying the previous summary and column instructions.
   Later entry uses `StepReturn`, with `forward`, `back`, or `retry` and the reason.
   `AgentResume` nudges the current column alone.
   Each entry's transition owns its delivery mark.
   The runtime claims that mark before writing the prompt and releases an unwritten claim.
   A queued prompt counts only when written, and repeated scheduler passes queue it once.
   A stale entry is skipped if another column already owns the task.
7. Any step agent can fail the task with `fail_task`.
   A retry starts at the first column and reuses its conversation where available.
   Finished, failed, and cancelled tasks stop every task agent.
   Finished tasks use the configured worktree cleanup. Failed and cancelled tasks retain recoverable work.
8. The orchestrator briefing names the workflow and each column where it previously named the landing.
   Step sessions carry seat `agent`, share the task branch, and load their assigned skill documents.
   Usage entries identify each agent's column. Session facts carry seat `agent` and `data.step`.
9. Migration 0022 expands the schema and rebuilds seat and actor checks without removing old rows.
   It preserves child rows with foreign keys disabled outside the migration transaction.
   Foreign key enforcement and validation resume before the store accepts writes.

## Acceptance criteria

- The parser accepts the two shipped documents and refuses a document with
  no `workflow` line, a duplicate column id, a repeated key, an unknown
  rank, an unknown gate, or no column, each refusal naming its line
  (`workflow.rs`, module tests).
- `request-merged` is spelled with a hyphen in the document and
  `request_merged` on the wire (`workflow.rs::request_merged_is_spelled_with_a_hyphen_in_the_document_and_an_underscore_on_the_wire`).
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
- The `merge` skill passes the shipped-skill tests of `defaults.rs`: named
  once, describes itself, STE, within its cap
  (`defaults.rs::every_shipped_skill_is_named_once_and_describes_itself`,
  `::skill_size_caps_hold`,
  `::every_default_text_is_simplified_technical_english`).

- A goal snapshots its columns, and an unstepped goal exposes none
  (`workflow_steps.rs::a_goal_snapshots_its_workflow_and_an_unstepped_goal_has_no_columns`).
- Staffing validates all columns, inherits skills, and permits complete replacement before work starts
  (`store.rs::stepped_staffing_requires_one_agent_for_each_column`).
- Repository defaults, immutable snapshots, deletion protection, and `pr-babysit` work together
  (`store.rs::workflow_snapshots_keep_their_columns_and_references_prevent_deletion`).
- A task walks forward, returns for changes, and finishes through its gates on one worktree.
  Each move records both columns and its reason, and finishing stops every agent
  (`workflow_steps.rs::a_task_walks_develop_review_merge_with_one_agent_per_column`).
- First-column failure records the reason and retry reuses the first agent with a retry briefing
  (`workflow_steps.rs::a_first_column_failure_retries_on_develop_with_the_same_session`).
- Other columns and the orchestrator receive 403
  (`workflow_steps.rs::another_column_and_the_orchestrator_cannot_move_a_step`).
- Only the current column is nudged. Cancellation stops all task agents
  (`workflow_steps.rs::only_the_current_column_is_nudged_and_idle_columns_raise_no_attention`).
- A failed prompt handoff retains its delivery and retries once
  (`workflow_steps.rs::a_step_briefing_survives_a_closed_prompt_channel`).
- A push gate reads the remote tip, and a request gate reads the forge at each call
  (`workflow_steps.rs::the_push_gate_requires_the_current_tip_on_the_remote`,
  `::a_step_agent_opens_its_request_and_completion_reads_the_forge_now`).
- Any column can fail the task. Usage and facts name its column
  (`workflow_steps.rs::any_column_can_fail_the_task_and_usage_and_facts_name_the_column`).
- The orchestrator sees each column. Agents load their assigned models and skill documents
  (`workflow_steps.rs::the_orchestrator_reads_the_workflow_and_agents_load_their_own_skills`,
  `::a_task_walks_develop_review_merge_with_one_agent_per_column`).
- OpenAPI exposes both step routes, their responses, and every new DTO field
  (`workflow_steps.rs::openapi_describes_the_step_routes_and_workflow_fields`).
- Invalid stored skills, ranks, and gates return errors without panics or step changes
  (`store.rs::invalid_stored_column_skills_refuse_staffing_without_writing_a_task`,
  `workflow_steps.rs::an_invalid_stored_gate_refuses_completion_without_moving_the_task`,
  `::invalid_stored_columns_return_goal_errors`).
- The orchestrator briefing names the workflow even when its template omits the landing line
  (`prompts.rs::workflow_columns_are_named_with_or_without_a_landing_line`).
- Old landing behavior and assignment bodies retain their contracts
  (`landing_lifecycle.rs::feature_tasks_land_on_the_goal_branch_and_keep_the_base_unchanged`,
  `final_tasks.rs::a_final_task_sent_back_to_wait_again_and_again_still_starts`,
  `mcp/tools.rs::a_created_task_is_pinned_to_what_the_orchestrator_named`,
  `::an_edit_takes_default_for_the_effort_and_refuses_it_as_a_model`).
- Migration keeps old rows, references, and a usable backup
  (`store.rs::stepped_migration_preserves_old_rows_and_the_backup_can_be_opened`).
- Step moves accept adjacent columns only, and the new actor can finish or fail its task
  (`state_machine.rs::step_tests`).
- Default texts meet their caps and STE, and builders fill every declared placeholder
  (`defaults.rs::size_caps_hold`, `::every_default_text_is_simplified_technical_english`,
  `prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).

## Sources

`crates/ariadne-core/src/workflow.rs` (`Workflow`, `WorkflowStep`, `StepGate`,
`WorkflowParseError`, `parse`), `crates/ariadne-store/workflows/` (the shipped
documents), `crates/ariadne-store/skills/merge/SKILL.md`,
`crates/ariadne-store/src/workflows.rs`, `crates/ariadne-store/src/defaults.rs`
(`BUILTIN_WORKFLOWS`, `default_workflow_document`),
`crates/ariadne-store/src/entities.rs` (`Workflow`),
`crates/ariadne-api/src/workflows.rs`, `crates/ariadne-daemon/src/http/workflows.rs`,
`crates/ariadne-daemon/src/http/steps.rs`, `crates/ariadne-daemon/src/scheduler/tasks.rs`,
`crates/ariadne-daemon/src/launcher.rs`, `crates/ariadne-store/migrations/0022_stepped_goals.sql`.
