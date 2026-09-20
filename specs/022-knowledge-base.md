---
id: knowledge-base
status: current
updated: 2026-09-20
areas: [daemon, api, mcp, cli, ui]
commits: []
tests:
  - crates/ariadne-knowledge/tests/knowledge.rs
  - crates/ariadne-knowledge/src/languages.rs
  - crates/ariadne-knowledge/src/parser.rs
  - crates/ariadne-knowledge/src/interfaces.rs
  - crates/ariadne-knowledge/src/resolve.rs
  - crates/ariadne-knowledge/src/map.rs
  - crates/ariadne-knowledge/src/store.rs
  - crates/ariadne-knowledge/src/index.rs
  - crates/ariadne-daemon/tests/it/knowledge.rs
  - crates/ariadne-daemon/tests/it/adapters.rs
  - crates/ariadne-daemon/src/config.rs
  - crates/ariadne-cli/src/commands/mcp.rs
  - crates/ariadne-cli/src/commands/mcp/tools.rs
  - crates/ariadne-cli/src/commands/knowledge.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/features/knowledge/knowledge-screen.test.tsx
  - ui/src/features/knowledge/repositories-graph.test.ts
  - ui/src/features/knowledge/symbols-graph.test.ts
  - ui/src/features/knowledge/symbols-tab.test.tsx
  - ui/src/features/knowledge/impact-graph.test.ts
  - ui/src/features/knowledge/path-graph.test.ts
  - ui/src/features/knowledge/impact-tab.test.tsx
  - ui/src/features/knowledge/graph/layered-layout.test.ts
  - ui/src/features/knowledge/graph/force-layout.test.ts
  - ui/src/features/knowledge/files-graph.test.ts
  - ui/src/features/knowledge/graph/knowledge-graph.test.tsx
  - ui/src/events/dispatch.test.ts
  - ui/src/features/command-palette/command-palette.test.tsx
---

# Knowledge base

A symbol index over every registered repository, so an agent finds a
definition by name and reads a function by its line range instead of
grepping and slicing files — and, the store being one for every repository,
so a change in one repository shows its effect in another.

## Scope

In: the language registry, the parser, the resolution pass that turns a
reference into an edge, the interfaces a file holds beyond its symbols and
the link pass that matches them between repositories, the store and its
schema, when the daemon indexes what, the REST surface and its events, the
six MCP tools, the `ariadne knowledge` commands, the `knowledge_enabled`
key, and the desktop knowledge screen over the same routes (015).

Out: languages beyond the registry here; what the skill documents tell an
agent to do with the tools (017); and the memory tools beside these (019).

## Behavior

1. A file is read by its extension: 20 languages read by a tags query, and
   7 outline-only formats with no tags query, listed in the table below. A
   manifest with no language of its own — `go.mod`, `pom.xml`,
   `build.gradle`, `settings.gradle`, a `.env` file (`.env`, `.env.*`) — is
   read by its name, as the language `manifest`, for its interfaces alone
   (rule 30). Any other file is skipped.
2. A code file is parsed with tree-sitter and the tags query its grammar
   ships. Every definition the query captures is a symbol, of the query's
   own kind: `function`, `method`, `class`, `module`, `interface`, `type`,
   `macro` or `constant`. The Rust query gets one pattern more, naming every
   `impl` block by its type; the TypeScript and TSX query gets two more, a
   `type X = …` alias as a `type` and an `enum` as a `class`, which the
   shipped query tags neither of, and a type in an annotation is the
   `references` mention the shipped query already tags; the C# query loses its one bare `@module` capture,
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
   `import` and `require` in TypeScript and JavaScript, `using` in C#, and
   `import` and `export` in Dart. Each one names a module, and a name in it
   where the statement names one — a glob, a whole-module import and every
   C# directive name only the module. A Dart `show` clause names each shown
   definition; its `as` and `hide` clauses name none, and its `dart:` SDK
   directives are ignored. An alias keeps the original name, which is what a
   definition is called. A named import is a mention of kind `imports`, at
   file scope. Every other language names no import, and its references
   resolve on the three steps that are left.
9. A mention becomes edges by looking for the definitions of its name in
   four places, nearest first: the same file, the same directory, the
   modules the file imports, and then anywhere in the repository at that
   ref. The first of those that holds a definition answers. One definition
   there is one edge marked `exact`; several are one edge to each, marked
   `heuristic`, and every edge carries the step that answered — `file`,
   `directory`, `import` or `repository` — and how many definitions matched
   there. A step that holds too many definitions says nothing about which
   one was meant, and the mention is left unresolved: more than 3 at step
   `repository`, where the name is all that joins the two ends, and more
   than 20 at the three nearer steps, which stand on where the definition
   is. At every step, definitions from outline-only formats do not hold a
   candidate; a step with only those definitions is empty and the search
   continues. A module is what an import named where the definition's path,
   or its qualified name, carries the module's segments, past `crate`,
   `self`, `super` and a leading `.` or `/`.
10. An edge joins two ends, each a blob at one ref of one repository, a line,
    and the definition there where there is one. A symbol edge (rule 9) is
    keyed by the referencing blob at one ref of one repository, so deriving
    it again is one delete by that key and one insert. The ref is part of
    the key because the answer depends on it: two refs share the blob of an
    unchanged file, and each resolves its names against its own tree, so one
    edge set per blob would hold whichever ref resolved it last and the
    other ref would answer for a definition it does not have. A dropped ref
    and a dropped repository take their edges with them, at either end. A
    run derives the symbol edges again for every blob it parsed, and for
    every blob of the ref that names a definition the run moved — which is
    every name the touched paths held before the run and after it — and then
    runs the link pass (rule 31).
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
    default). It answers one `KnowledgeSymbolDto` per definition of the
    name, in path order: `repository_id`, `path`, `start_line`, `end_line`,
    `kind`, `name`, `signature` and `doc`. `source` adds `source`, the text
    of the definition, read from the blob it was parsed from. `context` adds
    `callers` (the `calls` and `calls_route` edges into it), `callees`,
    `implementations`, `references` (the `references` and `imports` edges
    into it, which is where a reference from another repository is listed)
    and `tests`, each a list of `repository_id`, `path`, `line`, `name`,
    `confidence`, `step` and `candidates`, each capped at 20 entries, and
    each listing the definition's own repository first and every other
    repository's ends after it. `context` also adds `more`: `callers`,
    `callees`, `implementations`, `references` and `tests`, each how many
    entries the matching list held back past its cap, 0 where the list is
    whole. Without `repository` a user reads every repository, and an agent
    session the repositories of its goal; the ref is the caller's own (rule
    20).
