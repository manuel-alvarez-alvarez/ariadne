---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-09-08
areas: [prompts, store, core, mcp]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b, a69b953f, 03f9c8b7, a4d7da95]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/prompts.rs
  - crates/ariadne-daemon/tests/skill_documents.rs
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
     tools, whether anyone answers a question, how few turns to take, and the
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
3. The skill index is one line per skill — its name, the summary its
   frontmatter states, and the path of its document in the run directory — and
   the instruction to read a document before doing the work it covers. The
   document itself stays on disk, so a broad set of skills costs an agent a
   few lines rather than a few pages. How each CLI is pointed at those files
   is 007.
4. The lifecycle briefings are Ariadne's own constants, read from the code on
   every launch and every resume. No route reads or writes one, and no row
   holds one. Because nothing is copied into the database, rewording a default
   reaches every session started after it.
5. A briefing is rendered by substituting `{name}` tokens. Rendering is
   lenient by construction: an unknown token, an unclosed brace and an empty
   template all render to something, and none of them fails a spawn.
6. Each kind declares the placeholders its builder fills in, and a default
   naming one outside its list fails the suite. Nothing hand-written reaches
   an agent any more — every text is the code's — so the check runs over the
   defaults rather than at a save.
7. Every agent-facing text is ASD-STE100 Simplified Technical English: one
   instruction to a sentence, the imperative for an instruction, the active
   voice, at most 25 words a sentence, one meaning per word, a list for a
   sequence of steps.
8. Two of those rules are read off the text by test: sentence length, and a
   list of banned words (`utilise`, `prior to`, `in order to`, `ensure`,
   `should`, `may`).
9. STE binds what the agents write too — turn text and visible reasoning, task
   titles and descriptions, review summaries, verdicts, failure reasons, commit
   subjects and bodies, and pull request text — and that rule lives in the
   session rules, where no edit of a skill can remove it.
10. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion. The
    shipped skill documents are capped on their own scale, since a skill is
    read once and on purpose rather than carried by every turn.

## Acceptance criteria

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`).
- Every default text is within its cap and the totals
  (`defaults.rs::size_caps_hold`).
- A rule is stated in exactly one briefing
  (`defaults.rs::each_rule_is_stated_in_exactly_one_briefing`,
  `::a_seat_rule_is_stated_in_its_own_prompt_alone`), and no default repeats
  what the MCP server already tells every session
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`).
- Every default names only placeholders its kind can fill in
  (`defaults.rs::every_default_names_only_placeholders_its_kind_can_fill_in`),
  and every allowed placeholder is one a builder actually passes
  (`prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).
- Broken template syntax still renders
  (`prompts.rs::broken_syntax_passes_through`,
  `::an_unknown_placeholder_travels_verbatim`).
- A seat's prompt carries what the seat owes and nothing of a skill
  (`skill_documents.rs::a_seat_prompt_carries_only_what_the_seat_owes`).
- The orchestrator system prompt holds no playbook step; the phases and their
  order are the `orchestration` skill's
  (`defaults.rs::the_orchestrator_playbook_asks_before_it_plans_and_plans_before_it_starts`,
  `::the_orchestrator_staffs_a_plan_on_a_mix_of_agent_clis`), and an
  orchestrator session indexes that skill
  (`skill_documents.rs::an_orchestrator_session_indexes_the_orchestration_skill`).
- The index adds one line per skill, and the path it names holds the document
  (`prompts.rs::a_spawned_author_is_briefed_from_the_builtin_template`).
- Every shipped skill document is within its cap
  (`defaults.rs::skill_size_caps_hold`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text and the STE rules),
`crates/ariadne-daemon/src/agents/prompts.rs` (assembly),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules),
`crates/ariadne-daemon/src/agents/mod.rs` (`write_skills`).
