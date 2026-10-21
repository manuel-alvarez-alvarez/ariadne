---
id: skills-and-staffed-agents
status: current
updated: 2026-10-21
areas: [store, api, cli, ui, daemon, prompts]
commits: [03f9c8b7, 29e6d84e]
tests:
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-daemon/src/agents/mod.rs
  - crates/ariadne-daemon/tests/it/home_skills.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-daemon/tests/it/workflow_pull_request.rs
  - crates/ariadne-daemon/tests/it/pull_request_reviews.rs
---

# Skills and staffed agents

Ariadne defines one agent type, the orchestrator — and even its playbook is a
skill, fixed by name rather than staffed. Every other agent is generic, and
becomes what its task needs by loading skills.

A task is staffed one agent per column of its goal's workflow (030).

## Scope

In: what a skill is, the catalog Ariadne ships, who may edit one, how a task
staffs an agent on skills, and what a seat means once identity is gone.

Out: how a skill document reaches the agent (007), how the index is
written into the system prompt (006), and the lifecycle the seats sit in
(001, 030).

## Behavior

1. A **skill** is one document — a `SKILL.md`, YAML frontmatter and a body —
   that tells a generic agent how to do one kind of work, and the name is
   how a task loads it. A skill of the user's own is created under a name
   checked in two parts, on the route and in the store, since the name
   becomes a directory under the shared skills directory. First, its shape:
   1 to 64 characters of ASCII letters in either case, digits, hyphens and
   underscores, not leading with a hyphen or a dot. Second, its path: joined
   onto the skills directory (canonicalized, where a real one is on hand)
   it must stay a direct entry of it, both its parent and its file name
   unchanged by the join — which a name holding a separator, or naming `.`
   or `..`, never does. A name that fails either part is refused with 422
   rather than written, `../etc` or an absolute path included. A third check
   compares the name, case-insensitively, against every name already there,
   shipped skills included: a case-insensitive volume — the macOS default,
   and Windows's own — writes `Demo` and `demo` to the one directory, so the
   second of them is refused with 409 rather than silently overwriting the
   first's document on the next sync. That check and the row's insert run in
   one write transaction, on the store's one write connection, rather than
   the check reading through the separate read pool first: two concurrent
   creates of names differing only in case cannot both read no conflict and
   both insert, since the second's check waits for the first's whole
   transaction, commit or rollback, before it ever runs.
2. An **agent** has no identity: it is a registry agent, a model, an
   effort, a brief and a set of skills. Its pin is one string,
   `<agent>:<model>`, where `<agent>` is the id of an agent in the daemon's
   ACP registry (011). The agent and the model are both required, so every
   agent names both. No agent kind is stored beside the pin. A **seat** says
   only where it sits. The agent of a develop column, of a review column and
   of a merge column are all `agent`, and differ only in the column they
   staff and the skills they hold.
3. There are four seats. `orchestrator` belongs to a goal; `agent` belongs
   to one column of one task; `reviewer` belongs to a pull request (029,
   rule 14); `keeper` belongs to a request of the user's own asked for by
   hand (032). A task has one agent per column of its goal's workflow (030):
   a task agent is written with its column (`step`) and its ordinal, which
   is the column's position in the task's staffing list. Whether a task is
   reviewed, and how it lands, is the workflow's to say, so the orchestrator
   asks the user neither (003).