23. `GET /v1/knowledge/impact` takes `repository`, optionally `git_ref` and
    `depth` (default 2, max 4), and exactly one of `symbol` and `diff`
    (`<base>..<head>`) — neither and both are refused. `symbol` names every
    definition of that name; `diff` names every definition whose lines a
    hunk of `git diff --unified=0 <base>..<head>` touched. A value that
    could read as a git flag is refused rather than run. For each changed
    definition it answers a `KnowledgeImpactDto`: the `symbol` itself, whose
    `step` is null because it is a definition and no end of an edge, its
    `callers` by depth (`depth`, `repository_id`, `path`, `line`, `name`,
    `confidence`, `step`, `candidates`, each caller once and at its shortest
    depth), and `stopped`, the definitions the walk did not go past because
    each has more than 200 callers. The walk follows `calls` and
    `calls_route` edges alike, so a request in another repository is a
    caller of the handler it reaches, and the walk goes on in that
    repository at the ref the edge names. Within one depth the changed
    definition's own repository comes first.
24. Every index run publishes `knowledge_indexed` (`repository_id`,
    `git_ref`, `commit`, `files`, `symbols`) on the domain stream, and a
    failed run `knowledge_failed` (`repository_id`, `error`) (012). Neither
    belongs to a goal or a task.
25. Every seat has `search_code`, `outline`, `symbol`, `path`, `impact` and
    `repo_map` (013).
    `search_code` passes `query`, `repository`, `all`, `git_ref`, `kind`,
    `path` and `limit` through to the search; the daemon applies the defaults
    of rules 19 and 20. `outline`, `symbol`, `path` and `impact` take the
    repository the memory tools take: the task's, then the goal's only one,
    and refuse with an instruction where the goal has several. `impact` with
    neither `symbol` nor `diff` is the task's own diff, the base branch to the
    task branch, for a reviewer, and a refusal naming both arguments for any
    other seat. `repo_map` with no `repository` maps every repository of the
    session's goal, each on its own share of the budget and under a heading
    of its own, and refuses a goal that names none.
26. `search_code` and `outline` answer plain text, one line per result:
    `path:line kind name signature` for a search (each line led by its
    repository id where the answer spans several), `path:start-end kind name
    signature` for an outline. `symbol`, `path` and `impact` group their
    answer under headings: `# <repository path>` per repository — the path
    the repository is registered at, read once per call from `GET
    /v1/repositories`, which is what an agent knows a repository by; its id
    where it is no longer listed — `## <location> …` per definition, and
    `### callers|callees|implementations|references|tests` per list of a
    context, an empty list reading `(none)`, and a list past its cap ending
    with a line naming how many more matched and saying to narrow the query.
    An end of a list reads `path:line name <confidence> via <step>`, and `,
    <n> candidates` where the step held more than one — `heuristic via
    directory, 3 candidates`, `exact via file` — and an impact's caller
    reads the same after its depth. The lists under a definition hold its
    own repository's ends; every other repository's ends follow under a `#`
    heading of that repository's own, with only the lists it has an end in
    (`symbol`), its ordered hops (`path`), or its callers alone (`impact`).
    A path hop is `path:line kind name <- edge_kind confidence`, with no
    edge on the first hop; an empty path is `(no path within N)`. An answer
    is cut at 8 KiB, with a last line naming how many results were left and
    saying to narrow the query.
27. `ariadne knowledge status|reindex|search|outline|symbol|path|impact|interactions|map|graph`
    (014) read the same endpoints. `search` takes `--repository`, `--ref`,
    `--kind`, `--path` and `--limit`; `outline` takes the repository, the path
    and `--ref`; `symbol` takes the name, `--repository`, `--ref` and
    `--detail`; `path` takes `from`, `to`, `--repository`, `--ref` and
    `--depth`; `impact` takes `--repository`, one of `--symbol` and
    `--diff`, `--ref` and `--depth`; `interactions` takes the repository and
    `--ref`; `map` takes the repository, `--path`, `--budget` and `--ref`,
    and prints the map's text as it came; `graph` takes the repository,
    `--ref`, `--limit` and `--json`. Without `--json`, `graph` prints its
    node count, its edge counts by kind, and the ten files with the largest
    degree. With `--json`, it prints the route response. `search`,
    `outline`, `path`,
    `impact` and `interactions` are listings,
    whose `-q` prints `path:line` and the line range — an interaction row
    leads with its from end as `repository:path:line`, and names its kind,
    its to end, its confidence and the step that joined them, which an
    impact row names too; `--format json` prints the daemon's groups;
    `symbol` prints a block per definition, an end in another repository led
    by that repository's id and followed by its confidence, its step and its
    candidate count.
28. `knowledge_enabled` in `config.toml` defaults to true. False, the daemon
    indexes nothing and opens no store, the status says `disabled`, a
    search, an outline or a reindex is refused with a line naming the key,
    and every launch tells the session's MCP server
    (`ARIADNE_KNOWLEDGE_ENABLED=false`), which then lists and serves none of
    the six tools.
