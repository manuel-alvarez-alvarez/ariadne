---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-09-21
areas: [prompts, store, core, mcp]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b, a69b953f, 03f9c8b7, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-core/src/lib.rs
---

# Prompts and Simplified Technical English

Every word Ariadne hands an agent: which layer owns it, who may edit it, and
the English all of it is written in.

## Scope

In: the layers of text (system prompt, skill index, lifecycle briefing,
landing briefing), what each layer states, rendering and placeholder
validation, the STE rules, and the size caps.

Out: the wording of any one procedure — those belong to the spec of the thing
they describe (003, 004, 005) — and what a skill is (017).

## Behavior

1. Text reaches an agent in four layers, and each rule is stated in exactly
   one of them:
   - the **MCP session rules**, which every session receives before its first
     prompt, whatever its seat: that Ariadne is reached only through its
     tools, that a client which defers tools loads the ones it needs in one
     tool search before the first call, whether a session works alone or
     waits on the user, when it may ask anyway, that code is found with
     `search_code` and `symbol` before a file is read (022) — only while the
     knowledge base is on — that memory is searched with `search_memory`
     before a discovery is repeated (019), how few turns to take, and the
     English to write in (013);
   - the **system prompt**, which states what a seat owes from its first read
     to the call that ends its turn, and then indexes the skills this agent
     was staffed with (017) — for the orchestrator, the one skill its seat
     fixes by name;
   - the **lifecycle briefing**, which carries the values of one goal, task or
     task and whatever is only true of this moment;
   - the **landing briefing**, which carries the procedure that ends a task,
     one per ending (005).
2. The system prompt is the code's, one text per seat. Nothing an agent runs
   under carries a lifecycle text of its own: what makes one agent differ from
   another in the same seat is the skills it holds. No seat text carries a
   playbook step — the orchestrator's ten phases are the `orchestration`
   skill (017), and its seat text keeps only what no skill edit may take
   away: the plan is made with the user, no code is written, a blocked point
   goes to the user.
   The reviewer text refreshes its detached worktree, then starts the whole
   suite, build and linters before the read, once per verdict. It keeps the
   worktree read-only, puts the judged SHA in every verdict and judges tests
   by reading them. Its resume starts the checks again and reads only commits
   after the last verdict's SHA. With no known SHA or unrelated HEAD, it uses
   `get_diff`.
3. The division of the checks is one of those rules, and the author's seat
   text is the one place it is stated: the author runs the tests and the lint
   of the crates and packages it changed once, before the commit. After the
   commit, it leaves `git status` empty and the repository's generate step
   unchanged. No run of the whole suite is its
   own — a reviewer runs it once before each verdict it gives (004), and the
   landing runs it once after the rebase and before the fast-forward (005). A
   skill scopes the step it owns to what the change touched and names neither
   run, so thirteen documents cannot disagree about who runs what. The landing
   briefing carries its own run, as a step of the procedure it belongs to.
   The same text states how the branch is committed: one commit for the task,
   one more commit for each review answer, and no amend (004). A skill carries
   only the commit of the step it owns: `coding` commits the task it built,
   and `refactoring` the moves it made. It repeats neither the review answer
   nor the amend. No skill divides a task into slices or small commits.
   When a memory is written is a skill's step to say, the way the tool of a
   step is: `coding`, `debugging`, `code-review` and `research` each call
   `save_memory` at the step that earns it, and each carries the
   same bar on what is worth keeping (019). `orchestration` calls
   `search_memory` while it explores a goal. No skill repeats the read rule
   the session rules state.
4. The skill index is one line per skill — its name, the summary its
   frontmatter states, and the path of its document in the run directory — and
   the instruction to read a document before doing the work it covers. The
   document itself stays on disk, so a broad set of skills costs an agent a
   few lines rather than a few pages. How each CLI is pointed at those files
   is 007.
5. The lifecycle briefings are Ariadne's own constants, read from the code on
   every launch and every resume. No route reads or writes one, and no row
   holds one. Because nothing is copied into the database, rewording a default
   reaches every session started after it.
6. A briefing is rendered by substituting `{name}` tokens. Rendering is
   lenient by construction: an unknown token, an unclosed brace and an empty
   template all render to something, and none of them fails a spawn.
7. Each kind declares the placeholders its builder fills in, and a default
   naming one outside its list fails the suite. Nothing hand-written reaches
   an agent any more — every text is the code's — so the check runs over the
   defaults rather than at a save.
8. Every agent-facing text is ASD-STE100 Simplified Technical English: one
   instruction to a sentence, the imperative for an instruction, the active
   voice, at most 25 words a sentence, one meaning per word, a list for a
   sequence of steps.
9. Two of those rules are read off the text by test: sentence length, and a
   list of banned words (`utilise`, `prior to`, `in order to`, `ensure`,
   `should`, `may`).
