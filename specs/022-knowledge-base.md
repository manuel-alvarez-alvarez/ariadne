---
id: knowledge-base
status: current
updated: 2026-09-19
areas: [daemon, api, mcp, cli, ui]
commits: []
tests:
  - crates/ariadne-knowledge/tests/knowledge.rs
  - crates/ariadne-knowledge/src/languages.rs
  - crates/ariadne-knowledge/src/parser.rs
  - crates/ariadne-knowledge/src/resolve.rs
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

In: the language registry, the parser, the resolution pass that turns a
reference into an edge, the store and its schema, when the daemon indexes
what, the REST surface and its events, the four MCP tools, the
`ariadne knowledge` commands, the `knowledge_enabled` key, and the desktop
knowledge page over the same routes (015).

Out: resolution across repositories, and the edges a manifest, a route or an
environment variable makes (a later task adds them, with the interactions
listing that reads them off); languages beyond the registry here; what the
skill documents tell an agent to do with the tools (017); and the memory
tools beside these (019).

## Behavior

1. A file is read by its extension: 20 languages read by a tags query, and
   7 outline-only formats with no tags query, listed in the table below.
   Any other file is skipped.
2. A code file is parsed with tree-sitter and the tags query its grammar
   ships. Every definition the query captures is a symbol, of the query's
   own kind: `function`, `method`, `class`, `module`, `interface`, `macro`
   or `constant`. The Rust query gets one pattern more, naming every `impl`
   block by its type; the C# query loses its one bare `@module` capture,
   which is no tags capture; and each language that has base classes gets a
   `@reference.extends` pattern, because no upstream query tells a base apart
   from any other class reference. No query is vendored from elsewhere.
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
   of kind `test` named by the call's first string argument and scopes what
   its body names.
7. Every reference the tags query captures is a mention of one blob: its
   kind (`call` and `send` are `calls`, `implementation` is `implements`,
   `extends` is `extends`, every other capture is `references`), the name, the
   line, and the definition it sits in — none for a reference at file scope. A
   Rust `impl Trait for Type` is a mention from `Type` to `Trait`. One
   definition naming another names it once, at the first line it does.
8. The import statements of a file are read off its text, by the syntax of
   its language: `use` in Rust, `import` and `from … import` in Python,
   `import` and `require` in TypeScript and JavaScript, `using` in C#. Each
   one names a module, and a name in it where the statement names one — a
   glob, a whole-module import and every C# directive name only the module.
   An alias keeps the original name, which is what a definition is called.
   A named import is a mention of kind `imports`, at file scope. Every other
   language names no import, and its references resolve on the three steps
   that are left.
9. A mention becomes edges by looking for the definitions of its name in
   four places, nearest first: the same file, the same directory, the modules
   the file imports, and then anywhere in the repository at that ref. The
   first of those that holds a definition answers. One definition there is
   one edge marked `exact`; several are one edge to each, marked `heuristic`,
   and every edge carries how many matched. A step that holds more than 20
   definitions says nothing about which one was meant, and the mention is left
   unresolved. A module is what an import named where the definition's path,
   or its qualified name, carries the module's segments, past `crate`, `self`,
   `super` and a leading `.` or `/`.
10. Edges are keyed by the referencing blob at one ref of one repository, so
    deriving them again is one delete by that key and one insert. The ref is
    part of the key because the answer depends on it: two refs share the blob
    of an unchanged file, and each resolves its names against its own tree, so
    one edge set per blob would hold whichever ref resolved it last and the
    other ref would answer for a definition it does not have. A dropped ref
    and a dropped repository take their edges with them. A run derives the
    edges again for every blob it parsed, and for every blob of the ref that
    names a definition the run moved — which is every name the touched paths
    held before the run and after it.
11. A symbol's tests are the symbols marked `is_test` with a path of at most
    two `calls` edges to it: a test that calls it, and a test that calls
    something that calls it.
12. The store is one SQLite file, `knowledge.db`, beside the daemon's
    database, with the schema below. It is disposable: `PRAGMA user_version`
    holds the schema version, and a file at another version is deleted and
    rebuilt from the repositories.
