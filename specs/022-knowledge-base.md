---
id: knowledge-base
status: current
updated: 2026-09-18
areas: [daemon, api, mcp, cli, ui]
commits: []
tests:
  - crates/ariadne-knowledge/tests/knowledge.rs
  - crates/ariadne-knowledge/src/languages.rs
  - crates/ariadne-knowledge/src/parser.rs
  - crates/ariadne-knowledge/src/store.rs
  - crates/ariadne-knowledge/src/index.rs
  - crates/ariadne-daemon/tests/it/knowledge.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/knowledge.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/features/knowledge/knowledge-page.test.tsx
  - ui/src/events/dispatch.test.ts
  - ui/src/features/command-palette/command-palette.test.tsx
---

# Knowledge base

A symbol index over every registered repository, so an agent finds a
definition by name and reads a function by its line range instead of
grepping and slicing files.

## Scope

In: the language registry, the parser, the store and its schema, when the
daemon indexes what, the REST surface and its events, the two MCP tools, the
`ariadne knowledge` commands, the `knowledge_enabled` key, and the desktop
knowledge page over the same routes (015).

Out: references and the `edges` between symbols (a later task fills the
table reserved here, and the interactions read off it), what the skill
documents tell an agent to do with the tools (017), and the memory tools
beside these (019).

## Behavior

1. A file is read by its extension. The languages are Rust (`rs`),
   TypeScript (`ts`, `mts`, `cts`), TSX (`tsx`), JavaScript (`js`, `mjs`,
   `cjs`), JSX (`jsx`), C# (`cs`), Python (`py`, `pyi`) and Markdown (`md`,
   `markdown`). Any other file is skipped.
2. A code file is parsed with tree-sitter and the tags query its grammar
   ships. Every definition the query captures is a symbol, of the query's
   own kind: `function`, `method`, `class`, `module`, `interface`, `macro`
   or `constant`. The Rust query gets one pattern more, naming every `impl`
   block by its type, and the C# query loses its one bare `@module` capture,
   which is no tags capture. No query is vendored from elsewhere.
3. Markdown is outline-only: each heading is a symbol of kind `heading`,
   spanning to the line before the next heading of its level or above, and
   qualified by the headings above it (`Title > Sub`).
4. A symbol carries its name, its qualified name (the names of the
   definitions it sits in, joined by `::` in Rust, `.` elsewhere), its first
   and last line (1-based, inclusive), its signature (the definition's
   opening, on one line, up to its body, cut at 200 characters), its doc
   comment, and `is_test`.
5. The doc comment is what the tags query captured, else what the
   language's comment syntax finds: the `///` lines right above a Rust or C#
   definition, past its attributes; a `/** … */` block or `//` lines right
   above a JavaScript or TypeScript definition; a Python docstring. Markdown
   has none. A doc is cut at 1000 characters.
6. `is_test` is the language's marker: an attribute naming `test` on a Rust
   definition (`#[test]`, `#[tokio::test]`); a `test_` name in Python;
   `[Fact]`, `[Theory]`, `[Test]` or `[TestMethod]` on a C# method; in
   TypeScript and JavaScript an `it(` or `test(` call, which becomes a symbol
   of kind `test` named by the call's first string argument.
7. The store is one SQLite file, `knowledge.db`, beside the daemon's
   database, with the schema below. It is disposable: `PRAGMA user_version`
   holds the schema version, and a file at another version is deleted and
   rebuilt from the repositories.
8. Symbols are keyed by blob: a file whose content did not change is parsed
   once, and its symbols are shared by every ref and every repository that
   holds it. A symbol's id is never given twice. A blob no file holds any
   more is dropped, with its symbols, whenever a ref or a repository is.
9. The daemon indexes one repository at a time, off the request path. A
   repository's base branch is read at daemon start, at registration, when
   its base branch is edited, and after every landing (a task transition to
   `finished`). A task branch is read every time the daemon sees its head
   move (002, rule 11), and every in-flight task branch is read at daemon
   start — leniently: a task branch the repository no longer has fails
   nothing, and its rows go. A landing drops the rows of the task's branch
   and of its author branches (002, rule 6) before the base branch is read
   again. A deleted repository's rows go with it.
10. The first read of a ref takes every file git tracks at its head; every
    later read runs `git diff --name-only <last indexed commit> <head>` and
    reads only the changed paths. A file over 1 MiB, a binary file (a NUL
    byte in its first 8000 bytes), a symlink and a submodule are skipped,
    and a changed path the run skips loses the row it had. Content is read
    from the commit over `git cat-file --batch`, never from a working tree.
