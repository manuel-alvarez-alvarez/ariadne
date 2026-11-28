---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-11-28
areas: [prompts, store, core, mcp, daemon]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b, a69b953f, 03f9c8b7, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-store/src/skill_tests/template.rs
  - crates/ariadne-store/src/skill_tests/produce.rs
  - crates/ariadne-store/src/skill_tests/review.rs
  - crates/ariadne-store/src/skill_tests/operate.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/src/agents/handoff.rs
  - crates/ariadne-daemon/src/forge/news.rs
  - crates/ariadne-daemon/src/scheduler/goals.rs
  - crates/ariadne-daemon/src/scheduler/tasks.rs
  - crates/ariadne-daemon/src/launcher.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-daemon/tests/it/posts.rs
  - crates/ariadne-daemon/tests/it/scheduler_attention.rs
  - crates/ariadne-daemon/tests/it/resume.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/workflow_steps.rs
  - crates/ariadne-daemon/tests/it/standalone_tasks.rs
  - crates/ariadne-daemon/tests/it/skill_documents.rs
  - crates/ariadne-daemon/tests/it/seat_brief.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-core/src/lib.rs
---

# Prompts and Simplified Technical English

Every word Ariadne hands an agent: which layer owns it, who may edit it, and
the English all of it is written in.

## Scope

In: the layers of text (MCP instructions, system prompt, skill supplement,
skill index, lifecycle briefing), the word list a shipped skill uses in
place of an Ariadne word, what each layer states, which turn carries the system
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
     when a seat asks the user and how, what turn text is for — a result,
     a question or a blocker — how few turns to take, how every check runs,
     the English to write in, and that no text carries a trailer. After the
     rules come the seat text, picked by the seat, then the supplement of
     each skill the session loads (rule 21), then the index of those skills
     (rule 4);
   - the **system prompt**, which is that seat text, those supplements and
     that skill index again: what a seat owes from its first read to the
     call that ends its work, what Ariadne adds to each skill in this seat,
     then the skills this agent was staffed with (017) — for the
     orchestrator, the one skill its seat fixes by name. It reaches the
     agent through the MCP instructions and in a turn (rule 2);
   - the **lifecycle briefing**, which carries the values of one goal, task
     or column and whatever is only true of this moment: the column's
     instructions and the summary of the column before it (030), who sent a
     post, the task it names and where to answer it (039).
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
   text is the pull request review session's (029 rule 5).
   It states the explicit ask, all authors, no required GitHub assignment, and drafts (029 rule 1).
   It also states its detached worktree and that it commits and pushes nothing. It names
   neither its skill, which the skill index does for every seat, nor the end
   of its turn, which its briefing and the news prompt each end on. The
   `keeper` seat text is the session's that keeps a request of the user's
   own asked for by hand (032).
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
   - asking the user: the session rules, picked by seat, with no tool
     description repeats it;
   - working in the task's worktree and on the current column: the `agent`
     seat text alone; no column's skill repeats it;
   - planning the goal with the user, writing no code, and deciding no
     open point alone: the `orchestrator` seat text alone; the
     `orchestration` skill states its phases, never these three;
   - ending the turn after a call: the `complete_step`, `fail_step` and
     `post_to_session` descriptions;
   - how a check runs — in the foreground, split by crate, logged outside
     the worktree, its failures alone printed, never polled: the session
     rules. A skill says only which checks it runs: the checks of what it
     changed. The repository's hooks run its whole suite at a push, and the
     merge skill pushes before it fast-forwards (030);
   - no trailer in any text: the session rules;
   - never polling a request: the session rules of the `agent` and
     `reviewer` seats alone, which say that Ariadne wakes the session with
     the request's news (026, 030) instead. No skill names a timer or a
     poll;
   - the column report a column ends on, and the return report a column is
     sent back with: the `complete_step` and `fail_step` descriptions
     alone (013 rule 4). No skill restates a field of either; `coding`
     and `pr-babysit` each name their own report only as "the column
     report", and `merge` uses the plain word of rule 21;
   - stopping a fix loop: the agent seat text alone, which stops an agent
     after three failed attempts at one fix;
   - what a plain word of a shipped skill stands for in Ariadne — the
     requester is the orchestrator, reached with `post_to_session`, a stage
     is a column, the report is the column report, to return the work is to
     fail the step, an update is news, your instructions are the briefing,
     and the primary checkout is the `Repo:` path: the agent seat text
     alone (rule 21);
   - which call reads the whole change and which calls return and pass the
     work, for `code-review` in the `agent` seat — `get_diff`, `fail_step`
     with the return report, `complete_step` with the verdict: that seat's
     `code-review` supplement alone. The skill once said its two step calls
     were the task seat's; a supplement is picked by seat, so that rule
     left with them;
   - that a review session posts each `code-review` finding as
     `pr-reviewer` says, and that `pr-reviewer` ends the round: the
     `reviewer` seat's `code-review` supplement alone, which names no step
     call;
   - the `merge_commit` value of the call that passes the work, and the
     `fail_step` call for a failed check, for `merge`: the `agent` seat's
     `merge` supplement alone;
   - the call that ends the stage where no skill invoked
     `conflict-resolution`: the `agent` seat's `conflict-resolution`
     supplement alone.