13. Symbols are keyed by blob: a file whose content did not change is parsed
    once, and its symbols, its mentions and its imports are shared by every
    ref and every repository that holds it. A symbol's id is never given
    twice. A blob no file holds any more is dropped, with its symbols, its
    mentions, its imports and its edges, whenever a ref or a repository is.
14. The daemon indexes one repository at a time, off the request path. A
    repository's base branch is read at daemon start, at registration, when
    its base branch is edited, and after every landing (a task transition to
    `finished`). A task branch is read every time the daemon sees its head
    move (002, rule 11), and every in-flight task branch is read at daemon
    start — leniently: a task branch the repository no longer has fails
    nothing, and its rows go. A landing drops the rows of the task's branch
    and of its author branches (002, rule 6) before the base branch is read
    again. A deleted repository's rows go with it.
15. The first read of a ref takes every file git tracks at its head; every
    later read runs `git diff --name-only <last indexed commit> <head>` and
    reads only the changed paths. A file over 1 MiB, a binary file (a NUL
    byte in its first 8000 bytes), a symlink and a submodule are skipped,
    and a changed path the run skips loses the row it had. Content is read
    from the commit over `git cat-file --batch`, never from a working tree.
    The files of one batch are parsed a core each.
16. A search matches identifiers by their parts: a name is stored whole and
    split on camelCase, snake_case and the segments of its qualified name,
    and every word of the query is a prefix that has to match. An exact name
    comes first, then a name the query is a prefix of, then FTS rank.
17. `GET /v1/repositories/{id}/knowledge` answers `KnowledgeStatusDto`:
    `repository_id`, `state` (`idle`, `indexing`, `failed`, `disabled`),
    `refs` (each `git_ref`, `commit`, `indexed_at`, `files`, `symbols`),
    `files` (distinct paths across the refs), `symbols` (of the distinct
    blobs those paths hold), `languages` (`language`, `files`) and `error`.
18. `POST /v1/repositories/{id}/knowledge/reindex` marks the repository
    `indexing`, answers 202 with its status, and the worker then drops its
    rows and reads its base branch and every task branch it had again.
19. `GET /v1/knowledge/search` takes `q`, and optionally `repository`,
    `all`, `git_ref`, `kind`, `path` (a substring) and `limit` (default 20,
    max 50), and answers `KnowledgeHitDto`s: `repository_id`, `path`,
    `line`, `kind`, `name`, `signature`. Without `repository` a user
    searches every repository, and an agent session the repositories of its
    goal, or every one with `all=true`.
20. Without `git_ref` a repository is read at the caller's own ref: a task
    session's task branch, where that branch has been indexed in the task's
    repository, else the base branch. A task branch not yet indexed is the
    base branch's tree, so the base branch answers for it.
21. `GET /v1/knowledge/outline` takes `repository`, `path` and optionally
    `git_ref`, and answers `KnowledgeOutlineEntryDto`s in line order: `kind`,
    `name`, `start_line`, `end_line`, `signature`. A path not indexed at
    that ref is a 404.
22. `GET /v1/knowledge/symbol` takes `name`, and optionally `repository`,
    `git_ref` and `detail` (`outline`, `source` or `context`; `outline` by
    default). It answers one `KnowledgeSymbolDto` per definition of the name,
    in path order: `repository_id`, `path`, `start_line`, `end_line`, `kind`,
    `name`, `signature` and `doc`. `source` adds `source`, the text of the
    definition, read from the blob it was parsed from. `context` adds
    `callers`, `callees`, `implementations` and `tests`, each a list of
    `repository_id`, `path`, `line`, `name` and `confidence`, and each capped
    at 20 entries. Without `repository` a user reads every repository, and an
    agent session the repositories of its goal; the ref is the caller's own
    (rule 20).
23. `GET /v1/knowledge/impact` takes `repository`, optionally `git_ref` and
    `depth` (default 2, max 4), and exactly one of `symbol` and `diff`
    (`<base>..<head>`) — neither and both are refused. `symbol` names every
    definition of that name; `diff` names every definition whose lines a hunk
    of `git diff --unified=0 <base>..<head>` touched. A value that could read
    as a git flag is refused rather than run. For each changed definition it
    answers a `KnowledgeImpactDto`: the `symbol` itself, its `callers` by
    depth (`depth`, `repository_id`, `path`, `line`, `name`, `confidence`,
    each caller once and at its shortest depth), and `stopped`, the
    definitions the walk did not go past because each has more than 200
    callers.
