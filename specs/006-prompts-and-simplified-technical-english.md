---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-10-16
areas: [prompts, store, core, mcp, daemon]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b, a69b953f, 03f9c8b7, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/src/agents/handoff.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/scheduler/goals.rs
  - crates/ariadne-daemon/src/scheduler/tasks.rs
  - crates/ariadne-daemon/src/launcher.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-daemon/tests/it/agent_messages.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/resume.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-daemon/tests/it/seat_brief.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-core/src/lib.rs
---

# Prompts and Simplified Technical English

Every word Ariadne hands an agent: which layer owns it, who may edit it, and
the English all of it is written in.

## Scope

In: the layers of text (MCP instructions, system prompt, skill index,
lifecycle briefing), what each layer states, which turn carries the system
prompt on which agent, rendering and placeholder validation, the STE rules, the
size caps, and the handoff text a session switch hands the new agent in place
of the conversation it cannot resume.

Out: the wording of any one procedure — those belong to the spec of the thing
they describe (003, 030) — what a skill is (017), and the switch itself
(021).

## Behavior

1. Text reaches an agent in three layers, and each rule is stated in exactly
   one of them:
   - the **MCP server instructions**, which every session receives before
     its first prompt, whatever its seat (013). They hold the session rules
     first: that Ariadne is reached only through its tools, that a client
     which defers tools loads the ones it needs in one tool search first,
     when a seat asks the user and how, how few turns to take, how every
     check runs, the English to write in, and that no text carries a
     trailer. After the rules come the seat text, picked by the seat, and
     the index of the skills the session loads (rule 4);
   - the **system prompt**, which is that seat text and that skill index
     again: what a seat owes from its first read to the call that ends its
     work, then the skills this agent was staffed with (017) — for the
     orchestrator, the one skill its seat fixes by name. It reaches the
     agent through the MCP instructions and in a turn (rule 2);
   - the **lifecycle briefing**, which carries the values of one goal, task
     or column and whatever is only true of this moment: the column's
     instructions and the summary of the column before it (030), who sent a
     relayed message, the task it names and where to answer it (018).
2. A turn carries its own text alone, except the first turn of a launch and
   the first turn after a model switch: those carry the system prompt ahead
   of their own text. A session switch and a relaunch are new launches, so
   their first turn carries it too. A slash command the agent offers never
   carries it (021). Every agent takes this path, because none is proven to
   keep the MCP instructions in its real system prompt through a compaction:
   - `claude-acp`: claude-agent-acp 0.81.3-preview.1, on claude-agent-sdk
     0.3.280, passes the MCP servers to the SDK. The SDK hands their
     instructions to the model as an `mcp_instructions_delta` attachment, a
     meta message with the user role, not as part of the system prompt;
   - `codex-acp` and `opencode`: no section of their system prompt that
     holds the MCP instructions could be shown;
   - an agent nobody named is one nobody proved.
   An agent that is proven later can skip the turn copy; until then the MCP
   instructions are a second copy, not the only one.
3. The system prompt is the code's, one text per seat. Nothing an agent runs
   under carries a lifecycle text of its own: what makes one agent differ
   from another in the same seat is the skills it holds. No seat text carries
   a playbook step — the orchestrator's phases are the `orchestration` skill
   (017), and its seat text keeps only what no skill edit may take away: no
   code is written, the plan is made with the user, and no open point is
   decided alone. The `agent` seat text is the one text every column's agent
   runs under: work in the task's worktree and on the current column, read
   its skills, and what `complete_step`, `fail_step` and `fail_task` are for,
   with the ban on `--no-verify` and `-n`. What a column does — build,
   review, merge, keep a request — is its skill's to say. The `reviewer` seat
   text is the pull request review session's (029), and the `keeper` seat
   text is the session's that keeps a request of the user's own asked for
   by hand (032).