6. `coding` uses test-first development for a code behavior change: it runs
   the new seam test failing before implementation. Prose changes and
   behavior-preserving refactors use verification that fits their risk.
   `coding` calls `complete_step` with the column report that tool's
   description defines (013), covering every criterion. `code-review` names
   the report and the return report by name, and restates no field
   of either. It reads the report as unverified claims before it
   pins the scope to the diff, then runs the tests, the lint and the
   format check of the files the diff changes itself, once for the
   verdict. It is an independent gate, not an audit of the report: a
   reported result stands only for a check outside those files, and only
   where the report names its command, the reviewed revision and the
   result; the skill never reruns a check whose result it already holds.
   Evidence never replaces code inspection, acceptance review or
   repository-rule enforcement. What a blocking or must-fix finding must
   name, and the cap on the rest, is 017 rule 11's to state, for all three
   review skills alike. Each other produce skill that ends its own column —
   `conflict-resolution`, `documentation`, `research`, `spec-writing` —
   calls `complete_step` with
   the column report the same way, naming it and restating none of its
   fields.
7. A skill carries only the commit of the step it owns: `coding` commits the
   task it built, and a return from review adds a new commit on top and
   amends nothing, since the merge column squashes. The refactoring path of `coding` commits
   the moves it made, and `pr-babysit` never amends a pushed request.
   `spec-writing` commits the spec it wrote once, after the orchestrator
   agrees to it, under the repository's commit rules and with no commit
   form of its own. `documentation` checks the page and commits it once
   under the repository's commit rules. No skill divides a task into slices
   or small commits.
8. The lifecycle briefings are Ariadne's own constants, read from the code on
   every launch and every resume. No route reads or writes one, and no row
   holds one. Because nothing is copied into the database, rewording a default
   reaches every session started after it. The notice an orchestrator gets
   when the scheduler moved an exhausted agent to another model (021) is one
   of those constants too, and so is the note the first daemon prompt after
   a cancelled turn starts with (021 rule 8).
9. A briefing is rendered by substituting `{name}` tokens. Rendering is
   lenient by construction: an unknown token, an unclosed brace and an empty
   template all render to something, and none of them fails a spawn.
10. Each kind declares the placeholders its builder fills in, and a default
   naming one outside its list fails the suite. Nothing hand-written reaches
   an agent any more — every text is the code's — so the check runs over the
   defaults rather than at a save. The kinds are the orchestrator's briefing,
   resume and attention, `IncomingMessage`, `StepBriefing`, `StepReturn` and
   `AgentResume` (`ariadne_core::PromptKind`). `IncomingMessage` tags the
   prompt of a post (039). A post writes its own text, so no builder
   renders that kind's template. `AgentResume`, the quiet nudge
   of a column's agent (009), says four things only: this is not a new
   request, to check whether the work is done, to make the call that ends
   the column or go on, and to write no reply about the nudge itself.
   `OrchestratorResume` does not say to ask an open question again.