24. Every index run publishes `knowledge_indexed` (`repository_id`,
    `git_ref`, `commit`, `files`, `symbols`) on the domain stream, and a
    failed run `knowledge_failed` (`repository_id`, `error`) (012). Neither
    belongs to a goal or a task.
25. Every seat has `search_code`, `outline`, `symbol` and `impact` (013).
    `search_code` passes `query`, `repository`, `all`, `git_ref`, `kind`,
    `path` and `limit` through to the search; the daemon applies the defaults
    of rules 19 and 20. `outline`, `symbol` and `impact` take the repository
    the memory tools take: the task's, then the goal's only one, and refuse
    with an instruction where the goal has several. `impact` with neither
    `symbol` nor `diff` is the task's own diff, the base branch to the task
    branch, for a reviewer, and a refusal naming both arguments for any other
    seat.
26. `search_code` and `outline` answer plain text, one line per result:
    `path:line kind name signature` for a search (each line led by its
    repository id where the answer spans several), `path:start-end kind name
    signature` for an outline. `symbol` and `impact` group their answer under
    headings: `# <repository id>` per repository, `## <location> …` per
    definition, and `### callers|callees|implementations|tests` per list of a
    context, an empty list reading `(none)`. An answer is cut at 8 KiB, with
    a last line naming how many results were left and saying to narrow the
    query.
27. `ariadne knowledge status|reindex|search|outline|symbol|impact` (014)
    read the same endpoints. `search` takes `--repository`, `--ref`,
    `--kind`, `--path` and `--limit`; `outline` takes the repository, the path
    and `--ref`; `symbol` takes the name, `--repository`, `--ref` and
    `--detail`; `impact` takes `--repository`, one of `--symbol` and
    `--diff`, `--ref` and `--depth`. `search`, `outline` and `impact` are
    listings, whose `-q` prints `path:line` and the line range; `symbol`
    prints a block per definition.
28. `knowledge_enabled` in `config.toml` defaults to true. False, the daemon
    indexes nothing and opens no store, the status says `disabled`, a
    search, an outline or a reindex is refused with a line naming the key,
    and every launch tells the session's MCP server
    (`ARIADNE_KNOWLEDGE_ENABLED=false`), which then lists and serves none of
    the four tools.
29. The desktop app's knowledge page (015) shows the status card, a Reindex
    button that posts the reindex and shows `indexing` at once, a search box
    over `q`, `kind` and `path`, and the interactions of the repository
    grouped by kind — reached from a row on the repositories screen and from
    the command palette, the way the memory page (019) is reached from its
    row. `knowledge_indexed` and `knowledge_failed` refetch what the page
    shows for their repository and leave every other repository's caches
    alone.

## Languages

The 20 tags languages, each with the grammar crate pinned in
`crates/ariadne-knowledge/Cargo.toml`, its test marker, and the import
syntax a later task's resolver reads (`edges` stays empty until then):

| Language | Extensions | Test marker | Import syntax |
| --- | --- | --- | --- |
| Rust | `rs` | `#[test]`, `#[tokio::test]` | `use path::Item;` |
| TypeScript, TSX | `ts`, `mts`, `cts`, `tsx` | `it(`/`test(` | `import { x } from "path"` |
| JavaScript, JSX | `js`, `mjs`, `cjs`, `jsx` | `it(`/`test(` | `import`/`require(` |
| C# | `cs` | `[Fact]`, `[Theory]`, `[Test]`, `[TestMethod]` | `using Namespace;` |
| Python | `py`, `pyi` | a `test_` name | `import module`, `from x import y` |
| Go | `go` | a `TestX` name | `import "path"` |
| Java | `java` | `@Test` | `import package.Class;` |
| C | `c`, `h` | none | `#include "file.h"` |
| C++ | `cpp`, `cc`, `cxx`, `hpp`, `hh`, `hxx` | none | `#include "file.hpp"` |
| Ruby | `rb` | a `describe`/`it` block | `require`/`require_relative` |
| PHP | `php` | a `test` name (PHPUnit) | `use Namespace\Class;` |
| Kotlin | `kt`, `kts` | `@Test` | `import package.Class` |
| Swift | `swift` | `@Test`, or a name starting with `test` | `import Module` |
| Dart | `dart` | `test(` | `import 'package:pkg/file.dart';` |
| Scala | `scala`, `sc` | `test(` | `import package.Class` |
| Bash | `sh`, `bash`, `bats` | a bats `@test` block | `source file.sh`, `. file.sh` |
| Lua | `lua` | none | `require("module")` |
| Elixir | `ex`, `exs` | `test "…" do` | `alias`/`import`/`use Module` |

