---
id: skills-and-staffed-agents
status: current
updated: 2026-09-08
areas: [store, api, cli, ui, daemon, prompts]
commits: [03f9c8b7, 29e6d84e]
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/tests/skill_documents.rs
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-daemon/tests/prompts.rs
---

# Skills and staffed agents

Ariadne defines one agent type, the orchestrator — and even its playbook is a
skill, fixed by name rather than staffed. Every other agent is generic, and
becomes what its task needs by loading skills.

## Scope

In: what a skill is, the catalog Ariadne ships, who may edit one, how a task
staffs an agent on skills, and what a seat means once identity is gone.

Out: how a skill document reaches the agent CLI (007), how the index is
written into the system prompt (006), and the lifecycle the seats sit in
(001, 004).

## Behavior

1. A **skill** is one document — a `SKILL.md`, YAML frontmatter and a body —
   that tells a generic agent how to do one kind of work. It is named in
   kebab-case, and the name is how a task loads it.
2. An **agent** has no identity: it is an agent CLI, a model, an effort, a
   brief and a set of skills — and the CLI and the model are required, so
   every agent names both (011). A **seat** says only where it sits. The
   author of a specification, of a fix and of a release are all `author`,
   and differ only in the skills they hold.
3. There are three seats. `orchestrator` belongs to a goal; `author` and
   `reviewer` belong to a task. A task takes exactly one author, and the
   schema holds it to that. Reviewers are not required: the orchestrator
   agrees the review with the user task by task (003), and a task with
   nothing to review — a release, a dependency bump the suite already judged —
   is staffed with none and approved as soon as its author asks (001).
4. Ariadne ships a catalog of seventeen skills, in four scopes:
   - **orchestrate** — `orchestration`, the orchestrator's own playbook;
   - **produce** — `spec-writing`, `coding`, `debugging`, `refactoring`,
     `testing`, `documentation`, `research`;
   - **review** — `code-review`, `spec-review`, `security-review`,
     `performance-review`, `architecture-review`;
   - **operate** — `release`, `dependency-upgrade`, `migration`, `triage`.
5. Each shipped document lives in the code
   (`crates/ariadne-store/skills/<name>/SKILL.md`) and is seeded with a `NULL`
   document. So a reworded skill reaches every database without a migration,
   and a reset drops the row's document rather than copying a default into it.
6. Seeding is by name and runs on every open: a shipped skill the database
   lacks is added on the shipped text, and no document the database holds is
   touched — an edited document stays edited, and a deleted skill of the
   user's own stays deleted. A skill of the user's own under a name the
   catalog gained is adopted: the row becomes a built-in, its text stays as
   the override, and a reset of it goes to the shipped document. That is what
   carries an old database across a release that ships a new skill.
7. A skill Ariadne ships is reset, never deleted. A skill of the user's own is
   deleted, never reset: nothing ships under its name to go back to.
8. A skill still loaded by a staffed agent cannot be deleted, and an agent
   cannot be staffed on a skill nothing answers to.
9. A skill has a seat, read off its name and stored in no column
   (`Skill::seat`): `orchestration` is the orchestrator's, every other skill
   a task agent's. The store refuses a task agent staffed on the
   orchestrator's skill, and the launcher loads that skill from the store for
   every orchestrator session — indexed and written to disk the way a task
   agent's skills are (006, 007) — so an edit or a reset of it reaches the
   next launch.
10. A skill document obeys the STE rules and the size caps of spec 006, the
    same as every other default text.
11. The orchestrator staffs each task: it names the skills of each agent and
    the model each runs on (011), may size the effort beside it, and may add
    a brief that the task itself does not carry. How the task ends is agreed
    the same way (005).
12. Every user-facing skill action exists in both the CLI (`ariadne skill`)
    and the desktop app, per the parity rule of spec 015.

## Acceptance criteria

- A fresh database carries every shipped skill, each on its own text
  (`store.rs::a_fresh_database_is_seeded_with_every_shipped_skill_on_its_own_text`,
  `skill_documents.rs::every_shipped_skill_is_seeded_and_describes_itself`).
- A shipped skill runs on the shipped document until one is set, and a reset
  goes back to it
  (`skill_documents.rs::a_shipped_skill_runs_on_the_document_ariadne_ships`).
- The two refusals tell a shipped skill from one of the user's own
  (`store.rs::skill_crud_and_the_two_refusals_that_tell_them_apart`,
  `skill_documents.rs::a_skill_of_your_own_is_deleted_rather_than_reset`,
  `::a_shipped_skill_cannot_be_deleted`).
- A skill an agent still loads cannot be deleted
  (`store.rs::a_skill_an_agent_still_loads_cannot_be_deleted`), and an agent
  cannot be staffed on a skill nothing answers to
  (`store.rs::an_agent_cannot_be_staffed_on_a_skill_nothing_answers_to`).
- A task agent cannot be staffed on the orchestrator's skill, at creation or
  by a later edit
  (`store.rs::a_task_agent_cannot_be_staffed_on_the_orchestrators_skill`).
- A new shipped skill reaches an existing database on its next open, and
  nothing the database held is touched
  (`store.rs::a_new_shipped_skill_reaches_an_existing_database_on_reopen`,
  `::a_reopen_reseeds_no_row_the_database_already_holds`,
  `::a_user_skill_under_a_shipped_name_becomes_a_built_in_on_its_own_text`).
- An orchestrator session indexes `orchestration` and holds its document in
  the run directory
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`),
  and an edit of it reaches the next launch
  (`skill_documents.rs::an_edited_orchestration_skill_reaches_the_next_launch`).
- A task staffed with no reviewer is approved as soon as its author asks
  (`unreviewed_tasks.rs::a_task_with_no_reviewer_is_approved_as_soon_as_its_author_asks`).
- Every shipped skill is named once and describes itself
  (`defaults.rs::every_shipped_skill_is_named_once_and_describes_itself`), and
  every document is within its cap (`defaults.rs::skill_size_caps_hold`).
- An agent is written on the pin it was given, whole
  (`store.rs::an_agent_is_written_on_the_pin_it_was_given_whole`).
- A staffed agent's skills reach it as an index and as documents on disk
  (`prompts.rs::a_spawned_author_is_briefed_from_the_builtin_template`).

## Sources

`crates/ariadne-store/skills/` (the shipped documents),
`crates/ariadne-store/src/skills.rs`, `crates/ariadne-store/src/task_agents.rs`,
`crates/ariadne-store/src/defaults.rs` (`BUILTIN_SKILLS`,
`ORCHESTRATION_SKILL`), `crates/ariadne-store/src/entities.rs`
(`Skill::seat`), `crates/ariadne-daemon/src/launcher.rs` (the orchestrator's
skill), `crates/ariadne-core/src/lib.rs` (`Seat`).