11. Default texts, shipped skills, workflow documents, session rules and tool
    descriptions use ASD-STE100 Simplified Technical English: one instruction
    to a sentence, the imperative for an instruction, the active voice, at
    most 20 words a sentence (`ste::MAX_WORDS`), one meaning per word, a list
    for a sequence of steps. Tests read sentence length line by line — a
    skill with each wrapped sentence joined first — and a
    list of banned words (`utilise`, `prior to`, `in order to`, `ensure`,
    `should`, `may`).

    The same two readable rules — the word count and the banned list — hold
    for a line the daemon builds rather than ships as a default, wherever
    that line reaches an agent: the forge's news, a goal's attention line,
    the handoff's intro and its omission notes, the issue goal's `Closes` line, and a tool's
    refusal or the result string it writes in place of the daemon's own
    JSON. Each is proven with the `ste` module beside the code that builds
    it, rendered with sample values where the line takes any.
12. STE binds what the agents write too — turn text, task titles and
    descriptions, step and task reasons, commits, and pull request text —
    and that rule lives in the session rules, where no edit of a skill can
    remove it. An agent's visible reasoning is not bound: nobody reads it as
    a product, and a constraint on how a model reasons costs it reasoning
    tokens on every turn for no reader's gain.
13. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion. The
    shipped skill documents are capped on their own scale, since a skill is
    read once and on purpose rather than carried by every turn: one cap of
    8,000 characters holds every skill alike, 70,000 holds the catalog, and a
    body stays under 500 lines (017 rule 16). One number for every skill,
    rather than a budget per skill, lets a skill grow for a reason of its own
    without a change of the test first; the total still keeps one skill from
    taking the whole room. A supplement rides in the system prompt beside
    the seat text, so it is capped like one: each pair of skill and seat has
    a cap of its own, and one total holds them all (rule 21). A pair with no
    cap fails the test, so a new supplement argues its cap first.
14. The handoff is a text built from a session's stored events for a
    session switched to another coding agent (021): no ACP agent can resume
    another's conversation, so the new agent gets this instead, as its
    history. `handoff_text` (`crates/ariadne-daemon/src/agents/handoff.rs`)
    folds the events into the typed turns of the trajectory fold (040), and
    opens the text with one line that names it as the history of the session the agent
    continues and tells it to go on from where that history ends.
15. One entry follows per kept event, oldest first, each under its own
    marker: a `user_prompt_submit` whole under `user` where its source is
    `console`, and folded to its first six lines with a count of the rest
    under `daemon` where its source is `daemon`; an `agent_message` whole
    under `agent`; a `plan` as a checklist, headed by how many of its entries
    are complete; a paired `pre_tool_use` and `post_tool_use` as the head
    line of the call — the command, the path, the
    pattern, or the title — then its output and its diff, read from the
    rows the fold names for the call. The newest row with output and the
    newest row with a diff supply those values. Each folds to its last ten
    lines with a count of what is hidden. A call with no end
    renders its head alone; a `permission_request` and its
    `permission.replied` as one line, the head of the call and the option
    chosen; and a `session.error` as one line, its first line.
    `agent_thought`, `stop`, `session_start` and `session_end` are left out.
    What the history leaves out beyond these is stated once, in 021 rule 4.
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
    state a news line reports, a post's body, and a step's
    return reason or completion summary. One line is the exception: an
    issue goal's `Closes <url>` instruction carries the raw URL, never
    fenced, since GitHub only closes the issue on that exact text; the
    same URL is still fenced everywhere else the briefing names the
    issue.
