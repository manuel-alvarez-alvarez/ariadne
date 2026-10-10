---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-10-10
areas: [prompts, store, core, mcp, daemon]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b, a69b953f, 03f9c8b7, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/src/agents/handoff.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-core/src/lib.rs
---

# Prompts and Simplified Technical English

Every word Ariadne hands an agent: which layer owns it, who may edit it, and
the English all of it is written in.

## Scope

In: the layers of text (system prompt, skill index, lifecycle briefing), what
each layer states, rendering and placeholder validation, the STE rules, the
size caps, and the handoff text a session switch hands the new agent in place
of the conversation it cannot resume.

Out: the wording of any one procedure — those belong to the spec of the thing
they describe (003, 030) — what a skill is (017), and the switch itself
(021).

## Behavior

1. Text reaches an agent in three layers, and each rule is stated in exactly
   one of them:
   - the **MCP session rules**, which every session receives before its first
     prompt, whatever its seat: that Ariadne is reached only through its
     tools, that a client which defers tools loads the ones it needs in one
     tool search before the first call, whether a session works alone or
     waits on the user, when it may ask anyway, how few turns to take, and
     the English to write in (013);
   - the **system prompt**, which states what a seat owes from its first read
     to the call that ends its turn, and then indexes the skills this agent
     was staffed with (017) — for the orchestrator, the one skill its seat
     fixes by name;
   - the **lifecycle briefing**, which carries the values of one goal, task
     or column and whatever is only true of this moment: the column's
     instructions and the summary of the column before it (030), who sent a
     relayed message, the task it names and where to answer it (018).
2. The system prompt is the code's, one text per seat. Nothing an agent runs
   under carries a lifecycle text of its own: what makes one agent differ
   from another in the same seat is the skills it holds. No seat text carries
   a playbook step — the orchestrator's phases are the `orchestration` skill
   (017), and its seat text keeps only what no skill edit may take away: the
   plan is made with the user, no code is written, a blocked point goes to
   the user. The `agent` seat text is the one text every column's agent runs
   under: read the task and the column, do the column's work, and end the step with `complete_step` or
   `fail_step` (030). What a column does — build, review, merge, keep a
   request — is its skill's to say. The `reviewer` seat text is the pull
   request review session's (029).
3. The skills that own a step state its checks: an agent runs the tests and the lint
   of the crates and packages its step changed once, before the commit. After
   the commit, it leaves `git status` empty and the repository's generate
   step unchanged. No run of the whole suite is the develop column's own: the
   `code-review` skill runs it once before each verdict, and the `merge`
   skill runs it once after the rebase and before the fast-forward (030). A
   skill scopes the step it owns to what the change touched and names no
   other column's run, so the documents cannot disagree about who runs what.
   The same text states how the branch is committed: one commit for the
   task, one more commit for each answer to a review, and no amend of a
   pushed commit. A skill carries only the commit of the step it owns:
   `coding` commits the task it built, and `refactoring` the moves it made.
   No skill divides a task into slices or small commits.
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
   defaults rather than at a save. The kinds are the orchestrator's briefing,
   resume and attention, `IncomingMessage`, `StepBriefing`, `StepReturn` and
   `AgentResume` (`ariadne_core::PromptKind`).
8. Default texts, shipped skills, and workflow documents use ASD-STE100 Simplified
   Technical English with at most 25 words a sentence. MCP session rules use one
   instruction to a sentence, the imperative for an instruction, the active
   voice, at most 20 words a sentence, one meaning per word, a list for a
   sequence of steps.
9. Tests read sentence length and a
   list of banned words (`utilise`, `prior to`, `in order to`, `ensure`,
   `should`, `may`).
10. After a question, end your turn. Do not poll `read_messages`. Ariadne
    delivers the answer as a new turn.
11. STE binds what the agents write too — turn text and visible reasoning,
    task titles and descriptions, step reasons, failure reasons, commit
    subjects and bodies, and pull request text — and that rule lives in the
    session rules, where no edit of a skill can remove it.
12. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion. The
    shipped skill documents are capped on their own scale, since a skill is
    read once and on purpose rather than carried by every turn.
13. The handoff is a text built from a session's stored events for a
    session switched to another coding agent (021): no ACP agent can resume
    another's conversation, so the new agent gets this instead, as its
    history. `handoff_text` (`crates/ariadne-daemon/src/agents/handoff.rs`)
    folds the events the same way the console does (008), and opens the text
    with one line that names it as the history of the session the agent
    continues and tells it to go on from where that history ends.
14. One entry follows per kept event, oldest first, each under its own
    marker: a `user_prompt_submit` whole under `user` where its source is
    `console`, and folded to its first six lines with a count of the rest
    under `daemon` where its source is `daemon`; an `agent_message` whole
    under `agent`; a `plan` as a checklist, headed by how many of its entries
    are complete; a paired `pre_tool_use` and `post_tool_use` as the head
    line the console draws for the call — the command, the path, the
    pattern, or the title — then its output and its diff, each folded to
    their last ten lines with a count of what is hidden; a call with no end
    renders its head alone; a `permission_request` and its
    `permission.replied` as one line, the head of the call and the option
    chosen; and a `session.error` as one line, its message.
    `agent_thought`, `stop`, `session_start` and `session_end` are left out.