4. Ariadne ships a catalog of sixteen skills, in four scopes:
   - **orchestrate** — `orchestration`, the orchestrator's own playbook;
   - **produce** — `spec-writing`, `coding`, `debugging`, `refactoring`,
     `documentation`, `research`;
   - **review** — `code-review`, `spec-review`, `performance-review`,
     `architecture-review`, `pr-reviewer`;
   - **operate** — `migration`, `conflict-resolution`, `merge`,
     `pr-babysit`.

   `pull-request` left the catalog once opening a request became the
   daemon's own tool (`open_pull_request`) rather than an agent's to read a
   procedure for: the `pr` column's agent makes that one call (030).

   The catalog holds what Ariadne is staffed on, so a skill nothing is ever
   staffed on leaves it. `release`, `dependency-upgrade`, `security-review`
   and `triage` were staffed on no agent in the first twenty-three goals and
   were taken out; the work each covered is a task like any other, staffed on
   the skills that do the work.

   A skill that is always staffed beside another is one skill. `testing`
   merged into `coding`: 92 of its 96 staffings sat on an agent that held
   `coding` too. `coding` states both, and the orchestrator makes one
   decision where it made two.
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
7. The same open takes out a built-in the catalog no longer holds, so that
   dropping a skill reaches an old database the way adding one does. A skill
   that merged into another goes first: the staffings that named it name the
   skill that does its work now, and an agent staffed on both keeps one row.
   A row still on the shipped text holds nothing of anybody's and goes. A row
   somebody wrote a document over keeps their text and becomes a skill of
   their own, theirs to delete and no longer resettable. A row a staffed
   agent still loads stays a built-in, so an old task still reads as the
   skills it ran on; it has no text behind it and reads as empty, and it
   goes on the open after the last task naming it is deleted.
8. A skill Ariadne ships is reset, never deleted. A skill of the user's own is
   deleted, never reset: nothing ships under its name to go back to.
9. A skill a staffed agent still loads, or a goal's own snapshot still names
   in a column, cannot be deleted while the goal either belongs to is not
   yet `completed` or `cancelled`; a workflow's own column names a skill
   unconditionally, with no goal to go terminal, and keeps blocking the
   delete regardless. Once a goal ends, nothing more will ever staff a task
   off its agents or its snapshot, so a skill only such a goal still names
   is free to delete. An agent cannot be staffed on a skill nothing answers
   to.
10. A skill has a seat, read off its name and stored in no column
    (`Skill::seat`): `orchestration` is the orchestrator's, `pr-reviewer` is
    loaded by the daemon itself for a review session, and every other skill
    is a column agent's — `pr-babysit` included, which the `pr` column of
    `develop-review-pr` stages (030) and which the daemon also loads for a
    keeper session on a request asked for by hand (032). The store refuses
    a task agent staffed on `orchestration` or `pr-reviewer`, and
    `list_skills` lists neither (013). Nor does it list a skill with no
    document, shipped or written, or one of blank space only: a dropped
    skill such as `pull-request` (rule 7) keeps its row for an old staffing,
    but the catalog never offers that row as a fresh choice (013). The launcher loads the
    orchestrator's skill from the store for every orchestrator session —
    indexed, and reached through the run
    directory's link the way a task agent's skills are (006, 007) — so an
    edit or a reset of it reaches the next launch and changes what a running
    session reads when it opens the document again.