19. A goal's title and description reach every briefing plain, never
    fenced, since they are the user's own words — unless the goal came
    from an issue, where rule 18 already fences them as the forge's own
    text. A task's title and description are plain the same way,
    wherever a briefing, a post or a nudge names
    the task: whoever opens a task is Ariadne's own user or orchestrator,
    never a third party. The orchestrator's own brief for a column's
    agent (`TaskAgent.brief`) is plain too — the orchestrator plans
    alongside the user, not against it, so its brief is read the same as
    the user's own words, not fenced as another agent's would be.
20. The agent of a task with no workflow (001 rule 26) has no column, so its
    briefing and its nudge name none. The briefing gives the task, its
    worktree and branch, and says that `complete_step` finishes the task and
    `fail_step` fails it. Both texts are constants of the code, in
    Simplified Technical English. The agent loads the skills of its staffing,
    and none when none are given.

21. A **supplement** is what Ariadne adds for one shipped skill in one seat:
    what the plain words of that skill stand for there, in tools and calls.
    A shipped skill reads the same in any harness, and its supplement makes
    it Ariadne's.
    - Each supplement is a file of the code,
      `crates/ariadne-store/supplements/<skill>/<seat>.md`, compiled in
      through `SKILL_SUPPLEMENTS` in `crates/ariadne-store/src/defaults.rs`.
      No row, route, command, screen or host sync holds or changes one, and
      no edit or reset of the skill reaches it.
    - The daemon picks a supplement by the skill's name and the seat whose
      text the session runs under (`skill_supplement`). A pair with none
      adds no text. A shipped skill the user edited gets the supplement of
      its name. A skill of the user's own gets none, whatever its name.
    - The system prompt holds the seat text, then the supplement of each
      staffed skill in the order of its staffing, then the skill index. The
      launcher hands the same supplements to the MCP server as
      `ARIADNE_SKILL_SUPPLEMENTS`, which puts them between the seat text
      and the index (013 rule 2).
    - A shipped skill that names no Ariadne word (017 rule 16) uses the plain
      word of this word list in its place:

      | Ariadne word | Plain word in a skill |
      | --- | --- |
      | orchestrator | the requester |
      | column | stage |
      | column report | the report |
      | fail the step | return the work, with a return report |
      | news | an update |
      | briefing | your instructions |
      | `Repo:` path | the primary checkout of the repository |
      | seat, keeper | not used in a skill |
      | task, ticket, goal | unchanged |

      The agent seat text binds each plain word to its Ariadne word in one
      line, and names the orchestrator as the requester and
      `post_to_session` as the way to reach it (rule 5). No skill and no
      supplement repeats the binding.
    - Each supplement has a cap of its own and obeys the STE rules of rule
      11, the same as a default text (rule 13).
    - Four ship. `code-review` in the `agent` seat holds the `get_diff` call
      for the whole change, and the `fail_step` and `complete_step` calls
      that return and pass the work. `code-review` in the `reviewer` seat
      hands the round to `pr-reviewer` and names no step call.
      `merge` in the `agent` seat holds the `complete_step` call with its
      `merge_commit` and the `fail_step` call for a failed check.
      `conflict-resolution` in the `agent` seat holds the `complete_step`
      call for where no skill invoked it. `architecture-review` has none:
      the seat text binds its plain words.

## Acceptance criteria