15. Every entry is fenced in a character reserved to the fence alone, and
    stripped from the entry's own text first, so nothing a tool's output or
    a diff carries can forge the fence and close the block early.
16. The text is kept under a budget of characters by entry priority, then
    recency for ties, and an entry is never cut in
    the middle. A line in the dropped entries' place counts how many there
    were. A budget that fits every entry writes no such line.

## Acceptance criteria

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`), and
  the rules are read line by line
  (`::the_ste_rules_are_read_off_a_text_line_by_line`).
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
- The `agent` seat text moves the task with the two step calls and states
  the division of the checks
  (`defaults.rs::the_agent_seat_text_moves_the_task_with_the_two_step_calls`),
  the `merge` skill carries the one run of the whole suite between the
  rebase and the fast-forward
  (`::the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself`),
  with a squash onto the merge base and a guard before the fast-forward
  (`::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`), and
  no shipped skill sends a develop agent to the whole suite
  (`::a_skill_scopes_its_own_checks_to_what_the_task_changed`).
- The seat text and the two skills that name a commit make the task one
  commit and amend nothing that is pushed
  (`defaults.rs::a_task_is_one_commit_and_nothing_amends_a_pushed_one`).
- The review skill starts its checks before the read and ends on a step call
  (`defaults.rs::the_review_skill_starts_its_checks_before_the_read_and_ends_on_a_step_call`).
- Every default names only placeholders its kind can fill in
  (`defaults.rs::every_default_names_only_placeholders_its_kind_can_fill_in`),
  and every allowed placeholder is one a builder actually passes
  (`prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).
- Broken template syntax still renders
  (`prompts.rs::broken_syntax_passes_through`,
  `::an_unknown_placeholder_travels_verbatim`,
  `::an_empty_template_renders_to_nothing`).
- Every default briefing is its template with the values put in
  (`prompts.rs::every_default_briefing_is_its_template_with_the_values_put_in`).
- A seat's prompt carries what the seat owes and nothing of a skill
  (`skill_documents.rs::a_seat_prompt_carries_only_what_the_seat_owes`).
- The orchestrator system prompt holds no playbook step; the phases and their
  order are the `orchestration` skill's
  (`defaults.rs::the_orchestrator_seat_text_holds_no_playbook_step`,
  `::the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts`),
  and an orchestrator session indexes that skill
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`).
- The index adds one line per skill, and the path it names holds the document
  (`prompts.rs::a_started_column_agent_is_briefed_from_the_builtin_template`).
- Every shipped skill document is within its cap (`defaults.rs::skill_size_caps_hold`).
- Every session is told to load a deferred tool before it calls it
  (`mcp.rs::every_session_is_told_to_load_a_deferred_tool_before_it_calls_it`).
- A console-sourced prompt renders whole and a daemon-sourced one folds to
  six lines with a count of the rest
  (`handoff.rs::a_console_prompt_renders_whole_and_a_daemon_prompt_folds_to_six_lines_with_a_count`),
  and an agent message renders whole
  (`::an_agent_message_renders_whole`).
- A thought is left out of the handoff
  (`handoff.rs::a_thought_is_left_out`), and so are `stop`, `session_start`
  and `session_end`
  (`::stop_session_start_and_session_end_are_left_out`).
- A tool call and its end render as one entry with the head and the folded
  output (`handoff.rs::a_tool_call_and_its_end_render_as_one_entry_with_the_head_and_the_folded_output`),
  its diff folds the same way
  (`::a_diff_is_folded_the_same_way_as_output`), and a call with no end
  renders its head alone (`::a_call_with_no_end_renders_its_head_alone`).
- A plan renders as a checklist (`handoff.rs::a_plan_renders_as_a_checklist`).
- A permission request renders as one line with the option chosen
  (`handoff.rs::a_permission_request_renders_as_one_line_with_the_option_chosen`),
  and a session error renders as one line with its message
  (`::a_session_error_renders_as_one_line_with_its_message`).
- An entry's own text cannot forge the fence and close its block early
  (`handoff.rs::an_entrys_own_text_cannot_forge_a_fence_and_close_the_block_early`).
- A budget that fits every entry writes no count line
  (`handoff.rs::a_budget_that_fits_every_entry_writes_no_count_line`), the
  newest entries survive a small budget behind one line that counts what
  was left out
  (`::the_newest_entries_survive_a_small_budget_and_one_line_counts_what_was_left_out`),
  and no entry is cut in the middle at any budget from zero to the whole
  text (`::no_entry_is_cut_in_the_middle_at_any_budget`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text and the STE rules),
`crates/ariadne-daemon/src/agents/prompts.rs` (assembly),
`crates/ariadne-daemon/src/agents/handoff.rs` (the handoff text of a session
switch),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules),
`crates/ariadne-daemon/src/agents/mod.rs` (`write_skills`).