29. The desktop app's knowledge screen (015) is a sidebar entry at
    `#/knowledge`, over every repository. A repository picker and a ref
    picker lead it; the refs are the ones the status lists. Its tabs are
    `overview`, `repositories`, `symbols`, `impact` and `files`, and the URL
    keeps the picks and the tab. The Overview tab shows a card per
    repository with its status and a Reindex button that posts the reindex
    and shows `indexing` at once. The Repositories tab draws the
    interactions between repositories as a graph: a node per repository, an
    edge per pair and kind with its count, dashed where it is heuristic only,
    and a click on an edge lists its file-level ends with the step each
    rests on. The Impact & path tab draws a symbol's callers and the path
    between two symbols in layers (015). The command palette opens the
    screen on a repository. `knowledge_indexed` and `knowledge_failed`
    refetch what the screen shows for their repository and leave every other
    repository's caches alone; for the Files tab that includes its file
    graphs and outlines.
    The Symbols tab searches the selected repository and ref by name, kind,
    and path. Its graph puts the chosen definition at the centre. Callers,
    callees, implementations, references, and tests surround it in separate
    colours. Heuristic edges are dashed. Foreign nodes carry an arrow. Each
    relation gets one node for its hidden count. A node click centres that
    definition. Back and Forward move through the symbol history. The URL
    keeps the name in `?symbol=`. Equal names offer a definition picker. The
    side pane shows the signature, documentation, line range, and numbered
    source. Find symbol in the palette opens this tab.
    The Files tab draws `GET /v1/knowledge/graph` for the picked
    repository and ref. It opens with one depth-two directory node per group,
    sized by its symbols and coloured by its top-level directory, and an edge
    per node pair and kind weighted by its count. `?level=file` shows files.
    The level switch merges files by directory to a depth of path segments and
    sums their edges on the client. A directory click expands its files while
    the other directories stay grouped; `?open=` keeps that expansion through
    reloads, and a second click or Collapse closes it. The tab shows the shown
    and total node and edge counts. Path text, edge kinds and Hide unlinked
    filter it; a truncated response says how many of `total_nodes` files it
    shows and offers a higher `limit`; and a click on a file shows its outline,
    its edges both ways and a link per symbol to the Symbols tab.
    Every graph is drawn settled and still. A force graph is placed before
    its first frame: ForceAtlas2 runs a fixed number of iterations on the
    client, and an edge pulls with `1 + log2(count)` rather than with its
    count. A second pass then pushes apart every pair of nodes that cover
    each other, counted at the size each node is drawn and at the narrowest
    canvas the app draws a graph on — the window at the width the side pane
    opens beside it — so no node is drawn on top of another at any supported
    window size: a wider window only spreads the same graph further. A graph
    whose nodes cover more of that canvas than any arrangement of them fits
    in — the Files tab of a large repository — is drawn with smaller nodes
    instead: one share comes off every node, down to a floor that keeps a
    node visible, so the biggest node is still the biggest and the user reads
    the names by zooming in. The same graph gives the same places, and no
    node moves after that frame unless the user drags it. The names are
    picked by density, so the labels that
    are drawn are far enough apart to read; a hovered node and its
    neighbours are named whatever the density, and an edge is named only
    while the pointer is on one of its ends.
    Every free-text filter of the screen suggests its values under what is
    typed, in one list below the field. The three Impact & path fields and
    Search symbols suggest the symbols `search` finds, each row with its
    name, its kind and its path. Path and Filter by path suggest the
    directories and the files of the ref that hold the text, directories
    first, twenty rows at most, from the file graph the Files tab reads.
    Down and Up move through the rows. Enter takes the active row into the
    field, which then applies as typed text does. Escape closes the list and
    keeps the text. Free text stays valid: a field takes a value no row
    offers.