11. A skill document obeys the STE rules and the size caps of spec 006, the
    same as every other default text. It also obeys the rules the seat texts
    state, and repeats none of them: no column skill says to work in the
    task's worktree, and `orchestration` does not say to plan with the user,
    since the `agent` and `orchestrator` seat texts own those words alone
    (006 rule 5). No skill divides a task into slices or small commits.
    `coding` builds the whole task and commits it once, adds a new commit on
    a return from review and never amends, and `refactoring` holds every
    move in one commit (006). A skill that runs checks says which: the
    checks of what it changed. How a check runs — in the foreground, never
    polled, its output in a log outside the worktree — is the session
    rules' (006, 013), and no skill repeats it. `coding` ends its search
    step when the agent can name each definition it changes and its
    callers, not when a tool named every file it opened. Every review skill
    — `code-review`, `pr-reviewer`, `spec-review`, `architecture-review` and
    `performance-review` — marks a blocking or must-fix finding with a
    trigger, the input or state that causes it, the failure it causes, and
    the criterion, behavior or written rule it breaks. A finding with no
    nameable trigger is downgraded to its non-blocking weight and marked
    unverified, and every non-blocking finding of one review is capped at
    five. Each scopes its reading to the change under review, reuses
    evidence that already covers it — the column report's, for the four
    task-column skills, and its own prior round's, for `pr-reviewer` —
    rather than re-deriving it, and reads further only to judge a risk it
    can name. Each writes its verdict first, with no preamble, no process
    narration and no closing summary. A skill that ends a column ends on
    `complete_step` or `fail_step` (030): `code-review` fails the step on a
    must-fix finding, never on an optional one, and edits no file; `coding`,
    `merge` and `pr-babysit` name their own report only as "the column
    report", never a field of it (006 rule 5) — `merge` alongside the base
    branch's sha, `pr-babysit` on a merge; every other produce skill that
    ends its own column — `debugging`, `refactoring`, `migration`,
    `conflict-resolution`, `documentation`, `research`, `spec-writing` —
    calls `complete_step` with the column report the same way; each other
    review skill fails it on a blocking finding.

    A skill document holds one text. The launcher links the run directory's
    `skills` folder onto the shared home copy that holds it (15), for the
    agent to read whole. The path is shared: writing through it edits the
    home copy for every session.
12. The orchestrator staffs each task: for every column it names the skills
    of the agent — the column's own where it names none — and the model the
    agent runs on (011), may size the effort beside it, and may add a brief
    that the task itself does not carry. Once the task is running, its
    playbook's last step switches an exhausted, stuck or unsuitable agent to
    another pin over `switch_session` (013), rather than leave the task to
    fail.
13. Every user-facing skill action exists in both the CLI (`ariadne skill`)
    and the desktop app, per the parity rule of spec 015.
14. A review session (029) is staffed by the daemon on the pin asked
    directly on the request's own row — there is no repository pin (025) —
    and loads `pr-reviewer` the way a column agent loads its skills; no
    orchestrator staffs it.
