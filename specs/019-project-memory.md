---
id: project-memory
status: current
updated: 2026-09-10
areas: [store, api, daemon, mcp, cli, ui]
commits: []
tests:
  - crates/ariadne-daemon/tests/memories.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/features/memory/memory-page.test.tsx
---

# Project memory

Facts one session learns about a repository can help later sessions without
becoming part of every prompt.

## Scope

In: repository memory storage, expiry, source records, REST access, MCP save
and search tools, CLI list, search and delete commands, and the desktop
screen that offers the same three (015).

Out: prompt injection. Agents choose when to search.

## Behavior

1. Each memory belongs to one registered repository and contains text, its
   creation and expiry times, and the session, task and goal that sourced it.
2. An agent session saves a memory, and the daemon records its source from the
   session instead of accepting source fields from the caller.
3. A memory remains after its source session, task or goal is deleted. It is
   removed when its repository is deleted.
4. List and search return only memories whose expiry time is still ahead.
5. Search matches a case-insensitive literal substring inside the memory text.
6. An agent can read or change memories only for a repository in its task or
   goal. A user can manage any registered repository.
7. Every seat has `save_memory` and `search_memory`. A task session defaults
   to its task repository, and an orchestrator names a repository when its
   goal does not have exactly one.
8. No memory is added to a prompt. An agent calls `search_memory` when past
   repository knowledge can avoid repeated discovery or answer a question.
9. `ariadne memory ls|search|delete` names a repository by id or path and uses
   the shared list, mutation, JSON and quiet output forms (014).
10. The REST surface adds, lists, searches and deletes memory under one
    repository, and every endpoint appears in OpenAPI (012).
11. Memory creation emits the complete entry, and memory deletion emits the
    removed id on the domain event stream (012).
12. The desktop app has a memory page per repository: it lists, searches and
    deletes through the same REST endpoints the CLI uses, matching it (015).

## Acceptance criteria

- Another goal session of the same repository finds an author's saved memory
  after the source goal is deleted
  (`memories.rs::another_session_of_the_same_repository_finds_an_authors_memory`).
- A session of another repository does not find it
  (`memories.rs::another_repository_does_not_find_the_memory`).
- An expired memory never returns from list or search
  (`memories.rs::an_expired_memory_never_returns_from_list_or_search`).
- Delete removes a memory (`memories.rs::delete_removes_a_memory`), and the
  CLI accepts the entry and repository
  (`cli/tests.rs::memory_delete_takes_the_entry_and_its_repository`).
- Memory creation and deletion reach the domain event stream
  (`memories.rs::delete_removes_a_memory`).
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
- The desktop memory page lists, searches through the daemon's own search
  endpoint, and deletes an entry
  (`ui/src/features/memory/memory-page.test.tsx`).

## Sources

`crates/ariadne-store/src/memories.rs`,
`crates/ariadne-api/src/memories.rs`,
`crates/ariadne-daemon/src/http/memories.rs`,
`crates/ariadne-cli/src/commands/memory.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`,
`ui/src/features/memory/`.