Ruby's `_test.rb` filename convention is not read: `parse` takes a file's
text, not its path, so only the `describe`/`it` block marks a test. A bats
`@test "…" { … }` block is read as a call named `@test`, the same mechanism
as `it(`/`test(`; nothing else in Bash is read as a test. Neither C, C++
nor Lua has a common test marker. Swift's rule does not read whether the
function's class extends `XCTestCase`: a plain function or method named
`testConnection`, in any class, is marked as a test too.

Kotlin, Scala and Bash ship no tags query; `crates/ariadne-knowledge/src/vendor/`
holds one adapted from Aider (Apache-2.0) for each, attributed in `NOTICE`.
Swift's shipped query tags a method, an `init`/`deinit`/subscript or a
property with the range of the whole class or protocol around it; the
patterns that do this are dropped, so a method reads as kind `function`,
with its own range, and `init`, `deinit` and a subscript are not tagged.

Every grammar crate named above and below was checked against the
`tree-sitter` 0.27 runtime pinned here and built without changes; no
language was found unsupported.

The 7 outline-only formats have no tags query and no test marker; their own
structure stands in for definitions:

| Format | Extensions | Definitions |
| --- | --- | --- |
| Markdown | `md`, `markdown` | headings, each spanning to the next heading of its level |
| YAML | `yaml`, `yml` | top-level keys, and keys nested one level under a mapping |
| TOML | `toml` | top-level keys, and each `[table]`'s own keys |
| JSON | `json` | top-level keys, and keys nested one level under an object |
| HTML | `html`, `htm` | elements that carry an `id`, named by its value |
| CSS | `css` | each rule set's selector |
| SQL | `sql` | the object name of a `CREATE` or `ALTER` statement |

YAML, TOML and JSON stop at one level of nesting; a key deeper than that is
not a symbol. TOML's `[a.b]` is read as one table named `a.b`, not as `b`
nested under `a`: the grammar does not nest a dotted table under the table
its prefix names. SQL reads every `CREATE` and `ALTER` statement
`tree-sitter-sequel` names an object for — a table, a view, an index, a
function, a sequence, a type, a trigger, a materialized view or a schema —
by its own name: the first `object_reference` or bare identifier the
statement holds, before any table, column or rename target it goes on to
name. A statement of another kind is not read. `tree-sitter-toml` 0.20 and
`tree-sitter-sql` 0.0.2 predate `tree-sitter-language` and were not tried;
`tree-sitter-toml-ng` and `tree-sitter-sequel` are the maintained grammars
used instead, and both built against the runtime here — `tree-sitter-sequel`
at the cost of holding the workspace's `cc` build dependency below 1.3,
which its `~1.2.1` requirement pins for every crate that also builds with
`cc`, not only this one.

## Schema

`crates/ariadne-knowledge/src/schema.sql`, version 4:

| Table | Columns | Holds |
| --- | --- | --- |
| `repositories` | `id`, `state`, `error`, `updated_at` | every repository the index heard of, at `idle`, `indexing` or `failed` |
| `refs` | `repository_id`, `git_ref`, `commit_sha`, `indexed_at` | the refs read per repository, each at the commit it was last read at |
| `files` | `repository_id`, `git_ref`, `path`, `blob`, `language` | the tracked files of a ref, each with the blob it held there |
| `blobs` | `blob`, `language`, `parsed_at` | every blob parsed so far |
| `symbols` | `id` (autoincrement), `blob`, `kind`, `name`, `qualified_name`, `start_line`, `end_line`, `signature`, `doc`, `is_test` | the definitions of one blob |
| `symbols_fts` | `terms`, rowid = `symbols.id` | FTS5 over each symbol's identifier parts |
| `mentions` | `blob`, `kind`, `name`, `line`, `from_symbol` | the names one blob names, before they are resolved |
| `imports` | `blob`, `module`, `name`, `line` | the import statements of one blob |
| `edges` | `from_repository`, `git_ref`, `from_blob`, `kind`, `from_symbol`, `to_symbol`, `to_repository`, `from_line`, `confidence`, `candidates` | the relations the resolution pass derived, keyed by the referencing blob at one ref |

`refs` cascade from `repositories`, `files` from `refs`, and `symbols`,
`mentions`, `imports` and `edges` from `blobs` and `symbols`. Dropping a ref
or a repository then drops the blobs no file holds, with their symbols and
FTS rows, in two statements: the FTS rows are found by rowid, never by reading
the table; everything keyed by the blob goes with it.

`edges` is indexed in both directions under the ref a walk reads,
`(from_repository, git_ref, kind, from_symbol, to_symbol, confidence)` and
`(from_repository, git_ref, kind, to_symbol, from_symbol, confidence)`, so a
walk over the graph reads an index and no rows of the table; and by
`(from_repository, git_ref, from_blob)`, which is what deriving a blob's edges
again and dropping a ref delete by. `symbols` is indexed by `name`, which is
what resolution and `symbol` ask by.

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
  `::a_test_is_marked_by_the_rule_of_the_new_languages`,
  `::attributes_are_read_by_their_last_path_segment`,
  `knowledge.rs::a_test_definition_is_marked_in_every_language`).
- Each of the 6 outline-only formats added here reads its own structure as
  symbols — YAML, TOML and JSON their keys, HTML its elements with an `id`,
  CSS its selectors, SQL its `CREATE`/`ALTER` object names
  (`knowledge.rs::every_format_is_outlined`).
- An import is read by the syntax of its language, and a reference belongs to
  the definition it sits in — a Rust `impl Trait for Type` to the type, a base
  class to the class that names it
  (`parser.rs::an_import_is_read_by_the_syntax_of_its_language`,
  `::a_reference_belongs_to_the_definition_it_sits_in`).
- A name resolves at the nearest step that holds a definition, a name past the
  candidate cap resolves nowhere, and a module is matched by the path or the
  qualified name it names
  (`resolve.rs::a_name_resolves_at_the_nearest_step_that_holds_a_definition`,
  `::a_name_past_the_candidate_cap_is_left_unresolved`,
  `::a_module_is_matched_by_the_path_or_the_qualified_name_it_names`).
- A Rust file that calls a definition it brought in with `use` names it
  exactly, and the caller and the callee each list the other, while a second
  definition of the same name that nothing imports is called by nobody
  (`knowledge.rs::a_call_through_an_import_resolves_to_the_definition_it_named`);
  a name two files define, imported by neither, is a guess, and both are
  listed (`::a_name_defined_twice_resolves_to_both_as_a_guess`).
- Two files that import the same name each get an edge of their own
  (`resolve.rs::two_files_that_import_the_same_name_each_get_an_edge`).
- Edges belong to the ref they were resolved at: two refs share the blob of an
  unchanged caller and each answers for its own definition, a later run on the
  base branch does not take the branch's edges, and a dropped ref takes its
  own
  (`knowledge.rs::edges_belong_to_the_ref_they_were_resolved_at`).
- One import shape per language of the registry resolves the name it brought
  in: Rust `use`, Python `from pkg.m import x`, TypeScript
  `import { x } from './m'`, JavaScript `require`, C# `using`
  (`knowledge.rs::an_import_of_every_language_resolves_the_name_it_brought_in`).
- The callers are walked by depth, a test two edges away is a test of a
  definition, and nothing is reached from a definition nothing calls
  (`knowledge.rs::the_callers_are_walked_by_depth_and_a_test_two_edges_away_is_a_test`).
- A Rust `impl Trait for Type` is listed under the trait
  (`knowledge.rs::an_implementation_is_listed_under_the_trait_it_is_of`).