- Rule 21: the system prompt is the seat text, then the supplements, then
  the index, and a review session gets the reviewer seat's supplement
  (`prompts.rs::the_system_prompt_holds_the_seat_text_then_the_supplements_then_the_index`);
  a pair with no supplement adds no text
  (`prompts.rs::a_pair_with_no_supplement_adds_no_text`,
  `defaults.rs::a_supplement_is_picked_by_its_skill_and_its_seat`); an
  edited shipped skill gets the supplement of its name and a skill of the
  user's own gets none
  (`prompts.rs::an_edited_shipped_skill_gets_its_supplement_and_a_user_skill_gets_none`).
  A review column's agent is launched with the `code-review` supplement in
  its system prompt and its MCP environment, and a develop column's with
  none (`it/prompts.rs::a_review_columns_agent_is_launched_with_the_code_review_supplement`).
  The MCP instructions hold the supplements between the seat text and the
  index (`mcp.rs::each_seat_is_told_its_seat_text_and_its_skill_index_after_the_rules`).
  The agent seat text binds each plain word of the word list
  (`skill_tests/template.rs::the_agent_seat_text_binds_each_plain_word_to_ariadne`).
  `code-review` and `architecture-review` name no Ariadne, no MCP tool and
  no Ariadne word, by the one helper `assert_names_no_ariadne_word`
  (`skill_tests/template.rs::the_two_review_skills_name_no_ariadne_word`).
  The agent seat's `code-review` supplement holds its `get_diff`,
  `fail_step` and `complete_step` rules, and the reviewer seat's names no
  step call
  (`skill_tests/review.rs::the_review_skill_reviews_in_place_and_ends_on_a_step_call`).
  `merge` and `conflict-resolution` name no Ariadne word either
  (`skill_tests/template.rs::merge_and_conflict_resolution_name_no_ariadne_word`),
  and their supplements hold the calls the skills dropped
  (`skill_tests/operate.rs::the_merge_supplement_holds_the_step_calls_the_skill_dropped`,
  `skill_tests/operate.rs::the_conflict_resolution_supplement_holds_the_call_that_ends_the_stage`).
  Each supplement is within its cap and the total
  (`defaults.rs::supplement_size_caps_hold`), is STE
  (`defaults.rs::every_default_text_is_simplified_technical_english`), and
  each rule of rule 5 it owns is stated there alone
  (`skill_tests/template.rs::each_rule_is_stated_in_exactly_one_briefing`).

- Rule 20: `prompts.rs::a_task_with_no_workflow_is_briefed_without_a_column`
  and `standalone_tasks::a_task_with_no_workflow_is_finished_by_its_agents_complete_step`
  (the briefing as sent, and no skill index when none is given), and
  `standalone_tasks::the_agent_of_a_task_with_no_workflow_loads_its_supplied_skill`
  (a supplied skill is in the index and in the run directory).

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`), and
  the rules are read line by line
  (`::the_ste_rules_are_read_off_a_text_line_by_line`). So does every text
  the MCP server hands an agent
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- So does every line the daemon builds rather than ships as a default: the
  forge's news
  (`news.rs::every_news_line_is_simplified_technical_english`), a goal's
  attention line
  (`goals.rs::goal_attention_lines_are_simplified_technical_english`), the
  handoff's intro and its omission notes
  (`handoff.rs::the_intro_and_the_omission_notes_are_simplified_technical_english`),
  the issue goal's `Closes` line
  (`prompts.rs::the_closes_line_is_simplified_technical_english`), and a
  tool's refusal or the result string it writes in place of the daemon's
  own JSON
  (`tools.rs::every_refusal_and_result_string_is_simplified_technical_english`,
  `mcp.rs::the_seat_refusal_is_simplified_technical_english`).
- A column's first briefing has a `Column:` line, no `Goal:` line and no
  `Dependencies:` line, and `Previous:` reads `none` where no column came
  before
  (`prompts.rs::missing_values_keep_their_fallbacks`,
  `::every_default_briefing_is_its_template_with_the_values_put_in`).
- A return reads `Resume "<task>" at <column>. <sentence>` and then
  `Reason: <reason>`, where the sentence is one of three
  (`prompts.rs::a_return_says_its_direction_in_a_sentence_and_its_reason_on_a_line`,
  `it/prompts.rs::a_return_and_a_nudge_assemble_word_for_word`,
  `workflow_steps.rs`, the three directions through a real task). A fresh
  session that goes back to a column it saw before is briefed in full: the
  report of the column before it, from a move earlier than the return, as
  `Previous:` (`none` where there is none), then the return, with the
  return reason said once
  (`it/prompts.rs::a_fresh_session_back_in_a_column_gets_the_full_briefing_and_the_return_once`,
  `::a_fresh_session_back_in_a_later_column_has_the_previous_report`;
  `tasks.rs::the_previous_report_is_the_last_forward_move_into_the_column`
  for the choice of report). The orchestrator's `brief` follows only a
  prompt that holds the full step briefing: it ends the first prompt and the
  one for a fresh session, and a return to a session that already read it
  does not repeat it
  (`it/prompts.rs::a_return_to_a_session_that_read_the_briefing_does_not_repeat_the_brief`,
  and the two tests above for the fresh session).
- The orchestrator's briefing shows each repository's id where the goal has
  more than one repository
  (`prompts.rs::the_orchestrator_sees_repository_ids_only_when_there_are_several`).
  The two request briefings have no `Repo:` line
  (`prompts.rs::the_pull_request_texts_fill_every_placeholder_they_name`,
  `::the_keeper_briefing_fills_every_placeholder_it_names`).
- Every default text is within its cap and the totals
  (`defaults.rs::size_caps_hold`).
- The seat texts and the briefings the agreed text gives ship word for word
  (`defaults.rs::the_agreed_texts_ship_word_for_word`).
- A rule is stated in exactly one briefing
  (`skill_tests/template.rs::each_rule_is_stated_in_exactly_one_briefing`,
  `::a_seat_rule_is_stated_in_its_own_prompt_alone`), and no default repeats
  what the MCP server already tells every session
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`). The
  MCP session rules state whether a seat works alone or waits on the user,
  and when it may ask anyway
  (`mcp.rs::only_the_orchestrator_is_told_to_ask`).
