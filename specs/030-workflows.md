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
---

# Workflows

A workflow is a linear kanban of columns that replaces the fixed author,
reviewer and landing pipeline. This spec settles the document that defines
one, the catalog Ariadne ships, and the routes that manage it.

## Scope

In: the workflow document's syntax, the catalog rules it follows (017 rules 5
to 9), the `merge` skill the shipped `develop-review-merge` workflow stages,
and the daemon routes over the catalog.

Out: how a goal or a task reads a workflow and steps through its columns —
that is the step engine, a later task. Nothing here changes what a goal or a
task does today.

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

## Sources

`crates/ariadne-core/src/workflow.rs` (`Workflow`, `WorkflowStep`, `StepGate`,
`WorkflowParseError`, `parse`), `crates/ariadne-store/workflows/` (the shipped
documents), `crates/ariadne-store/skills/merge/SKILL.md`,
`crates/ariadne-store/src/workflows.rs`, `crates/ariadne-store/src/defaults.rs`
(`BUILTIN_WORKFLOWS`, `default_workflow_document`),
`crates/ariadne-store/src/entities.rs` (`Workflow`),
`crates/ariadne-api/src/workflows.rs`, `crates/ariadne-daemon/src/http/workflows.rs`.