- A diff names the definitions it changed and no others, their callers are
  what the change reaches, and a range that is no range is refused
  (`knowledge.rs::a_diff_names_the_definitions_it_changed_and_their_callers`);
  the hunks of a file the diff deletes belong to no path
  (`index.rs::the_hunks_of_a_deleted_file_belong_to_no_path`).
- A definition with more than 200 callers is not walked past, and the answer
  names it
  (`knowledge.rs::a_definition_with_more_than_two_hundred_callers_is_not_walked_past`).
- A changed blob has its own edges derived again, and so has every blob that
  names a definition the change moved, and nothing else
  (`knowledge.rs::a_changed_blob_resolves_itself_and_what_names_what_moved`).
- A walk three deep over a hundred thousand edges answers in under 100 ms
  (`store.rs::impact_three_deep_over_a_hundred_thousand_edges_answers_under_a_tenth_of_a_second`).
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
- `symbol` answers the outline, the source and the context of a definition,
  and `impact` the callers of a change; a diff with no ref is read at its own
  head; one of `symbol` and `diff` is required and both are refused
  (`tests/it/knowledge.rs::the_symbol_and_impact_endpoints_answer_from_the_derived_graph`).
- Every endpoint is in the OpenAPI document
  (`tests/it/knowledge.rs::every_knowledge_endpoint_is_in_the_openapi_document`).
- Every seat lists `search_code`, `outline`, `symbol` and `impact`
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
- `symbol` groups its answer under a heading for each repository, each
  definition and each list of the context
  (`tools.rs::symbol_groups_its_answer_under_a_heading_for_each_repository`),
  and takes the task's repository by default
  (`::symbol_defaults_to_the_task_repository`).
- `impact` with no argument is the task's own diff for a reviewer
  (`tools.rs::impact_reads_the_task_diff_for_a_reviewer_that_names_nothing`),
  and a refusal for any other seat
  (`::impact_needs_a_symbol_or_a_diff_from_a_seat_that_is_no_reviewer`).
- The CLI commands are classified and parse their flags
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::knowledge_search_takes_its_filters`,
  `::knowledge_outline_takes_the_repository_and_the_path`,
  `::knowledge_symbol_takes_its_name_and_detail`,
  `::knowledge_impact_takes_a_symbol_or_a_diff_and_the_depth`), and a search row
  and an impact row each lead with their location
  (`commands/knowledge.rs::a_search_row_leads_with_its_location_and_titles_the_symbol`,
  `::an_impact_row_leads_with_its_location_and_says_how_far_away_it_is`).
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

Resolution reads one repository: a name that is only defined in another
repository resolves nowhere. An alias keeps the original name, so a file that
calls the definition by its alias resolves nothing. Two files of the same text
are one blob and so one definition, listed once per path and carrying the
edges of both. Dropping a ref drops the edges into the symbols it alone held,
and the blobs at other refs that pointed at them are not resolved again until
they or the names they name move.

A reviewer on a task staffed with several authors reads the task's first
branch by default and names another with `git_ref`. A blob is parsed as the
language of the first path it was seen at. The branch of a cancelled or failed
task, and of a task whose goal was deleted, keeps its rows until a reindex or
a restart drops what no longer resolves.

The desktop page also reads `GET /v1/knowledge/interactions`: the edges
found between files, grouped by kind (`depends_on`, `references`,
`calls_route`, `sets_env`), filtered by `repository` and `git_ref`, each
naming both ends (repository, path, line, symbol) and whether it was found
exactly or by a heuristic. `edges` holds `calls`, `references`, `implements`,
`extends` and `imports` now, and the two kinds a manifest and a route make are
the later task's, which is also the task that serves the listing. Until then
the page's interactions section shows the daemon's refusal.

## Sources

`crates/ariadne-knowledge/`, `crates/ariadne-daemon/src/knowledge.rs`,
`crates/ariadne-daemon/src/http/knowledge.rs`,
`crates/ariadne-api/src/knowledge.rs`,
`crates/ariadne-cli/src/commands/knowledge.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`, `docs/knowledge.md`,
`ui/src/features/knowledge/`.