15. The daemon keeps one shared copy of every store skill under
    `<home>/skills` by default, or `skills_dir` when configured. Every sync
    writes each document as `<skills_dir>/<name>/SKILL.md`, including
    `orchestration` and `pr-reviewer`, and removes folders for names the
    store no longer has without removing `skills_dir` itself. It syncs after
    the store seeds shipped skills at startup and after every skill create,
    document set, reset or delete. A stored name that fails any part of
    rule 1, checked here against the skills directory itself canonicalized,
    is logged and skipped rather than written. A name's own directory is
    never a symlink, and its `SKILL.md` is never a symlink or a hard link
    sharing its data with a file of some other name entirely: every one of
    those is a plain directory and a plain file the sync itself made, so a
    name holding either instead is never a skill's own, and is replaced
    with a fresh one — never opened, followed, or written through.

    On Unix each is opened directly off a directory already held open by
    its descriptor, with the name resolved once and never again, so a path
    swapped for a symlink after this sync already checked it is still
    refused, not raced. The directory's own open, with `O_DIRECTORY` and no
    `O_NOFOLLOW` match, answers `ELOOP` on Linux for a symlink there and
    `ENOTDIR` on macOS for the same one — the two kernels disagree on which
    error it is, not on refusing it — and either answer clears the entry
    and makes a fresh directory in its place. `SKILL.md` is never opened by
    its own name at all: the document is written to a fresh file under a
    name of its own, directly under the skill's directory, and that file is
    renamed onto `SKILL.md` in one step, which replaces whatever was there
    — symlink, hard link or a plain file — without the old entry, or
    whatever else holds the same data a hard link would, ever being opened.
    The fresh file's own name carries eight random bytes, hex-encoded,
    rather than the daemon's PID: a name anybody could predict and plant
    ahead of the write would otherwise collide, and the write holds
    `O_EXCL` on it, so a planted file is skipped and a new random name
    tried, up to eight times, rather than failing the write there. A write
    that still fails — every attempt at a fresh name taken, or the rename
    itself refused — is cleaned up if it was left behind, and is never
    swallowed: a sync that failed any skill's write fails as a whole,
    naming every skill it could not write, rather than reporting success
    with one skill's document stale or missing.

    Each successful sync also links every skill folder, one link per name,
    into `<native-home>/.claude/skills` for Claude Code and OpenCode and
    `<native-home>/.agents/skills` for Codex and OpenCode. Missing native
    directories are created. A native entry is Ariadne's to replace or
    remove when it is a symlink that resolves under the canonical shared
    skills directory, or that resolves nowhere at all — broken, wherever it
    once pointed, so nothing of the user's is lost by clearing it; a link
    for a name no longer in the store is removed the same way, while every
    other entry is left alone. A file, directory, or a symlink that still
    resolves somewhere else is a collision: the sync logs an error naming
    the skill
    (`name`), the shared source path (`shared_path`), the native path
    (`native_path`), the kind of entry found there — `file`, `directory` or
    `symlink` (`entry_kind`) — and the safe outcome (`outcome`, always
    `left_untouched`), leaves it untouched, and continues with the other
    links. An invalid stored skill name or a failed document write is
    likewise logged with the skill's name (`name`) and the path it would
    have written (`path`). The daemon takes `native-home` from `HOME`,
    falling back to the platform account home as it does for other
    home-derived paths; tests inject a temporary native home rather than
    writing the account's real one.

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
- A name with `/`, `\`, `..`, a space, a leading dot or a leading hyphen is
  refused with 422 rather than written under the skills directory, a mixed-case
  name with a digit and an underscore is accepted, and the store refuses the
  same names the route does:
  `skill_documents.rs::a_skill_name_outside_the_shape_is_refused`,
  `::a_mixed_case_name_with_an_underscore_and_a_digit_is_accepted`,
  `store.rs::a_skill_name_must_be_shaped_right_and_stay_under_the_skills_directory`.
- A name differing only in case from an existing one, shipped or the user's
  own, is refused with 409, so the first name's document is never
  overwritten, and the same holds when two such creates race: exactly one
  of two names differing only in case is ever created, never both:
  `skill_documents.rs::a_name_differing_only_in_case_from_an_existing_skill_is_refused`,
  `store.rs::a_skill_name_differing_only_in_case_from_an_existing_one_is_refused`,
  `::concurrent_creates_differing_only_in_case_do_not_both_succeed`.
- A name that passes the shape and path check is still never written through a
  symlink already sitting at its directory, and lands at its own, now real,
  directory once the symlink is cleared:
  `skill_documents.rs::a_symlinked_skill_directory_is_never_written_through`,
  `::a_symlinked_skill_md_is_never_written_through`.
- A hard-linked `SKILL.md` is replaced by a fresh file rather than written
  through, leaving the data it shared with another name untouched, and a
  directory symlink left at a skill's own name at write time is replaced
  with a real directory on both of `ELOOP`'s and `ENOTDIR`'s own answer to
  the same open:
  `agents::symlink_safe::tests::a_hard_linked_skill_md_is_replaced_rather_than_written_through`,
  `::a_directory_symlink_left_at_write_time_is_replaced_rather_than_followed`.
- A file already sitting at the old, predictable temp name does not block a
  write, which tries a fresh random name instead, and a write that still
  fails after every attempt is reported rather than swallowed, the same when
  `sync_skills` runs the whole catalog:
  `agents::symlink_safe::tests::a_file_at_the_old_predictable_temp_name_does_not_block_the_write`,
  `::a_persistently_failing_write_is_reported`,
  `skill_documents.rs::sync_skills_reports_a_persistently_failing_write`.
- A skill an agent still loads cannot be deleted
  (`store.rs::a_skill_an_agent_still_loads_cannot_be_deleted`), and an agent
  cannot be staffed on a skill nothing answers to
  (`store.rs::an_agent_cannot_be_staffed_on_a_skill_nothing_answers_to`).
- A skill a nonterminal goal's agent or snapshot still names stays blocked,
  and the same skill is freed for deletion once that goal ends
  (`store.rs::a_skill_named_only_by_a_terminal_goals_snapshot_and_agent_can_be_deleted`,
  `tests/it/skill_documents.rs::a_skill_is_freed_for_deletion_once_the_goal_loading_it_ends`).
- A task agent cannot be staffed on the orchestrator's skill or the review
  session's, at creation or by a later edit, and can load `pr-babysit`
  (`store.rs::a_task_agent_cannot_be_staffed_on_the_orchestrators_skill`,
  `::a_task_agent_cannot_be_staffed_on_the_reviewer_sessions_skill`).
- The `pr` column's agent indexes `pr-babysit`
  (`workflow_pull_request.rs::the_pr_column_opens_the_request_once_and_keeps_it`),
  and the skill opens the request and ends on the merge or the close
  (`defaults.rs::the_pr_babysit_skill_opens_the_request_and_ends_on_the_merge_or_the_close`).
- A review session is staffed on the review pin and indexes
  `pr-reviewer`
  (`pull_request_reviews.rs::an_open_request_i_review_gets_one_session_detached_at_its_head`),
  and the skill ranks its findings and never approves
  (`defaults.rs::the_pr_reviewer_skill_ranks_its_findings_and_never_approves`).
- A new shipped skill reaches an existing database on its next open, and
  nothing the database held is touched
  (`store.rs::a_new_shipped_skill_reaches_an_existing_database_on_reopen`,
  `::a_reopen_reseeds_no_row_the_database_already_holds`,
  `::a_user_skill_under_a_shipped_name_becomes_a_built_in_on_its_own_text`).
- A dropped shipped skill leaves an existing database on its next open,
  except where a document was written over it, which becomes a skill of the
  user's own, or a staffed agent still loads it, which holds the row in place
  (`store.rs::a_dropped_shipped_skill_leaves_an_existing_database_on_reopen`).
- A merged skill hands its staffings to the skill that absorbed it
  (`store.rs::a_merged_skill_hands_its_staffings_to_the_skill_that_absorbed_it`).
- An orchestrator session indexes `orchestration` and holds its document in
  the run directory
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`),
  and an edit of it reaches the existing and next launches
  (`skill_documents.rs::an_edited_orchestration_skill_reaches_the_next_launch`).