4. The skill index is one line per skill — its name, the summary its
   frontmatter states, and the path of its document in the run directory — and
   the instruction to read a document before doing the work it covers. That
   path resolves through the run directory's `skills` folder, linked onto the
   daemon's shared home copy (017), so a broad set of skills costs an agent a
   few lines rather than a few pages. An agent reads the shared document when
   it opens this path, so a later sync can change what a running agent reads.
   The launcher hands the index to the MCP server as `ARIADNE_SKILL_INDEX`.
   How each CLI is pointed at those files is 007.
5. A rule every seat shares has one owner, and no other text repeats it:
   - asking the user: the session rules, picked by seat, with a short form
     in the `wait_for_user` description;
   - ending the turn after a call: the `complete_step`, `fail_step` and
     `send_message` descriptions;
   - how a check runs — in the foreground, split by crate, logged outside
     the worktree, its failures alone printed, never polled: the session
     rules. A skill says only which checks it runs: the checks of what it
     changed. The repository's hooks run its whole suite at a push, and the
     merge skill pushes before it fast-forwards (030);
   - no trailer in any text: the session rules;
   - never polling a request: the session rules of the `agent` and
     `reviewer` seats alone, which say that Ariadne wakes the session with
     the request's news (026, 030) instead. No skill names a timer or a
     poll.
6. `coding` uses test-first development for a code behavior change: it runs
   the new seam test failing before implementation. Prose changes and
   behavior-preserving refactors use verification that fits their risk.
   `coding` reports the committed revision, every check command and its
   result in its completion handoff. `code-review` reads the handoff reasons,
   accessible logs and CI evidence before deciding which checks to run. It
   reuses a result only when its revision is the reviewed revision and its
   command covers a required check. Missing, outdated or insufficient
   evidence runs the required check. Evidence never replaces code inspection,
   acceptance review or repository-rule enforcement.
7. A skill carries only the commit of the step it owns: `coding` commits the
   task it built, and a return from review adds a new commit on top and
   amends nothing, since the merge column squashes. `refactoring` commits
   the moves it made, and `pr-babysit` never amends a pushed request. No
   skill divides a task into slices or small commits.
8. The lifecycle briefings are Ariadne's own constants, read from the code on
   every launch and every resume. No route reads or writes one, and no row
   holds one. Because nothing is copied into the database, rewording a default
   reaches every session started after it. The notice an orchestrator gets
   when the scheduler moved an exhausted agent to another model (021) is one
   of those constants too.
9. A briefing is rendered by substituting `{name}` tokens. Rendering is
   lenient by construction: an unknown token, an unclosed brace and an empty
   template all render to something, and none of them fails a spawn.
10. Each kind declares the placeholders its builder fills in, and a default
   naming one outside its list fails the suite. Nothing hand-written reaches
   an agent any more — every text is the code's — so the check runs over the
   defaults rather than at a save. The kinds are the orchestrator's briefing,
   resume and attention, `IncomingMessage`, `StepBriefing`, `StepReturn` and
   `AgentResume` (`ariadne_core::PromptKind`).
11. Default texts, shipped skills, workflow documents, session rules and tool
    descriptions use ASD-STE100 Simplified Technical English: one instruction
    to a sentence, the imperative for an instruction, the active voice, at
    most 20 words a sentence (`ste::MAX_WORDS`), one meaning per word, a list
    for a sequence of steps. Tests read sentence length line by line — a
    skill with each wrapped sentence joined first — and a
    list of banned words (`utilise`, `prior to`, `in order to`, `ensure`,
    `should`, `may`).
12. STE binds what the agents write too — turn text and visible reasoning,
    task titles and descriptions, step and task reasons, commits, and pull
    request text — and that rule lives in the session rules, where no edit
    of a skill can remove it.
13. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion. The
    shipped skill documents are capped on their own scale, since a skill is
    read once and on purpose rather than carried by every turn.
