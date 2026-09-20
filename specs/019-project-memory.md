---
id: project-memory
status: current
updated: 2026-09-20
areas: [store, api, daemon, mcp, cli, ui, prompts]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/memories.rs
  - crates/ariadne-store/src/defaults.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/memory.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/features/memory/memory-page.test.tsx
  - ui/src/features/repositories/repositories-page.test.tsx
  - ui/src/events/dispatch.test.ts
---

# Memory

Facts one session learns about a repository, or about the work itself, can
help later sessions without becoming part of every prompt.

## Scope

In: repository and global memory storage, the user write path, an optional
expiry, source records, REST access, MCP save and search tools, CLI list,
search and delete commands, and the desktop screen that offers the same three
(015).

Out: prompt injection. Agents choose when to search.

## Behavior

1. Each memory belongs to one registered repository, or to none, which makes
   it global. It holds text, its creation time, an expiry time where one was
   given, and the session, task and goal that sourced it. A memory saved
   without an expiry time never expires.
2. An agent session saves a memory for a repository of its task or goal, and
   the daemon records its source from the session instead of accepting source
   fields from the caller. A session that saves a global memory is refused. A
   user call carries no session, and saves in any scope without a source.
3. A memory remains after its source session, task or goal is deleted. A
   repository memory is removed when its repository is deleted, and a global
   memory stays.
4. List and search return only memories whose expiry time is still ahead.
5. Search splits its query into words and matches each word without case
   sensitivity, including a prefix of a word. Where one or more memories
   match, search returns only those memories, ordered by FTS5 score. Where no
   word matches, search returns the five newest active memories of the scope
   and sets `fallback` in the `{hits, fallback}` answer to `true`.
   A create refuses an active memory of the same scope that matches a word of
   its text, naming the memory that already holds the fact. A task saves two
   memories at most; a session with no task saves two memories per goal.
6. An agent reads and changes the memories of a repository in its task or
   goal, and reads the global memories. A user reads, writes and deletes in
   every scope.
7. Every seat has `save_memory` and `search_memory`. A task session defaults
   to its task repository, and an orchestrator names a repository when its
   goal does not have exactly one. `search_memory` reads the named
   repository's memories and the global ones together. `save_memory` takes
   an optional expiry, and an omitted one never expires; its own text states
   the bound every write keeps to (behavior 8): two memories a task at most,
   a near-duplicate of an existing memory refused, and worth saving only a
   trap, a working command or a convention no file states — never a report
   of the task itself.
8. No memory is added to a prompt. The MCP session rules tell every seat to
   call `search_memory` before it repeats a discovery, and that read rule is
   stated there alone (006). Four skills carry a `save_memory` step at the
   step that earns it: `coding` the trap it hit, the seam it had to learn or
   the command that proved the change, `debugging` the cause once proved,
   `code-review` a convention breach that repeats
   across tasks, and `research` the finding that answers the question again
   later. `orchestration` searches memory while it explores a goal. Every
   write step carries the same bar: save a trap, a working command or a
   convention no file states, and only a fact that cost time; never save a
   task report, a change summary, a plan, or what the code, a spec or
   `AGENTS.md` already states; a task saves two memories at most, and the
   daemon refuses the third.
9. `ariadne memory add <text>` names its one scope with `--repo <id|path>` or
   `--global`, one of the two required, and takes an optional `--expires`.
   `ariadne memory ls` and `ariadne memory search` take the same two flags,
   or neither for every memory of every scope, and never both. `ariadne
   memory delete <id>` finds the entry by its id alone. Every list shows a
   `scope` column, `global` or the repository, and all four use the shared
   list, mutation, JSON and quiet output forms (014).
10. The REST surface adds, lists, searches and deletes memory under
    `/v1/memories`. A list and a search take a repository and a scope, which
    is `repository`, `global` or `all`, and `all` is the default. Every
    endpoint appears in OpenAPI (012).
11. Memory creation emits the complete entry, and memory deletion emits the
    removed id and its repository, which is null for a global memory, on the
    domain event stream (012).
12. The desktop app has one memory page, `#/memory`, that lists, searches,
    adds and deletes through the same REST endpoints the CLI uses, matching
    it (015). It shows the global memories and every repository's, and names
    the scope of each row. A scope filter narrows the list, and the filter
    lives in the URL as `?repository=<id>` (`global` for the global set), so a
    link or a reload keeps it. No repository row and no route of its own
    opens memory. Its add form takes the text, the scope (global or one
    repository) and an optional expiry. A refused save shows the daemon's
    message and keeps the typed text. Where no word of a search matched, a line above the
    list says so, and the newest memories stand in. A creation or deletion event refreshes every open
    memory list, so a global memory reaches each list that holds it.

## Acceptance criteria

- Another goal session of the same repository finds an author's saved memory
  after the source goal is deleted
  (`memories.rs::another_session_of_the_same_repository_finds_an_authors_memory`).
- A user call with no session saves a global memory, and a session of any
  repository reads it
  (`memories.rs::a_user_saves_a_global_memory_that_any_repository_reads`).
- An agent session that saves a global memory is refused
  (`memories.rs::an_agent_session_cannot_save_a_global_memory`).
- An agent session never reads a memory of a repository outside its goal
  (`memories.rs::an_agent_session_never_reads_another_repositorys_memory`).
- A memory saved without an expiry returns from list and search
  (`memories.rs::a_memory_without_an_expiry_stays_in_the_list_and_the_search`),
  and an expired memory never does
  (`memories.rs::an_expired_memory_never_returns_from_list_or_search`).