- The playbook staffs one agent per column and asks no reviewer question
  (`defaults.rs::the_orchestration_skill_staffs_one_agent_per_column_and_asks_no_reviewer_question`).
- The playbook switches a struggling agent's session to a model one rank
  up, and tells the user
  (`defaults.rs::the_orchestration_skill_switches_a_struggling_agents_session`).
- Every shipped skill is named once and describes itself
  (`defaults.rs::every_shipped_skill_is_named_once_and_describes_itself`),
  every document is within its cap (`defaults.rs::skill_size_caps_hold`),
  and every shipped workflow stages shipped skills
  (`::every_shipped_workflow_parses_and_stages_shipped_skills`).
- No skill repeats how a check runs: that is the session rules'
  (`defaults.rs::the_skills_leave_how_a_check_runs_to_the_session_rules`,
  `mcp.rs::each_shared_rule_is_written_in_its_owner_alone`).
- `coding` ends its search step on what the agent knows
  (`::the_coding_search_step_ends_on_what_the_agent_knows`).
- `coding` and the other produce skills that end their own column name the
  column report there and restate no field of it
  (`::a_produce_skill_names_the_column_report_where_it_ends_a_column`).
- No shipped skill divides a task, `coding` adds a new commit on a return
  and never amends, and `refactoring` names one commit
  (`defaults.rs::a_task_is_one_commit_and_nothing_amends_a_pushed_one`).