- Each rule of rule 5 is written in its owner alone
  (`mcp.rs::each_shared_rule_is_written_in_its_owner_alone`), no skill
  repeats how a check runs
  (`skill_tests/template.rs::the_skills_leave_how_a_check_runs_to_the_session_rules`),
  and no skill names a timer or a poll
  (`skill_tests/template.rs::no_skill_names_a_timer_or_a_poll`).
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
- `coding` calls `complete_step` with the column report that tool's
  description defines, covering every criterion, and names no field of it
  itself. `code-review` names the report and the return report by
  name, reads the former as unverified claims, then runs the tests, the
  lint and the format check of the changed files itself and reuses a
  reported result only for a check outside them.
  Evidence does not replace its code inspection, acceptance review or
  repository-rule enforcement
  (`skill_tests/produce.rs::a_skill_scopes_its_own_checks_to_what_the_task_changed`,
  `skill_tests/review.rs::the_review_skill_reviews_in_place_and_ends_on_a_step_call`,
  `tools.rs::complete_step_and_fail_step_define_their_reports`).
  No shipped skill names a whole suite.
  Every other produce skill that ends its own column names the column
  report the same way, restating none of its fields
  (`skill_tests/produce.rs::a_produce_skill_names_the_column_report_where_it_ends_a_column`).
  The merge skill pushes before it fast-forwards, with a squash onto the
  merge base and a guard before the push
  (`skill_tests/operate.rs::the_merge_skill_lands_in_order_and_no_skill_merges_a_request_itself`,
  `skill_tests/operate.rs::a_late_squash_keeps_what_another_landing_put_on_the_base_branch`).
- The `agent` seat text names what the three task calls are for, bans
  `--no-verify` and `-n`, states the stop rule once, and names no end of a
  turn (`defaults.rs::the_agent_seat_text_moves_the_task_with_the_two_step_calls`).
  No skill repeats any of it
  (`skill_tests/template.rs::each_rule_is_stated_in_exactly_one_briefing`).
- The quiet nudge says this is not a new request, to check whether the work
  is done, to make the call that ends the column or go on, and to write no
  reply about the nudge; the stop rule has the agent seat text as its one
  owner
  (`defaults.rs::the_quiet_nudge_says_four_things_and_the_stop_rule_has_one_owner`).
  The orchestrator nudge does not say to ask an open question again. It
  fits the conversation and the goal under way
  (`defaults.rs::the_orchestrator_nudge_fits_the_conversation_and_the_goal_under_way`).