- A read of the scope `repository` hides the global memories
  (`memories.rs::the_repository_scope_hides_the_global_memories`).
- Deleting a repository removes its memories and keeps the global ones
  (`memories.rs::deleting_a_repository_removes_its_memories_and_keeps_the_global_ones`).
- Delete removes a memory (`memories.rs::delete_removes_a_memory`), a session
  deletes no global memory (`memories.rs::a_session_deletes_no_global_memory`),
  and the CLI accepts the entry alone, with no repository
  (`cli/tests.rs::memory_delete_takes_the_entry_alone`).
- The creation and the deletion events carry the scope
  (`memories.rs::the_creation_and_the_deletion_events_carry_the_scope`).
- Every endpoint appears in OpenAPI
  (`memories.rs::every_memory_endpoint_is_in_the_openapi_document`).
- Every seat receives both memory tools
  (`mcp.rs::every_seat_has_the_tools_its_playbook_names_and_no_others`), and
  the tools call the named repository
  (`tools.rs::memory_tools_save_and_search_the_named_repository`).
- Memory tools default to a task's repository
  (`tools.rs::memory_tools_default_to_the_task_repository`), default to a
  goal's only repository
  (`::memory_search_defaults_to_the_goals_only_repository`), and require a
  repository when the goal has several
  (`::memory_search_needs_a_repository_when_the_goal_has_several`).
- Every MCP text is Simplified Technical English
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- Every session is told to search memory before it repeats a discovery
  (`mcp.rs::every_session_is_told_how_ariadne_is_reached`), and no default
  text or skill repeats that rule
  (`defaults.rs::no_default_repeats_what_every_session_is_told_by_the_mcp_server`).
- Each of the five skills that learn names `save_memory` at its own step with
  the bar on what is worth keeping, and `orchestration` names `search_memory`
  (`defaults.rs::every_skill_that_learns_names_the_memory_tools`).
- The desktop memory page lists, searches through the daemon's own search
  endpoint, and deletes an entry
  (`ui/src/features/memory/memory-page.test.tsx::searches through the daemon's own endpoint rather than filtering locally`,
  `::deletes an entry of either scope`).
- Two matching facts rank by their FTS5 score
  (`memories.rs::two_word_matches_rank_by_fts5_score`).
- A nonmatching fact stays out while another fact matches
  (`memories.rs::a_nonmatching_memory_stays_out_of_a_matching_search`).
- A search with no matching word returns the five newest facts with fallback
  (`memories.rs::a_search_without_a_word_match_answers_the_five_newest_memories`).
- A repeated fact is refused with the existing memory named
  (`memories.rs::a_second_create_that_repeats_a_memory_is_refused`).
- A task saves two facts, and another task of the same goal can save its own
  fact (`memories.rs::a_task_saves_two_memories_but_another_task_can_save_its_own_two`).
- A taskless session saves two facts per goal
  (`memories.rs::a_taskless_session_saves_two_memories_per_goal`).
- The FTS index excludes nonmatches and follows text updates and deletes
  (`store.rs::memory_word_search_excludes_nonmatches_and_tracks_text_changes`).
- The desktop page lists memories of both scopes and names the
  scope of each row
  (`memory-page.test.tsx::lists memories of both scopes and names the scope of each row`),
  and its scope filter shows the global set alone
  (`::shows the global set alone under the global scope filter`).
- The desktop add form creates a global memory that the list then shows
  (`memory-page.test.tsx::adds a global memory, and the list shows it`),
  creates a memory for one repository
  (`::adds a memory for one repository`).
- The desktop page opens narrowed to the repository of `?repository=<id>`, and
  writes the scope filter back to that param, which a change to all scopes
  clears
  (`memory-page.test.tsx::opens with the repository of the URL selected, and lists its memories alone`,
  `::writes the picked scope back to the URL, and clears it for all scopes`,
  `::keeps other params of the URL when the scope changes`).
- A repository row on the repositories screen has no Memory button
  (`repositories-page.test.tsx::offers no Memory button on a row, which is managed from its own screen`).
- A refused desktop save shows the daemon's message and keeps the typed text
  (`memory-page.test.tsx::shows the daemon's refusal and keeps the typed text`).
- The desktop page says so where no search word matched
  (`memory-page.test.tsx::says no word matched, where the newest memories stand in`).
- The desktop page deletes an entry of either scope
  (`memory-page.test.tsx::deletes an entry of either scope`).
- A creation or deletion event of a global memory refreshes every list that
  holds it
  (`dispatch.test.ts::refetches every list that holds a saved global memory`,
  `::refetches every list that held a deleted global memory`).
- `ariadne memory add` names exactly one scope and refuses a call that names
  none or both (`cli/tests.rs::memory_add_names_exactly_one_scope`).
- `ariadne memory ls` and `search` take a repository, `--global` or neither,
  and refuse both
  (`cli/tests.rs::memory_ls_and_search_take_a_repository_or_global_or_neither`).
- The memory list names its scope, `global` or the repository, in every row
  (`memory.rs::the_memory_list_names_its_scope`).

## Sources

`crates/ariadne-store/src/memories.rs`,
`crates/ariadne-api/src/memories.rs`,
`crates/ariadne-daemon/src/http/memories.rs`,
`crates/ariadne-cli/src/commands/memory.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`,
`crates/ariadne-cli/src/commands/mcp.rs` (the read rule),
`crates/ariadne-store/skills/` (the write steps),
`ui/src/features/memory/`.