- The review skill ends on a step call, and the merge skill lands in order
  (`defaults.rs::the_review_skill_reuses_matching_evidence_and_ends_on_a_step_call`,
  `::the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself`).
- The orchestration skill selects within each column's permitted ranks after a
  case-by-case scope and complexity assessment, and the shipped merge and PR
  skills never force-push, including with `--force-with-lease`
  (`defaults.rs::the_orchestrator_staffs_each_column_on_its_rank`,
  `::the_merge_skill_forbids_every_force_push`,
  `::the_pr_babysit_skill_opens_the_request_and_ends_on_the_merge_or_the_close`).
- Every review skill names a trigger on a blocking or must-fix finding,
  downgrades and marks unverified a finding with none, and caps its
  non-blocking findings at five; the four task-column skills also pin
  their reading to the diff and reuse the column report's evidence
  (`defaults.rs::every_review_skill_names_a_trigger_and_caps_its_optional_findings`).
- An agent is written on the pin it was given, whole
  (`store.rs::an_agent_is_written_on_the_pin_it_was_given_whole`), and no
  table names an agent by anything but the registry id at the head of its
  pin (`store.rs::the_schema_names_agents_by_registry_id_alone`).
- A staffed agent's skills reach it as an index and as documents on disk
  (`prompts.rs::a_started_column_agent_is_briefed_from_the_builtin_template`,
  `workflow_steps.rs::the_orchestrator_reads_the_workflow_and_agents_load_their_own_skills`).
- No agent reads a `pull-request` skill: opening a request is the daemon's
  own tool now. An old staffing on it — the row a database written before
  this release still carries — keeps its row, because the task that staffed
  it still names it, and reads as empty, since nothing ships under the name
  any more
  (`skill_documents.rs::an_old_staffing_on_pull_request_reads_as_empty`,
  `store.rs::an_old_staffing_on_pull_request_keeps_its_row_and_reads_as_empty`).
  `list_skills` leaves that row out of the catalog, while a skill with a
  document, shipped or written, still lists
  (`tools.rs::the_skill_catalog_excludes_a_skill_with_no_document`).
- The home copy starts with every skill's current document, follows creates,
  edits and resets, and removes deleted and stale folders while retaining the
  other skill folders (`home_skills.rs::the_home_skill_copy_tracks_the_catalog_and_its_documents`).
- Every shared skill is linked into both native agent directories, stale
  Ariadne links are removed without touching a user's neighboring entry, and
  a user collision is logged with the skill, both paths, the entry kind and
  the outcome, and left untouched while the other directory
  continues syncing (`home_skills.rs::the_home_skill_copy_tracks_the_catalog_and_its_documents`,
  `::a_user_native_skill_collision_is_logged_and_left_untouched`).
- A broken native link, and one that resolves into the shared skill folder
  through another link rather than naming it directly, are both replaced
  with no collision logged; a working link to somewhere else of the user's
  own still logs the collision and keeps its target
  (`home_skills.rs::a_broken_native_skill_link_is_replaced_without_a_log`,
  `::a_native_skill_link_resolving_into_the_shared_folder_is_replaced_without_a_log`,
  `::a_working_unrelated_native_skill_link_is_logged_and_left_untouched`).

## Sources

`crates/ariadne-store/skills/` (the shipped documents),
`crates/ariadne-store/src/skills.rs`, `crates/ariadne-store/src/task_agents.rs`,
`crates/ariadne-store/src/defaults.rs` (`BUILTIN_SKILLS`,
`ORCHESTRATION_SKILL`, `PR_BABYSIT_SKILL`, `PR_REVIEWER_SKILL`),
`crates/ariadne-store/src/entities.rs`
(`Skill::seat`), `crates/ariadne-daemon/src/launcher.rs` (the orchestrator's
skill), `crates/ariadne-core/src/lib.rs` (`Seat`).