- `coding` adds a new commit on a return and amends nothing, and no skill
  divides a task
  (`skill_tests/produce.rs::a_task_is_one_commit_and_nothing_amends_a_pushed_one`).
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
  `skill_tests/operate.rs::the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts`),
  and an orchestrator session indexes that skill
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`).
- The index adds one line per skill, and the path it names holds the document
  (`prompts.rs::a_started_column_agent_is_briefed_from_the_builtin_template`).
- Every shipped skill document is within the one skill cap, and the catalog
  within its total (`skill_tests/template.rs::skill_size_caps_hold`).
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
  (`::a_diff_is_folded_the_same_way_as_output`), and mixed updates keep the
  newest output and the newest diff
  (`::a_call_keeps_its_newest_output_across_mixed_updates`). A call with no end
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
  forge value a reviewer or a keeper briefing or a news prompt carries is
  fenced, and the exhaustive-substitution tests prove it word for word
  (`prompts.rs::every_default_briefing_is_its_template_with_the_values_put_in`,
  `::the_pull_request_texts_fill_every_placeholder_they_name`,
  `::the_keeper_briefing_fills_every_placeholder_it_names`).
- A goal the user typed reaches the orchestrator's briefing with no fence
  around its title or its description, and a goal from an issue keeps
  them fenced the same as its URL
  (`prompts.rs::a_goal_the_user_typed_is_not_fenced`,
  `::a_goal_from_an_issue_is_fenced`,
  `::a_goal_from_an_issue_names_the_line_that_closes_it`; end to end,
  `prompts.rs::an_issue_goal_keeps_its_url_and_briefs_the_orchestrator_to_close_it`).
  The same holds everywhere else a goal's title reaches a briefing: the
  resume nudge and the attention text fence
  an issue goal's title and leave a user-typed one plain, the same as
  the orchestrator's own first briefing does
  (`prompts.rs::an_issue_goals_title_stays_fenced_everywhere_it_is_named`,
  `::a_user_typed_goals_title_stays_plain_everywhere_it_is_named`).
  A task's title and description reach a briefing, a nudge or a post
  plain, never fenced, proved through the
  daemon's own rendering, not a hand-built string
  (`posts.rs::a_post_reaches_the_live_session_as_a_prompt_and_writes_no_row`,
  `prompts.rs::a_started_column_agent_is_briefed_from_the_builtin_template`,
  `::a_return_and_a_nudge_assemble_word_for_word`,
  `scheduler_attention.rs::an_agent_up_and_never_prompted_is_nudged`,
  `resume.rs::the_scheduler_relaunches_the_current_columns_agent_in_its_session`,
  `acp_runtime.rs::a_scheduler_nudge_arrives_at_the_stub_agent_as_a_prompt`,
  `workflow_steps.rs::only_the_current_column_is_nudged_and_idle_columns_raise_no_attention`).
  The orchestrator's own brief for a column's agent reaches it plain too,
  the same as the user's own words:
  `prompts.rs::an_agents_brief_reaches_it_in_plain_text`.
  The fence
  rule is in the shared session rules alone, within their size cap
  (`mcp.rs::the_shared_rules_stay_small`,
  `::each_shared_rule_is_written_in_its_owner_alone`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text, the STE rules
and `SKILL_SUPPLEMENTS`), `crates/ariadne-store/supplements/` (the
supplement texts), `crates/ariadne-daemon/src/agents/prompts.rs` (assembly,
`skill_supplements`),
`crates/ariadne-daemon/src/agents/handoff.rs` (the handoff text of a session
switch),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules, and the seat text,
the supplements and the skill index in the instructions),
`crates/ariadne-daemon/src/acp.rs` (`seat_brief_in_turns`, which turn
carries the system prompt),
`crates/ariadne-daemon/src/agents/mod.rs` (`link_skills`).