10. STE binds what the agents write too — turn text and visible reasoning,
    task titles and descriptions, review summaries, verdicts, failure reasons,
    commit subjects and bodies, and pull request text — and that rule lives in
    the session rules, where no edit of a skill can remove it.
11. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion. The
    shipped skill documents are capped on their own scale, since a skill is
    read once and on purpose rather than carried by every turn. A skill
    document holds a text for the knowledge base on and one for it off
    (017), and its cap holds the longer of the two.

## Acceptance criteria

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`).
- Every default text is within its cap and the totals
  (`defaults.rs::size_caps_hold`).
- A rule is stated in exactly one briefing
  (`defaults.rs::each_rule_is_stated_in_exactly_one_briefing`,
  `::a_seat_rule_is_stated_in_its_own_prompt_alone`), and no default repeats
  what the MCP server already tells every session
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`). The
  MCP session rules state whether a seat works alone or waits on the user,
  and when it may ask anyway
  (`mcp.rs::only_the_orchestrator_is_told_to_ask`).
- The author's seat text states the division of the checks
  (`defaults.rs::the_author_scopes_its_checks_and_names_who_runs_the_whole_suite`),
  the `merge` landing carries the one run of the whole suite
  (`::the_direct_landing_runs_the_whole_suite_after_the_rebase_and_before_the_fast_forward`),
  with a later pass that reruns it only after a conflict or a base change to
  a task file, a squash onto the merge base and a guard before the
  fast-forward (005,
  `::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`),
  and no shipped skill sends an author to the whole suite
  (`::a_skill_scopes_its_own_checks_to_what_the_task_changed`).
- The seat text and the two skills that name a commit make the task one commit
  and each review answer one more. No skill repeats the review answer or the
  amend (`defaults.rs::a_task_is_one_commit_and_a_review_answer_is_one_more`).
- Every default names only placeholders its kind can fill in
  (`defaults.rs::every_default_names_only_placeholders_its_kind_can_fill_in`),
  and every allowed placeholder is one a builder actually passes
  (`prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).
- Broken template syntax still renders
  (`prompts.rs::broken_syntax_passes_through`,
  `::an_unknown_placeholder_travels_verbatim`).
- A seat's prompt carries what the seat owes and nothing of a skill
  (`skill_documents.rs::a_seat_prompt_carries_only_what_the_seat_owes`).
- The reviewer prompt and code-review skill start the full checks before the
  read, once per verdict
  (`defaults.rs::reviewer_checks_start_before_the_read_once_per_verdict`).
- Every reviewer verdict carries the judged SHA
  (`defaults.rs::every_reviewer_verdict_carries_the_sha_it_judged`), and its
  resume reads only commits after that SHA
  (`::a_reviewer_resume_reads_only_commits_since_its_last_verdict_sha`).
- The reviewer prompt and skill require a read-only judgment of each test
  (`defaults.rs::a_reviewer_judges_a_test_by_reading_it_without_changing_code`).
- The reviewer resume refreshes the branch before checks
  (`defaults.rs::reviewer_texts_refresh_the_named_branch_before_checks`)
  and falls back to the whole diff when HEAD does not follow its last SHA
  (`::a_reviewer_uses_the_whole_diff_when_head_does_not_follow_the_last_sha`).
- The orchestrator system prompt holds no playbook step; the phases and their
  order are the `orchestration` skill's
  (`defaults.rs::the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts`,
  `::the_orchestrator_staffs_a_plan_on_a_mix_of_agents`), and an
  orchestrator session indexes that skill
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`).
- The index adds one line per skill, and the path it names holds the document
  (`prompts.rs::a_spawned_author_is_briefed_from_the_builtin_template`).
- Every shipped skill document is within its cap, with the knowledge base on
  and off (`defaults.rs::skill_size_caps_hold`), and every skill that reads
  code names the knowledge tool of its own step while the knowledge base is
  on (`defaults.rs::every_skill_that_reads_code_names_the_knowledge_tools`).
- With the knowledge base off, no skill text and no session rule names a
  knowledge tool or `ariadne knowledge`
  (`defaults.rs::a_skill_names_no_knowledge_tool_when_the_knowledge_base_is_off`,
  `mcp.rs::the_knowledge_tools_are_not_listed_when_the_knowledge_base_is_off`).
- Every session is told to load a deferred tool before it calls it
  (`mcp.rs::every_session_is_told_to_load_a_deferred_tool_before_it_calls_it`).
- Every skill that learns names `save_memory` at its own step with the bar
  on what is worth keeping, and `orchestration` names `search_memory`
  (`defaults.rs::every_skill_that_learns_names_the_memory_tools`); the read
  rule stays the session rules' alone
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text and the STE rules),
`crates/ariadne-daemon/src/agents/prompts.rs` (assembly),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules),
`crates/ariadne-daemon/src/agents/mod.rs` (`write_skills`).
