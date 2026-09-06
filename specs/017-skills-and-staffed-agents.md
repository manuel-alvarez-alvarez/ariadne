---
id: skills-and-staffed-agents
status: current
updated: 2026-09-06
areas: [store, api, cli, ui, daemon, prompts]
commits: [a69b953f, 083c3132, 26f49633, e7997a4f, 87fa62cf]
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/tests/skill_documents.rs
  - crates/ariadne-daemon/tests/adapters.rs
  - crates/ariadne-daemon/tests/prompts.rs
---

# Skills and staffed agents

Ariadne defines one agent type, the orchestrator. Every other agent is
generic, and becomes what its task needs by loading skills.

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
   brief and a set of skills. A **seat** says only where it sits. The author
   of a specification, of a fix and of a release are all `author`, and differ
   only in the skills they hold.
3. There are three seats. `orchestrator` belongs to a goal; `author` and
   `reviewer` belong to a task. A task takes exactly one author and any number
   of reviewers, and the schema holds it to that.
4. Ariadne ships a catalog of sixteen skills, in three scopes:
   - **produce** — `spec-writing`, `coding`, `debugging`, `refactoring`,
     `testing`, `documentation`, `research`;
   - **review** — `code-review`, `spec-review`, `security-review`,
     `performance-review`, `architecture-review`;
   - **operate** — `release`, `dependency-upgrade`, `migration`, `triage`.
5. Each shipped document lives in the code
   (`crates/ariadne-store/skills/<name>/SKILL.md`) and is seeded into an empty
   database with a `NULL` document. So a reworded skill reaches every database
   without a migration, and a reset drops the row's document rather than
   copying a default into it.
6. Seeding runs only while the table is empty. Once a database holds skills, a
   deleted skill stays deleted and an edited document stays edited.
7. A skill Ariadne ships is reset, never deleted. A skill of the user's own is
   deleted, never reset: nothing ships under its name to go back to.
8. A skill still loaded by a staffed agent cannot be deleted, and an agent
   cannot be staffed on a skill nothing answers to.
9. A skill document obeys the STE rules and the size caps of spec 006, the
   same as every other default text.
10. The orchestrator staffs each task: it names the skills of each agent, and
    may size the model and effort per agent (011) and add a brief that the
    task itself does not carry.
11. Every user-facing skill action exists in both the CLI (`ariadne skill`)
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
- Every shipped skill is named once and describes itself
  (`defaults.rs::every_shipped_skill_is_named_once_and_describes_itself`), and
  every document is within its cap (`defaults.rs::skill_size_caps_hold`).
- An agent is written on the pin it was given, and auto where it was given
  none (`store.rs::an_agent_is_written_on_the_pin_it_was_given_and_auto_where_it_was_given_none`).
- A staffed agent's skills reach it as an index and as documents on disk
  (`prompts.rs::a_spawned_author_is_briefed_from_the_builtin_template`).

## Sources

`crates/ariadne-store/skills/` (the shipped documents),
`crates/ariadne-store/src/skills.rs`, `crates/ariadne-store/src/task_agents.rs`,
`crates/ariadne-store/src/defaults.rs` (`BUILTIN_SKILLS`),
`crates/ariadne-core/src/lib.rs` (`Seat`).