30. Beyond its symbols, a file holds interfaces: what it offers another
    repository and what it takes from one. Each is read off the text at parse
    time, kept per blob like the mentions, and carries the definition it sits
    in where there is one:
    - A manifest, read by its file name, names the package it defines and
      the packages it depends on. `Cargo.toml`: `[package] name`, and every
      key of a `dependencies`, `dev-dependencies`, `build-dependencies`,
      `workspace.dependencies` or `target.*.dependencies` table, a `path`
      dependency told from one by name and `package = "x"` naming what is
      depended on. `package.json`: `name`, and every key of `dependencies`,
      `devDependencies`, `peerDependencies` and `optionalDependencies`, a
      `file:`, `link:`, `workspace:` or `portal:` value being a path
      dependency, and every `workspaces` entry that names a directory a path
      dependency on the package named by its last segment. `pubspec.yaml`:
      `name`, and every key under `dependencies`, `dev_dependencies` and
      `dependency_overrides`, one with a `path:` key under it by path.
      `go.mod`: `module`, every `require`, and every `replace` to a
      directory as a path dependency. `pom.xml`: the project's own
      `groupId:artifactId` — the first outside `<parent>`, `<dependency>` and
      `<plugin>` — and one per `<dependency>`. `build.gradle` and
      `build.gradle.kts`: the `group:artifact` of every `implementation`,
      `api`, `compileOnly`, `runtimeOnly`, `testImplementation`,
      `testCompileOnly`, `testRuntimeOnly`, `annotationProcessor`, `kapt`
      and `classpath` line, a `project(':x')` being a path dependency on `x`;
      `settings.gradle` and `settings.gradle.kts`: `rootProject.name`. Best
      effort throughout: a manifest shape not named here names nothing.
    - A string literal that starts with `/` and has two or more segments, in
      the arguments of a call, is a route. In a call that registers one it is
      a route template: a call named `route`, `nest`, `route_service`,
      `handle`, `Handle`, `HandleFunc`, `handleFunc`, `resource`, `service`,
      `mount`, `MapGet`, `MapPost`, `MapPut`, `MapDelete`, `MapPatch`,
      `MapMethods`, `RequestMapping`, `GetMapping`, `PostMapping`,
      `PutMapping`, `DeleteMapping`, `PatchMapping` or `Path`; an HTTP verb
      (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, `all`,
      `any`, `use`, in lowercase, uppercase or capitalised) on a receiver
      named `app`, `router`, `routes`, `r`, `mux`, `server`, `srv`, `group`,
      `g`, `route` or `sub`; or any of those under a decorator (`@app.get`)
      or an attribute (`#[get(…)]`). In a request call it is a route use: a
      verb on any other receiver (`axios.get`, `client.get`, `http.Get`), or
      a call named `fetch`, `request`, `Request`, `NewRequest`,
      `NewRequestWithContext`, `open`, `ajax`, `getJSON`, `apiFetch`,
      `daemonFetch` or `$http`. A query string and a trailing `/` are
      dropped. A route read off a verb carries that verb, uppercased, as its
      method: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD` or `OPTIONS`,
      and no method for `all`, `any` and `use`. A template carries the
      identifiers the registration passes after the literal, up to a
      closure's body and past the verbs and the routing words, as its
      handler names, and sits in the definition a decorator or an attribute
      is on, else the definition around it.
    - In Dart, a `get`, `post`, `put`, `patch` or `delete` call that requests
      a route — a verb on a receiver that is no router — takes its first
      string literal as the route, which needs no leading `/`: the client
      holds the prefix the path hangs under
      (`_api.put('users/$id/role', …)`). Each `$name` in it is written
      `${name}` and each `${expr}` is kept, which is the form a template
      literal gives, so an interpolation is one path parameter. The literal
      is a route where every character outside an interpolation is a letter,
      a digit, `-`, `_`, `.` or `/`, and one segment is enough. An
      interpolation holds a Dart expression, which is any text
      (`'teams/${team.id.toString()}/members'`), and a `?` in it starts no
      query string (`'members/${user?.id}/cards'`).
    - An environment variable is read by `env::var(`, `env::var_os(`,
      `option_env!(`, `env!(`, `process.env.X`, `process.env[`,
      `import.meta.env.X`, `Deno.env.get(`, `os.environ[`, `os.environ.get(`,
      `os.getenv(`, `GetEnvironmentVariable(`, `Platform.environment[`,
      `String.fromEnvironment(`, `os.Getenv(`, `os.LookupEnv(`,
      `System.getenv(`, `System.get_env(`, `ENV[`, `ENV.fetch(` and
      `getenv(`, the name a string literal or, after a dot, an identifier;
      and set by `env::set_var(`, by an `X=` line of a `.env` file (with or
      without `export`), and by an `X:` or `- X=` line of a compose file
      (`docker-compose*.yml`, `compose*.yml`) or a workflow
      (`.github/workflows/*.yml`) whose `X` is an uppercase name.
31. The link pass runs after every index run of a ref, and derives the edges
    between that ref and every other repository at its base ref, in both
    directions, and the interface edges within the ref itself — each pair of
    refs derived whole and replaced as one. The base ref of a repository is
    the base branch the daemon records before it reads it, and the first ref
    indexed until it does; a repository whose base ref is not indexed is
    left out. Three edges come off the interfaces, marked `exact` or
    `heuristic`, each naming the step that joined its two ends:
    - `depends_on`, from a dependency to every package of the same name the
      other ref defines: `exact` at step `path` for a path dependency,
      `heuristic` at step `name` for one by name. Both ends are manifest
      lines and no definition.
    - `calls_route`, from a route use to every template it fits segment by
      segment, a segment that starts with `:`, `{`, `<`, `*` or `$` or holds
      `$` or `{` standing for any value on either side: `exact` where every
      segment is the same, `heuristic` where a wildcard stood in for one,
      and no edge where the counts differ. A use with no leading `/` is a
      path below a prefix the client holds, so it is matched against the
      last segments of the template — `users/${id}/role` fits
      `/api/users/{id}/role` — and such a match is `heuristic`, the prefix
      being unread. The edge points at the handler:
      the definition of a handler name the template carries, in the
      template's own file first, then its directory, then anywhere in the
      ref under the candidate cap, one edge to each; and at the registration
      itself, with the definition it sits in, where none resolves. A
      definition of an outline-only format does not answer a handler name.
      Its step is `route`.
    - `sets_env`, from a set to every read of the same variable, `exact` at
      step `name`.
    Then, between two repositories joined by a `depends_on` edge in either
    direction only, `references`: every mention or named import of a name at
    least 4 characters long that the ref defines nowhere, pointed at every
    non-outline definition of that name the other ref holds under the
    candidate cap, `heuristic` at step `name`, carrying how many matched, and
    keyed by the name. The interface edges of every pair are derived before
    its references, so the manifest relationship is available first.
32. `GET /v1/knowledge/interactions` takes `repository` and optionally
    `git_ref` (the caller's own by default, rule 20), and answers the edges
    whose from end or to end is that ref and whose other end is another
    repository — the edges of rule 31 and no edge within one repository —
    as `KnowledgeInteractionGroupDto`s, one per kind that has an edge, in
    the order `depends_on`, `references`, `calls_route`, `sets_env`, each
    holding `kind` and `edges`. An edge is `from`, `to`, `confidence`,
    `step` and `candidates`;
    each end is a `KnowledgeEndpointDto` — `repository_id`, `path`, `line`
    and `symbol`, the definition at that end, or what the edge is about
    (the package, the route template, the variable, the referenced name)
    where the end sits in no definition. The edges of a kind come in the
    order of their from end, then their to end.

33. `GET /v1/knowledge/map` takes `repository` and optionally `git_ref` (the
    caller's own by default, rule 20), `path` and `budget` (tokens, 1000 by
    default, 4000 at most, a token counted as four characters). It ranks the
    files of that ref by PageRank over the symbol edges between them —
    `calls`, `references`, `implements`, `extends` and `imports` (rule 9),
    and no interface edge of rule 31, whose ends are manifest lines rather
    than definitions — the walk restarting at `path` where the call names one
    and at the whole ref where it does not, and answers a `KnowledgeMapDto`:
    `repository_id`, `git_ref`, `text`, `tokens`, `files` and `files_left`.
    A reference joins two files the way it points, and back at a quarter of
    that, so a map of the whole ref names what the ref depends on, and a map
    toward one path names that file's neighbours whichever way the reference
    points. The text is a line per file — its path — and under it a line per
    definition, `<start>-<end> <kind> <name> <signature>`, indented by two
    spaces: the definitions most of the ref points at, 10 a file, in line
    order. A file that defines nothing is left out, 100 files are ranked at
    most, and the text stops at the last whole line the budget holds.
    `files_left` is the ranked files the text did not hold, 0 where the
    budget held them all. Where the cut leaves room for the whole line, the
    text ends with it: `N files left. Raise the budget or name a path.` —
    the same rule every other line keeps, so a budget under the line's own
    length holds none of it, though `files_left` still counts what was left
    out.

34. `GET /v1/knowledge/path` takes `repository`, `from`, `to`, and optionally
    `git_ref` (the caller's own by default, rule 20) and `depth` (6 by default,
    10 at most). Every definition of `from` in that repository and ref is a
    start. Every definition of `to` in a registered repository, at that
    repository's own ref, is an end. A two-sided level walk follows `calls`,
    `calls_route`, `references`, `implements`, `extends` and `imports` in the
    direction they point, and crosses a repository at the ref the edge names.
    It answers one `KnowledgePathDto`, whose `hops` hold the shortest path in
    order from `from` to `to`. Each hop has `repository_id`, `path`, `line`,
    `kind`, `name`, and the `edge_kind` and `confidence` of its incoming edge;
    the first hop has neither edge field. `hops` is empty when no path exists
    within `depth`.

35. `GET /v1/knowledge/graph` takes `repository`, and optionally `git_ref`
    (the caller's own by default, rule 20) and `limit` (2000 by default,
    10000 at most). It answers one `KnowledgeGraphDto`: `repository_id`,
    `git_ref`, `nodes`, `edges`, `truncated` and `total_nodes`. A node is one
    file at the ref: `path`, `language` and its `symbols` count. An edge is
    one `(from, to, kind)` of file paths, grouped first by the edge blobs and
    then joined to the files at the ref: `count` is its symbol-level edge
    count, and `confidence` is `exact` only when every grouped edge is exact,
    else `heuristic`. Only edges whose two ends are the named repository and
    ref are kept. An edge whose blobs match is removed before paths are
    joined, and an edge from a path to itself is removed. When the ref has
    more files than `limit`, the response keeps the files with the
    largest degree, removes edges with an end outside that set, and sets
    `truncated`; degree counts the grouped edges at both ends. `total_nodes`
    is the file count before that cut.

## Languages

The 20 tags languages, each with the grammar crate pinned in
`crates/ariadne-knowledge/Cargo.toml`, its test marker, and the import
syntax the resolver reads:

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
| Dart | `dart` | `test(` | `import` or `export` a `package:` or relative URI; `show` names definitions |
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

The manifests are no grammar: `Cargo.toml`, `package.json`, `pubspec.yaml`,
the compose files, the workflows and `build.gradle.kts` are outlined by the
format above and read for their interfaces by their name besides; `go.mod`,
`pom.xml`, `build.gradle`, `settings.gradle` and a `.env` file are read for
their interfaces alone, under the language `manifest`, and define nothing.
`package.json` is walked with the JSON grammar; every other manifest is a
scan of its lines.

## Schema

`crates/ariadne-knowledge/src/schema.sql`, version 11:

| Table | Columns | Holds |
| --- | --- | --- |
| `repositories` | `id`, `state`, `error`, `updated_at`, `base_ref` | every repository the index heard of, at `idle`, `indexing` or `failed`, and the ref another repository is linked against |
| `refs` | `repository_id`, `git_ref`, `commit_sha`, `indexed_at` | the refs read per repository, each at the commit it was last read at |
| `files` | `repository_id`, `git_ref`, `path`, `blob`, `language` | the tracked files of a ref, each with the blob it held there |
| `blobs` | `blob`, `language`, `parsed_at` | every blob parsed so far |
| `symbols` | `id` (autoincrement), `blob`, `kind`, `name`, `qualified_name`, `start_line`, `end_line`, `signature`, `doc`, `is_test` | the definitions of one blob |
| `symbols_fts` | `terms`, rowid = `symbols.id` | FTS5 over each symbol's identifier parts |
| `mentions` | `blob`, `kind`, `name`, `line`, `from_symbol` | the names one blob names, before they are resolved |
| `imports` | `blob`, `module`, `name`, `line` | the import statements of one blob |
| `interfaces` | `blob`, `kind`, `name`, `line`, `symbol`, `handlers`, `method` | the packages, routes and variables of one blob (rule 30), before they are linked |
| `edges` | `from_repository`, `git_ref`, `from_blob`, `kind`, `from_symbol`, `from_line`, `to_repository`, `to_ref`, `to_blob`, `to_symbol`, `to_line`, `name`, `confidence`, `step`, `candidates` | the relations the resolution and link passes derived, each end a blob at a ref of a repository, a line and a definition where there is one, each naming the step that answered and how many definitions matched there |

`refs` cascade from `repositories`, `files` from `refs`, and `symbols`,
`mentions`, `imports`, `interfaces` and `edges` from `blobs` and `symbols`.
Dropping a ref or a repository then drops the blobs no file holds, with
their symbols and FTS rows, in two statements: the FTS rows are found by
rowid, never by reading the table; everything keyed by the blob goes with
it.

`edges` is indexed in both directions, `(from_repository, git_ref, kind,
from_symbol, to_symbol, confidence)` and `(to_repository, to_ref, kind,
to_symbol, from_symbol, confidence)`, so counting the callers of a level
reads an index and no rows of the table, the listing then reading one row per
edge it answers with, for its step and its candidate count; the edges into a
definition are found under its own repository and ref whichever repository
they come from; and by
`(from_repository, git_ref, from_blob)`, which is what deriving a blob's edges
again deletes by. `files` is indexed by `(blob, repository_id, git_ref)`,
which is what reading an end's path back joins by. `symbols` is indexed by
`name`, which is what resolution and `symbol` ask by; `interfaces` by blob
and by `(kind, name)`.

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
  `::a_dart_directive_reads_its_module_names_and_line`,
  `::a_reference_belongs_to_the_definition_it_sits_in`).
- A name resolves at the nearest step that holds a definition, a name past the
  candidate cap resolves nowhere, and a module is matched by the path or the
  qualified name it names
  (`resolve.rs::a_name_resolves_at_the_nearest_step_that_holds_a_definition`,
  `::a_name_past_the_candidate_cap_is_left_unresolved`,
  `::a_module_is_matched_by_the_path_or_the_qualified_name_it_names`).
- A name 4 definitions share at step `repository` makes no edge, while a nearer
  step keeps the cap of 20, and 2 or 3 definitions there make one `heuristic`
  edge to each
  (`resolve.rs::a_name_many_definitions_share_makes_no_edge_at_the_repository_step`,
  `::two_or_three_definitions_at_the_repository_step_make_a_guess_at_each`).
- A call skips a same-directory YAML key and resolves to a Rust function in a
  subdirectory, exactly; the YAML key has no caller
  (`knowledge.rs::a_call_resolves_to_a_code_definition_and_never_to_an_outline_key`).
- A Rust file that calls a definition it brought in with `use` names it
  exactly, and the caller and the callee each list the other, while a second
  definition of the same name that nothing imports is called by nobody
  (`knowledge.rs::a_call_through_an_import_resolves_to_the_definition_it_named`);
  a Dart package import names its definition exactly, and its call resolves
  at the import step
  (`knowledge.rs::a_dart_import_names_a_definition_and_resolves_a_call_at_import`);
  a name two files define, imported by neither, is a guess, and both are
  listed (`::a_name_defined_twice_resolves_to_both_as_a_guess`).
- An edge names the step that resolved it: a same-file call reads `exact`,
  step `file`, 1 candidate, and a name two files define, imported by neither,
  reads `heuristic`, step `repository`, 2 candidates
  (`knowledge.rs::an_edge_names_the_step_that_resolved_it`).
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
- A TypeScript or TSX `export type A = …` is a definition of kind `type` and
  an `export enum B { … }` one of kind `class`, each with its line range and
  signature
  (`parser.rs::a_type_alias_and_an_enum_are_definitions_in_typescript_and_tsx`).
- `search` with kind `type` returns the alias, and a file that names it in an
  annotation has a `references` edge to it
  (`knowledge.rs::a_type_alias_is_searched_by_its_kind_and_referenced_from_an_annotation`).
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
- Each manifest names its package and its dependencies, a path dependency
  told from one by name
  (`interfaces.rs::every_manifest_names_its_package_and_its_dependencies`),
  and a manifest with no language of its own is read by its name
  (`languages.rs::a_path_is_read_by_its_extension`).
- An environment variable is set by a `.env` line, a compose or workflow
  entry and `set_var`, and read by each language's own call
  (`interfaces.rs::an_environment_variable_is_set_and_read_by_each_syntax`).
- A literal in a registration call is a route template with the handler the
  call names, a literal in a request call is a route use, a decorator
  registers the definition under it, and a literal with one segment is
  neither
  (`interfaces.rs::a_route_is_registered_by_a_router_and_used_by_a_request`).
- A route use with a wildcard segment matches its template and is marked
  `heuristic`, a full match is `exact`, and a different segment count is no
  match (`interfaces.rs::a_route_use_fits_a_template_segment_by_segment`).
- Each of the five methods a Dart client calls is a route use with its
  method, its path and its line, and an interpolation in the path — `$id`,
  `${user.id}`, `${team.id.toString()}` or `${user?.id}` — is one path
  parameter
  (`interfaces.rs::a_dart_call_of_each_method_is_a_route_use_with_its_method_and_line`,
  `::a_dart_interpolation_is_one_path_parameter`).
- A route use with no leading `/` fits the last segments of a template, as a
  guess (`interfaces.rs::a_route_use_below_a_prefix_fits_the_end_of_a_template`),
  and a Dart `put('users/$id/role')` reaches the Rust handler of
  `/api/users/{id}/role` at step `route`
  (`knowledge.rs::a_dart_call_reaches_the_rust_handler_of_the_route_below_the_prefix`).
- A dependency joins the package it names, exactly by path and as a guess by
  name; a route use joins the template it fits, at the handler the
  registration named where the ref defines it; a set variable joins every
  read of it
  (`resolve.rs::interface_edges_join_a_dependency_a_route_use_and_a_set_variable`,
  `::a_route_handler_skips_outline_definitions`).
- A foreign reference is a guess at every definition of its name in the
  other repository, and a name past the cap makes none; an outline definition
  makes no foreign reference
  (`resolve.rs::a_foreign_reference_is_a_guess_at_every_definition_of_its_name`,
  `::a_foreign_reference_skips_outline_definitions`).
- Two repositories that share a symbol name but have no manifest link make no
  `references` edge
  (`knowledge.rs::repositories_without_a_manifest_link_do_not_share_references`);
  a dependency in the direction opposite the reference keeps the guessed edge
  (`knowledge.rs::a_reverse_manifest_link_keeps_references_between_repositories`).
- With `api` (Rust: the package `api-types`, a type `Item`, an axum route
  `/v1/items/{id}` and a read of `API_TOKEN`) and `web` (TypeScript: a
  dependency on `api-types`, a `new Item()`, a request to `/v1/items/42`
  and a `.env` that sets `API_TOKEN`) registered, `interactions` for `web`
  lists one edge of each kind with its ends, its confidence and its step —
  the dependency, the reference and the wildcard route as guesses, by
  `name`, by `name` and by `route`, and the variable exact by `name` — `api`
  lists the same edges from its side, and removing the dependency from `web`
  and reading it again removes the `depends_on` and `references` edges but
  keeps the route and variable edges
  (`tests/it/knowledge.rs::interactions_between_two_repositories_are_listed_by_kind`).
- `interactions` is empty for two repositories that only share a symbol name
  and have no manifest, route or variable relation
  (`tests/it/knowledge.rs::interactions_are_empty_without_a_true_repository_relation`).
- `impact --diff` for a change to the route handler in `api` lists the `web`
  call site, under `web`, at step `route`
  (`tests/it/knowledge.rs::a_route_handler_change_reaches_the_call_site_in_the_other_repository`).
- `symbol Item --detail context` from `api` lists the `web` reference under
  `web` with confidence `heuristic` at step `name`
  (`tests/it/knowledge.rs::a_type_named_in_the_other_repository_lists_that_reference_as_a_guess`).
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
- A map ranks the files of a fixture and holds to its budget, and a map
  toward one path ranks that file's neighbours first
  (`knowledge.rs::a_repo_map_ranks_the_files_and_holds_to_its_budget`); the
  ranking puts what every file names first and reaches a caller as well as a
  callee (`map.rs::the_file_every_other_file_names_ranks_first`,
  `::a_map_toward_one_file_ranks_what_that_file_joins_to`), and the text
  stops at the budget on a whole line
  (`map.rs::the_text_stops_at_the_budget`).
- A map ranks over the symbol edges alone: a repository whose crates depend
  on each other by path does not rank a manifest over the file its code calls
  (`knowledge.rs::a_map_ranks_over_the_symbol_edges_and_not_the_manifests`).
- Registering a repository answers its map under the budget asked for
  (`tests/it/knowledge.rs::registering_a_repository_indexes_its_base_branch`).
- A file graph groups symbol edges by file pair and kind, counts them, marks
  a group exact only when every edge is exact, and removes self-edges
  (`knowledge.rs::a_file_graph_groups_symbol_edges_and_removes_self_edges`,
  `::a_file_graph_removes_blob_self_edges_before_paths_are_joined`).
- A limited file graph keeps the files with the largest degree and
  reports the full file count with `truncated`
  (`knowledge.rs::a_file_graph_limit_keeps_the_files_with_most_edges_and_marks_truncation`).
- The file graph route returns its contract for a known repository and 404
  for an unknown one
  (`tests/it/knowledge.rs::the_file_graph_route_returns_its_contract_and_rejects_an_unknown_repository`).
- Every endpoint is in the OpenAPI document
  (`tests/it/knowledge.rs::every_knowledge_endpoint_is_in_the_openapi_document`).
- A direct edge wins over a two-edge path, the depth stops the walk, and a
  path is answered hop by hop
  (`store.rs::a_path_is_found_between_two_definitions_and_none_past_the_depth`,
  `knowledge.rs::the_shortest_path_between_two_symbols_is_answered_hop_by_hop`).
- A path crosses a route into another repository
  (`tests/it/knowledge.rs::a_path_crosses_a_route_into_the_other_repository`).
- Every seat lists `search_code`, `outline`, `symbol`, `path`, `impact` and
  `repo_map`
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
- `symbol` groups its answer under a heading for each repository — its path
  — each definition and each list of the context, another repository's ends
  under a heading of that repository's own
  (`tools.rs::symbol_groups_its_answer_under_a_heading_for_each_repository`),
  and takes the task's repository by default
  (`::symbol_defaults_to_the_task_repository`).
- Each end of a `symbol` answer names the step that resolved it, and the
  candidate count where the step held several
  (`tools.rs::symbol_prints_the_step_and_the_candidate_count_of_each_end`).
- `impact` with no argument is the task's own diff for a reviewer, and the
  callers in another repository sit under that repository's path
  (`tools.rs::impact_reads_the_task_diff_for_a_reviewer_that_names_nothing`),
  and a refusal for any other seat
  (`::impact_needs_a_symbol_or_a_diff_from_a_seat_that_is_no_reviewer`).
- `path` answers one line per hop and names an empty answer with its depth
  (`tools.rs::path_answers_one_line_per_hop_and_says_when_none`).
- `repo_map` maps every repository of the goal on a share of the budget, each
  under its own heading, and maps one repository with the path to rank around
  (`tools.rs::repo_map_maps_every_repository_of_the_goal_on_a_share_of_the_budget`,
  `::repo_map_takes_one_repository_with_the_path_to_rank_around`).
- The CLI commands are classified and parse their flags
  (`cli/tests.rs::every_command_in_the_tree_is_classified`,
  `::knowledge_search_takes_its_filters`,
  `::knowledge_outline_takes_the_repository_and_the_path`,
  `::knowledge_symbol_takes_its_name_and_detail`,
  `::knowledge_path_takes_two_names_and_the_depth`,
  `::knowledge_impact_takes_a_symbol_or_a_diff_and_the_depth`,
  `::knowledge_interactions_takes_the_repository_and_the_ref`,
  `::knowledge_map_takes_the_path_and_the_budget`,
  `::knowledge_graph_takes_the_repository_ref_limit_and_json_flag`), and a search
  row, an impact row and an interaction row each lead with their location
  (`commands/knowledge.rs::a_search_row_leads_with_its_location_and_titles_the_symbol`,
  `::an_impact_row_leads_with_its_location_and_says_how_far_away_it_is`,
  `::an_interaction_row_leads_with_its_from_end_and_names_its_kind`), and an
  end of a `symbol` block names its step and its candidate count
  (`::an_end_row_names_the_step_that_resolved_it`). A graph summary counts
  its edges by kind and ranks files by degree
  (`commands/knowledge.rs::graph_summary_counts_edges_by_kind_and_ranks_files_by_degree`).
- The desktop knowledge screen keeps its pickers and its tab in the URL,
  renders a status card per repository in every state, posts a reindex and
  shows `indexing` at once, and refetches once `knowledge_indexed` arrives
  (`ui/src/features/knowledge/knowledge-screen.test.tsx::the pickers and the tab`,
  `::the Overview tab`).
- The Repositories graph has a node per repository named by its folder, one
  edge per pair and kind with its count and colour, dashed where heuristic
  only, filtered by kind and confidence
  (`ui/src/features/knowledge/repositories-graph.test.ts`), and a click on
  an edge lists its ends with their confidence and step
  (`ui/src/features/knowledge/knowledge-screen.test.tsx::the Repositories tab`).
- The Symbols graph gives each relation its own colour. It includes hidden
  counts, foreign markers, and dashed heuristic edges
  (`ui/src/features/knowledge/symbols-graph.test.ts`).
- The Symbols tab sends the selected repository, ref, kind, and path.
  It opens results and preserves `?symbol=` through reloads. Node clicks,
  Back, and Forward change the centre. The pane shows numbered source.
  Equal names offer a definition picker
  (`ui/src/features/knowledge/symbols-tab.test.tsx::the Symbols tab`).
- Find symbol opens the Symbols tab
  (`ui/src/features/command-palette/command-palette.test.tsx::opens symbol search from the palette`).
- The shared graph component builds the model it draws, hides what it is
  told to hide, and hands node and edge clicks back by key
  (`ui/src/features/knowledge/graph/knowledge-graph.test.tsx`).
- The force layout places every node before the first frame, gives finite
  places, gives the same places on a second run, holds what the edges join
  together, draws no node on top of another in the pixels the narrowest
  desktop draws them at, draws the nodes of a graph too crowded for it smaller
  rather than covered, leaves no timer running, and damps an edge of 2467
  counts to under 13 and one of 1 to 1
  (`ui/src/features/knowledge/graph/force-layout.test.ts`). The view is
  handed those places and nothing moves them after; it keeps the label of a
  hovered node and of its neighbours, and names an edge only while the
  pointer is on one of its ends
  (`ui/src/features/knowledge/graph/knowledge-graph.test.tsx::the view`).
- The Impact graph puts the changed definition in the first layer and each
  caller in the layer of its depth, dashes a heuristic call, and marks a
  stopped definition as a node that says the walk stopped
  (`ui/src/features/knowledge/impact-graph.test.ts`); the Path graph is the
  hops as a chain with each edge named by its kind
  (`ui/src/features/knowledge/path-graph.test.ts`); ELK places the layers
  left to right (`ui/src/features/knowledge/graph/layered-layout.test.ts`).
  On screen, both modes draw from the daemon's answer, say so where a path
  is empty, keep the mode and its inputs in the URL, and open the Symbols
  tab on a clicked node
  (`ui/src/features/knowledge/impact-tab.test.tsx`) — parity with
  `ariadne knowledge impact|path` (rules 23, 34).
- Each free-text filter of the screen suggests its values as they are typed:
  the symbol fields from `search`, with the kind and the path of each hit,
  and the path fields from the file graph, directories before files
  (`ui/src/features/knowledge/impact-tab.test.tsx::the Impact mode`,
  `ui/src/features/knowledge/symbols-tab.test.tsx::the Symbols tab`,
  `ui/src/features/knowledge/knowledge-screen.test.tsx::the Files tab`).
  Down then Enter takes the active row into the field, which applies as
  typed text does, and Escape closes the list and keeps the text
  (`ui/src/features/knowledge/impact-tab.test.tsx::the Impact mode`).
- The Files graph sizes, colours and weights its model, opens at depth-two
  directory level, expands one directory into files while summing its edges,
  filters by path, kind and unlinked files without a rebuild, and builds 5000
  files within a second (`ui/src/features/knowledge/files-graph.test.ts`); on
  screen, a directory click keeps `?open=` and Collapse clears it, a click on
  a file shows its outline and edges, and a truncated response shows its notice
  and raises the limit
  (`ui/src/features/knowledge/knowledge-screen.test.tsx::the Files tab`).
- `knowledge_indexed` and `knowledge_failed` invalidate a repository's
  knowledge status, every interactions list, every impact and path walk,
  and every file graph and outline under it, and leave another repository's
  caches alone
  (`ui/src/events/dispatch.test.ts::knowledge events (022)`).
- The command palette opens the knowledge screen on a repository
  (`ui/src/features/command-palette/command-palette.test.tsx::opens the
  knowledge screen on a repository from the palette`).

## Known gap

An alias keeps the original name, so a file that calls the definition by its
alias resolves nothing. Two files of the same text are one blob and so one
definition, listed once per path and carrying the edges of both. Dropping a
ref drops the edges into the symbols it alone held, and the blobs at other
refs that pointed at them are not resolved again until they or the names
they name move.

Another repository is read at its base ref only: a task branch of `web` is
linked against `main` of `api`, and a task branch of `api` is what `main` of
`web` is linked against, so a route added on an `api` branch reaches `web`
once it lands. A reference across repositories exists only where a manifest
dependency joins them in either direction; within that pair, the name alone
joins the reference and it is always a guess. A call across repositories is
a `references` edge, not a `calls` one, so `impact` reaches another repository
through a route and not through a name. A route registered at file scope with
no handler the ref defines points at nothing but its own line. A manifest or
a `.env` file is read by the first path its blob was seen at, like every blob's
language. The link pass derives a whole pair of refs at a time, which is one
scan of the interfaces and the unresolved names of each side per run.

A reviewer on a task staffed with several authors reads the task's first
branch by default and names another with `git_ref`. A blob is parsed as the
language of the first path it was seen at. The branch of a cancelled or failed
task, and of a task whose goal was deleted, keeps its rows until a reindex or
a restart drops what no longer resolves.

## Sources

`crates/ariadne-knowledge/`, `crates/ariadne-daemon/src/knowledge.rs`,
`crates/ariadne-daemon/src/http/knowledge.rs`,
`crates/ariadne-api/src/knowledge.rs`,
`crates/ariadne-cli/src/commands/knowledge.rs`,
`crates/ariadne-knowledge/src/map.rs`,
`crates/ariadne-cli/src/commands/mcp/tools.rs`, `docs/knowledge.md`,
`ui/src/features/knowledge/`.
