---
id: prompts-and-simplified-technical-english
status: current
updated: 2026-09-04
areas: [prompts, store, core, mcp]
commits: [6b566fe6, 45c5e131, 20d998bc, 95083a17, 09b07d4b]
tests:
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-daemon/src/agents/prompts.rs
  - crates/ariadne-daemon/tests/prompts.rs
  - crates/ariadne-daemon/tests/profile_system_prompt.rs
  - crates/ariadne-core/src/lib.rs
---

# Prompts and Simplified Technical English

Every word Ariadne hands an agent: which layer owns it, who may edit it, and
the English all of it is written in.

## Scope

In: the three layers of text (system prompt, lifecycle briefing, landing
briefing), what each layer states, rendering and placeholder validation, the
STE rules, and the size caps.

Out: the wording of any one procedure — those belong to the spec of the thing
they describe (003, 004, 005).

## Behavior

1. Text reaches an agent in four layers, and each rule is stated in exactly
   one of them:
   - the **MCP session rules**, which every session receives before its first
     prompt, whatever its role: that Ariadne is reached only through its
     tools, whether anyone answers a question, how few turns to take, and the
     English to write in (013);
   - the **system prompt**, which states what a role owes from its first read
     to the call that ends its turn;
   - the **lifecycle briefing**, which carries the values of one goal, task or
     round and whatever is only true of this moment;
   - the **landing briefing**, which carries the procedure that ends a task,
     and belongs to the repository (005).
2. A profile owns exactly one text: its **system prompt**. It runs on the
   default of its role until somebody sets one, and a reset drops what was
   set rather than copying a default in.
3. The lifecycle briefings are Ariadne's own constants, the same for every
   profile, read from the code on every launch and every resume. No route
   reads or writes one, and no row holds one.
4. Because nothing is ever copied into the database, rewording a default
   reaches every profile still on it — including profiles created long before.
5. A briefing is rendered by substituting `{name}` tokens. Rendering is
   lenient by construction: an unknown token, an unclosed brace and an empty
   template all render to something, and none of them fails a spawn.
6. Each kind declares the placeholders its builder fills in. A hand-written
   text — today only a repository's landing briefing — is checked against that
   list when it is **saved**, which is the last moment anyone looks at a
   `{task_titel}`.
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
   session rules, where no profile edit can remove it.
10. Every default text is capped in size, per text and in total, and the caps
    come down to what a rewrite fits in. Moving a cap is a decision argued in
    the test's own documentation, never a way round a failing assertion.

## Acceptance criteria

- Every default text obeys the two readable STE rules
  (`defaults.rs::every_default_text_is_simplified_technical_english`).
- Every default text is within its cap and the totals
  (`defaults.rs::size_caps_hold`).
- A rule is stated in exactly one briefing
  (`defaults.rs::each_rule_is_stated_in_exactly_one_briefing`,
  `::a_role_rule_is_stated_in_its_own_prompt_alone`), and no default repeats
  what the MCP server already tells every session
  (`::no_default_repeats_what_every_session_is_told_by_the_mcp_server`).
- Every default names only placeholders its kind can fill in
  (`defaults.rs::every_default_names_only_placeholders_its_kind_can_fill_in`),
  and every allowed placeholder is one a builder actually passes
  (`prompts.rs::every_allowed_placeholder_is_one_a_briefing_fills_in`).
- Broken template syntax still renders
  (`prompts.rs::broken_syntax_passes_through`,
  `::an_unknown_placeholder_travels_verbatim`).
- A created profile starts on the default of its role and stores none of it
  (`profile_system_prompt.rs::a_created_profile_starts_on_the_default_of_its_role`,
  `store.rs::a_new_profile_starts_on_the_role_defaults_and_stores_none_of_them`).
- A system prompt is stored only while set, and a reset deletes the row
  (`store.rs::a_system_prompt_is_stored_only_while_it_is_set_and_a_reset_deletes_it`).
- No route reads or writes a lifecycle prompt
  (`profile_system_prompt.rs::no_route_reads_or_writes_a_lifecycle_prompt`).

## Sources

`crates/ariadne-store/src/defaults.rs` (every default text and the STE rules),
`crates/ariadne-daemon/src/agents/prompts.rs` (assembly),
`crates/ariadne-core/src/lib.rs` (`PromptKind`, placeholder validation),
`crates/ariadne-cli/src/commands/mcp.rs` (session rules).