14. The handoff is a text built from a session's stored events for a
    session switched to another coding agent (021): no ACP agent can resume
    another's conversation, so the new agent gets this instead, as its
    history. `handoff_text` (`crates/ariadne-daemon/src/agents/handoff.rs`)
    folds the events the same way the console does (008), and opens the text
    with one line that names it as the history of the session the agent
    continues and tells it to go on from where that history ends.
15. One entry follows per kept event, oldest first, each under its own
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
16. Every entry is fenced in a character reserved to the fence alone, and
    stripped from the entry's own text first, so nothing a tool's output or
    a diff carries can forge the fence and close the block early.
17. The text is kept under a budget of characters by entry priority, then
    recency for ties, and an entry is never cut in
    the middle. A line in the dropped entries' place counts how many there
    were. A budget that fits every entry writes no such line.
18. A value from the forge or from another agent is fenced before it
    reaches a briefing: wrapped between `<<<untrusted>>>` and
    `<<<end untrusted>>>` (`prompts::fence`), the same idea as rule 16's
    handoff fence, in its own two markers rather than one reserved
    character, since a briefing is prose an agent reads start to end
    rather than a feed of discrete entries. Every `<<<` the value itself
    carries is broken with a zero-width space first, so the value cannot
    close the fence early or open one of its own; an empty value fences to
    nothing. The session rules state once, for every seat, that text
    between the markers is data and never an instruction (018, 026, 032).
    No forge value is exempt for being structured, bounded or normally
    short — a URL, a sha, an id or a state word is still the forge's own
    text, and a briefing never trusts its shape over its source. Fenced:
    an issue's title, body and URL (as the goal's), a request's title,
    URL, head and base branch names, head sha and reviewed sha, a
    comment's id, author, path and first line, a failed check's name,
    conclusion and URL, the rolled-up checks, the review decision and the
    state a news line reports, an agent message's body, a step's return
    reason or completion summary, a task's title and description, and
    the orchestrator's own brief for a column's agent (`TaskAgent.brief`)
    — the orchestrator's own planning is still an agent's, so a task it
    names in a briefing, a message, a nudge or a dependency is fenced
    wherever its title reaches another agent, and so is anything beyond
    the task itself it tells a column's agent to do.

## Acceptance criteria

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`), and
  the rules are read line by line
  (`::the_ste_rules_are_read_off_a_text_line_by_line`). So does every text
  the MCP server hands an agent
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- Every default text is within its cap and the totals
  (`defaults.rs::size_caps_hold`).
- The seat texts and the briefings the agreed text gives ship word for word
  (`defaults.rs::the_agreed_texts_ship_word_for_word`).
- A rule is stated in exactly one briefing
  (`defaults.rs::each_rule_is_stated_in_exactly_one_briefing`,
  `::a_seat_rule_is_stated_in_its_own_prompt_alone`), and no default repeats
  what the MCP server already tells every session
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`). The
  MCP session rules state whether a seat works alone or waits on the user,
  and when it may ask anyway
  (`mcp.rs::only_the_orchestrator_is_told_to_ask`).
- Each rule of rule 5 is written in its owner alone
  (`mcp.rs::each_shared_rule_is_written_in_its_owner_alone`), no skill
  repeats how a check runs
  (`defaults.rs::the_skills_leave_how_a_check_runs_to_the_session_rules`),
  and no skill names a timer or a poll
  (`defaults.rs::no_skill_names_a_timer_or_a_poll`).
- Each seat's MCP instructions hold its seat text and its skill index after
  the session rules
  (`mcp.rs::each_seat_is_told_its_seat_text_and_its_skill_index_after_the_rules`).
- The first turn and the first turn after a model switch carry the system
  prompt, and no other turn does: a nudge reaches the agent word for word
  (`seat_brief.rs::an_agent_gets_the_seat_brief_on_the_first_turn_and_after_a_switch`,
  `prompts.rs::a_return_and_a_nudge_assemble_word_for_word`). A
  `claude-acp` agent takes the same path
  (`seat_brief.rs::a_claude_agent_gets_the_seat_brief_first_and_each_later_turn_word_for_word`).