11. A search matches identifiers by their parts: a name is stored whole and
    split on camelCase, snake_case and the segments of its qualified name,
    and every word of the query is a prefix that has to match. An exact name
    comes first, then a name the query is a prefix of, then FTS rank.
12. `GET /v1/repositories/{id}/knowledge` answers `KnowledgeStatusDto`:
    `repository_id`, `state` (`idle`, `indexing`, `failed`, `disabled`),
    `refs` (each `git_ref`, `commit`, `indexed_at`, `files`, `symbols`),
    `files` (distinct paths across the refs), `symbols` (of the distinct
    blobs those paths hold), `languages` (`language`, `files`) and `error`.
13. `POST /v1/repositories/{id}/knowledge/reindex` marks the repository
    `indexing`, answers 202 with its status, and the worker then drops its
    rows and reads its base branch and every task branch it had again.
14. `GET /v1/knowledge/search` takes `q`, and optionally `repository`,
    `all`, `git_ref`, `kind`, `path` (a substring) and `limit` (default 20,
    max 50), and answers `KnowledgeHitDto`s: `repository_id`, `path`,
    `line`, `kind`, `name`, `signature`. Without `repository` a user
    searches every repository, and an agent session the repositories of its
    goal, or every one with `all=true`.
15. Without `git_ref` a repository is read at the caller's own ref: a task
    session's task branch, where that branch has been indexed in the task's
    repository, else the base branch. A task branch not yet indexed is the
    base branch's tree, so the base branch answers for it.
16. `GET /v1/knowledge/outline` takes `repository`, `path` and optionally
    `git_ref`, and answers `KnowledgeOutlineEntryDto`s in line order: `kind`,
    `name`, `start_line`, `end_line`, `signature`. A path not indexed at
    that ref is a 404.
17. Every index run publishes `knowledge_indexed` (`repository_id`,
    `git_ref`, `commit`, `files`, `symbols`) on the domain stream, and a
    failed run `knowledge_failed` (`repository_id`, `error`) (012). Neither
    belongs to a goal or a task.
18. Every seat has `search_code` and `outline` (013). `search_code` passes
    `query`, `repository`, `all`, `git_ref`, `kind`, `path` and `limit`
    through to the search; the daemon applies the defaults of rules 14 and
    15. `outline` takes the repository the memory tools take: the task's,
    then the goal's only one, and refuses with an instruction where the goal
    has several.
19. Both tools answer plain text, one line per result: `path:line kind name
    signature` for a search (each line led by its repository id where the
    answer spans several), `path:start-end kind name signature` for an
    outline. An answer is cut at 8 KiB, with a last line naming how many
    results were left and saying to narrow the query.
20. `ariadne knowledge status|reindex|search|outline` (014) read the same
    endpoints. `search` takes `--repository`, `--ref`, `--kind`, `--path`
    and `--limit`; `outline` takes the repository, the path and `--ref`.
    `search` and `outline` are listings, whose `-q` prints `path:line` and
    the line range.
21. `knowledge_enabled` in `config.toml` defaults to true. False, the daemon
    indexes nothing and opens no store, the status says `disabled`, a
    search, an outline or a reindex is refused with a line naming the key,
    and every launch tells the session's MCP server
    (`ARIADNE_KNOWLEDGE_ENABLED=false`), which then lists and serves
    neither tool.
22. The desktop app's knowledge page (015) shows the status card, a Reindex
    button that posts the reindex and shows `indexing` at once, a search box
    over `q`, `kind` and `path`, and the interactions of the repository
    grouped by kind — reached from a row on the repositories screen and from
    the command palette, the way the memory page (019) is reached from its
    row. `knowledge_indexed` and `knowledge_failed` refetch what the page
    shows for their repository and leave every other repository's caches
    alone.

## Schema

`crates/ariadne-knowledge/src/schema.sql`, version 2:

| Table | Columns | Holds |
| --- | --- | --- |
| `repositories` | `id`, `state`, `error`, `updated_at` | every repository the index heard of, at `idle`, `indexing` or `failed` |
| `refs` | `repository_id`, `git_ref`, `commit_sha`, `indexed_at` | the refs read per repository, each at the commit it was last read at |
| `files` | `repository_id`, `git_ref`, `path`, `blob`, `language` | the tracked files of a ref, each with the blob it held there |
| `blobs` | `blob`, `language`, `parsed_at` | every blob parsed so far |
| `symbols` | `id` (autoincrement), `blob`, `kind`, `name`, `qualified_name`, `start_line`, `end_line`, `signature`, `doc`, `is_test` | the definitions of one blob |
| `symbols_fts` | `terms`, rowid = `symbols.id` | FTS5 over each symbol's identifier parts |
| `edges` | `kind`, `from_symbol`, `to_symbol`, `from_repository`, `to_repository`, `confidence` | relations between symbols; empty until a later task fills it |

`refs` cascade from `repositories`, `files` from `refs`, `symbols` and
`edges` from `blobs` and `symbols`. Dropping a ref or a repository then
drops the blobs no file holds, with their symbols and FTS rows, in two
statements: the FTS rows are found by rowid, never by reading the table.

## Acceptance criteria

- A path is read by its extension and every tags query compiles
  (`languages.rs::a_path_is_read_by_its_extension`,
  `::every_tags_query_compiles`).
- A fixture repository holds one file per language, each with a definition,
  a reference and a doc comment: for each language `search_code` finds the
  definition and `outline` lists it with its line range, and a Markdown
  file's headings appear in its outline
  (`knowledge.rs::every_language_definition_is_found_and_outlined_with_its_line_range`).
- A Rust method is qualified by its `impl` type and carries its `///` doc,
  a Python docstring and a JavaScript block comment are docs, and Markdown
  headings span to the next heading of their level
  (`parser.rs::a_rust_method_is_qualified_by_its_impl_type_and_carries_its_doc`,
  `::a_python_docstring_and_a_javascript_block_comment_are_docs`,
  `::markdown_headings_span_to_the_next_heading_of_their_level`,
  `::an_impl_block_is_named_by_the_type_it_is_for`).
- A test is marked by the rule of its language, in the parser and in the
  store (`parser.rs::a_test_is_marked_by_the_rule_of_its_language`,
  `::attributes_are_read_by_their_last_path_segment`,
  `knowledge.rs::a_test_definition_is_marked_in_every_language`).
- A second commit that changes one file parses only that file, and a run
  over an unchanged ref parses nothing
  (`knowledge.rs::a_second_commit_that_changes_one_file_parses_only_that_file`).
- A branch cut from an indexed base parses only what it added, each ref
  answers for its own tree, and dropping the repository empties its status
  and prunes its blobs
  (`knowledge.rs::a_branch_is_indexed_on_its_own_and_shares_the_base_blobs`).
- Dropping one ref takes its files and the blobs only it held, and leaves
  the other refs whole
  (`knowledge.rs::a_dropped_ref_takes_its_files_and_its_orphan_blobs`).
- A changed file the run skips loses its symbols
  (`knowledge.rs::a_changed_file_the_run_skips_loses_its_symbols`).
- Indexing this repository at HEAD completes in under 30 seconds, and
  `search_code` for `add_worktree` answers
  `crates/ariadne-daemon/src/gitwt.rs` at the line of the function
  (`knowledge.rs::indexing_this_repository_at_head_finds_add_worktree_in_gitwt`).
- An identifier is split on camelCase, snake_case and path segments, and a
  query becomes prefix terms that all have to match
  (`store.rs::an_identifier_is_split_on_camel_case_snake_case_and_path_segments`,
  `::a_query_becomes_prefix_terms_that_all_have_to_match`).
- A store at another schema version is rebuilt
  (`store.rs::a_store_at_another_schema_version_is_rebuilt`).
- `git ls-tree` and `git cat-file --batch` are read record by record, and a
  NUL byte marks a binary
  (`index.rs::an_ls_tree_record_is_read_for_its_mode_blob_size_and_path`,
  `::a_cat_file_batch_is_read_object_by_object`, `::a_nul_byte_marks_a_binary`).
- A ref that does not resolve fails the run, and the status says why
  (`knowledge.rs::a_ref_that_does_not_resolve_fails_the_run`,
  `tests/it/knowledge.rs::a_repository_git_cannot_read_reads_as_failed`).
- Registering a repository indexes its base branch, and the status, the
  search and the outline read it back
  (`tests/it/knowledge.rs::registering_a_repository_indexes_its_base_branch`).
- A task branch head move indexes the new commit: the branch ref finds a
  symbol added on it, the base ref does not, and the author's own default
  is its branch
  (`tests/it/knowledge.rs::a_task_branch_head_move_indexes_the_new_commit`).