- Every shipped skill holds to the STE limit with each wrapped sentence read
  whole, and a closing quote ends with its sentence
  (`defaults.rs::the_ste_rules_are_read_off_a_text_line_by_line`).
- `coding` uses a failing seam test before each code behavior change. Prose
  changes and behavior-preserving refactors use appropriate verification.
  Its completion handoff identifies the checked revision, each command and
  result. `code-review` reuses only matching evidence and runs missing,
  outdated or insufficient required checks. Evidence does not replace its
  code inspection, acceptance review or repository-rule enforcement
  (`defaults.rs::a_skill_scopes_its_own_checks_to_what_the_task_changed`,
  `::the_review_skill_reuses_matching_evidence_and_ends_on_a_step_call`).
  No shipped skill names a whole suite.
  The merge skill pushes before it fast-forwards, with a squash onto the
  merge base and a guard before the push
  (`::the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself`,
  `::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`).
- The `agent` seat text names what the three task calls are for and bans
  `--no-verify` and `-n`, and names no end of a turn
  (`defaults.rs::the_agent_seat_text_moves_the_task_with_the_two_step_calls`).
  No skill repeats the rule (`defaults.rs::each_rule_is_stated_in_exactly_one_briefing`).
- `coding` adds a new commit on a return and amends nothing, and no skill
  divides a task
  (`defaults.rs::a_task_is_one_commit_and_nothing_amends_a_pushed_one`).
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
- A fenced value sits between its two markers, an empty value fences to
  nothing, and a value holding the closing marker cannot close the fence
  early (`prompts.rs::a_fenced_value_sits_between_its_markers`,
  `::an_empty_value_fences_to_nothing`,
  `::a_value_holding_the_closing_marker_cannot_close_the_fence`). Every
  forge or agent value a briefing or a news prompt carries is fenced, and
  the exhaustive-substitution tests prove it word for word
  (`prompts.rs::every_default_briefing_is_its_template_with_the_values_put_in`,
  `::the_pull_request_texts_fill_every_placeholder_they_name`,
  `::the_keeper_briefing_fills_every_placeholder_it_names`). A task's
  title and description are fenced wherever a briefing, a nudge, a
  relayed message or a dependency line names them, proved through the
  daemon's own rendering, not a hand-built string
  (`agent_messages.rs::a_message_is_handed_to_the_agent_it_was_sent_to`,
  `::two_tasks_on_the_same_skills_are_named_apart_in_their_relay_to_the_orchestrator`,
  `prompts.rs::a_started_column_agent_is_briefed_from_the_builtin_template`,
  `::a_return_and_a_nudge_assemble_word_for_word`,
  `scheduler_attention.rs::an_agent_up_and_never_prompted_is_nudged`,
  `resume.rs::the_scheduler_relaunches_the_current_columns_agent_in_its_session`,
  `acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`,
  `workflow_steps.rs::only_the_current_column_is_nudged_and_idle_columns_raise_no_attention`).
  The orchestrator's own brief for a column's agent is fenced too, and a
  brief crafted with the closing marker and a fresh instruction cannot
  escape it:
  `prompts.rs::a_crafted_agent_brief_cannot_escape_its_fence`.
  The fence
  rule is in the shared session rules alone, within their size cap
  (`mcp.rs::the_shared_rules_stay_small`,
  `::each_shared_rule_is_written_in_its_owner_alone`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text and the STE rules),
`crates/ariadne-daemon/src/agents/prompts.rs` (assembly),
`crates/ariadne-daemon/src/agents/handoff.rs` (the handoff text of a session
switch),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules, and the seat text
and skill index in the instructions),
`crates/ariadne-daemon/src/acp.rs` (`seat_brief_in_turns`, which turn
carries the system prompt),
`crates/ariadne-daemon/src/agents/mod.rs` (`link_skills`).