- A landing indexes the base branch again and drops the task's branch
  (`tests/it/knowledge.rs::a_landing_indexes_the_base_branch_again`), and a
  start reads a task branch that is gone without failing its repository
  (`::a_start_reads_a_task_branch_that_is_gone_without_failing_the_repository`).
- A reindex answers 202 and reads the repository again
  (`tests/it/knowledge.rs::a_reindex_drops_the_rows_and_indexes_again`).
- A session searches its goal's repositories by default and every one on
  `all=true`
  (`tests/it/knowledge.rs::a_session_searches_its_goals_repositories_and_all_on_request`).
- With the knowledge base off, the status says `disabled`, no store is
  written and a search is refused
  (`tests/it/knowledge.rs::with_the_knowledge_base_off_the_status_says_disabled_and_nothing_is_indexed`),
  the key is read from `config.toml`
  (`config.rs::knowledge_can_be_turned_off_in_the_config`), every launch
  says so (`adapters.rs::every_launch_carries_the_session_context`,
  `::a_launch_says_when_the_knowledge_base_is_off`), and no seat is listed a
  knowledge tool
  (`mcp.rs::the_knowledge_tools_are_not_listed_when_the_knowledge_base_is_off`).
- Every endpoint is in the OpenAPI document
  (`tests/it/knowledge.rs::every_knowledge_endpoint_is_in_the_openapi_document`).
- Every seat lists `search_code` and `outline`
  (`mcp.rs::every_seat_has_the_tools_its_playbook_names_and_no_others`), and
  their descriptions are Simplified Technical English
  (`mcp.rs::every_text_the_server_hands_an_agent_is_simplified_technical_english`).
- `search_code` passes its filters through and answers one line per hit,
  travels bare by default and widens on `all`
  (`tools.rs::search_code_asks_the_daemon_with_its_filters_and_answers_one_line_per_hit`,
  `::search_code_defaults_to_the_sessions_own_scope_and_widens_on_request`).
- An answer over 8 KiB is cut with the number of results left
  (`tools.rs::an_answer_over_8_kib_is_cut_with_the_number_of_results_left`).
- `outline` takes the task's repository by default and lists line ranges
  (`tools.rs::outline_defaults_to_the_task_repository_and_lists_line_ranges`).
- The CLI commands are classified and parse their flags
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::knowledge_search_takes_its_filters`,
  `::knowledge_outline_takes_the_repository_and_the_path`), and a search row
  leads with its location
  (`commands/knowledge.rs::a_search_row_leads_with_its_location_and_titles_the_symbol`).
- The desktop knowledge page renders the status card in every state, posts a
  reindex and shows `indexing` at once, refetches once `knowledge_indexed` or
  `knowledge_failed` arrives, searches with `q`, `kind` and `path`, and groups
  interactions by kind with both ends and their confidence
  (`ui/src/features/knowledge/knowledge-page.test.tsx`).
- `knowledge_indexed` and `knowledge_failed` invalidate a repository's
  knowledge status and every interactions list under it, and leave another
  repository's caches alone
  (`ui/src/events/dispatch.test.ts::knowledge events (022)`).
- The command palette opens a repository's knowledge page
  (`ui/src/features/command-palette/command-palette.test.tsx::opens a
  repository's knowledge page from the palette`).

## Known gap

References are parsed only where a rule needs them (the `it(`/`test(`
calls) and are not stored; `edges` stays empty until the task that reads
references fills it. A reviewer on a task staffed with several authors reads
the task's first branch by default and names another with `git_ref`. A blob
is parsed as the language of the first path it was seen at. The branch of a
cancelled or failed task, and of a task whose goal was deleted, keeps its
rows until a reindex or a restart drops what no longer resolves.

The desktop page also reads `GET /v1/knowledge/interactions`: the edges
found between files, grouped by kind (`depends_on`, `references`,
`calls_route`, `sets_env`), filtered by `repository` and `git_ref`, each
naming both ends (repository, path, line, symbol) and whether it was found
exactly or by a heuristic. The daemon does not serve it yet — the task that
fills `edges` adds it — so until then the page's interactions section shows
the daemon's refusal.

## Sources

`crates/ariadne-knowledge/`, `crates/ariadne-daemon/src/knowledge.rs`,
`crates/ariadne-daemon/src/http/knowledge.rs`,
`crates/ariadne-api/src/knowledge.rs`,
`crates/ariadne-cli/src/commands/knowledge.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`, `docs/knowledge.md`,
`ui/src/features/knowledge/`.
