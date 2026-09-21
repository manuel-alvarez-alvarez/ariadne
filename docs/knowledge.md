# The knowledge base

The daemon keeps a symbol index over every registered repository: each
definition in the code, by name, with its line range. Agents ask it where a
function is instead of grepping and reading a file in slices, and you can ask
it the same from the CLI.

## What is indexed

Every registered repository is read at its base branch when the daemon
starts, when the repository is registered, and after every landing. A task
branch is read every time its head moves, so an author searching its own
branch finds what it committed a moment ago, and it leaves the index when
the task lands.

Only what git tracks is read, from the commit itself, never the working tree.
A file over 1 MiB and a binary file are skipped. A file's content is parsed
once: an unchanged file is shared between the base branch, every task branch
and every repository that holds it, so a commit that changes one file parses
one file.

A run parses several files at a time: half of the cores by default, and
`knowledge_workers` in `config.toml` sets another number. A file the parser
cannot get through is skipped, with a warning in the daemon's log that names
it, and the run goes on with the rest.

The languages, by extension:

| Language | Extensions | Definitions |
| --- | --- | --- |
| Rust | `rs` | functions, methods, structs, enums, unions, type aliases, traits, modules, macros |
| TypeScript, TSX | `ts`, `mts`, `cts`, `tsx` | functions, methods, classes, enums, interfaces, type aliases, modules, constants, `it(`/`test(` calls |
| JavaScript, JSX | `js`, `mjs`, `cjs`, `jsx` | the same |
| C# | `cs` | classes, interfaces, methods, namespaces |
| Python | `py`, `pyi` | functions, classes, module-level constants |
| Go | `go` | functions, methods, types |
| Java | `java` | classes, interfaces, methods |
| C | `c`, `h` | functions, structs, unions, enums, type aliases |
| C++ | `cpp`, `cc`, `cxx`, `hpp`, `hh`, `hxx` | the same, plus classes and methods |
| Ruby | `rb` | methods, classes, modules, `describe`/`it` blocks |
| PHP | `php` | functions, methods, classes, interfaces, traits, namespaces |
| Kotlin | `kt`, `kts` | classes, objects, functions |
| Swift | `swift` | classes, protocols, functions (including methods), properties |
| Dart | `dart` | classes, mixins, enums, functions, methods, constructors, `test(` calls |
| Scala | `scala`, `sc` | classes, objects, traits, enums, functions, `test(` calls |
| Bash | `sh`, `bash`, `bats` | functions, variables, a bats `@test` block |
| Lua | `lua` | functions and methods |
| Elixir | `ex`, `exs` | modules, functions and macros, `test "…" do` blocks |
| Markdown | `md`, `markdown` | headings, each spanning to the next heading of its level |
| YAML | `yaml`, `yml` | top-level keys, and keys nested one level under a mapping |
| TOML | `toml` | top-level keys, and each `[table]`'s own keys |
| JSON | `json` | top-level keys, and keys nested one level under an object |
| HTML | `html`, `htm` | elements that carry an `id` |
| CSS | `css` | each rule set's selector |
| SQL | `sql` | the object name of a `CREATE` or `ALTER` statement |

The last 7 are outline-only: they have no tags query, so their own
structure — headings, keys, elements, selectors, statements — stands in for
definitions, with no doc comment and no test marker.

Each definition carries its kind (`function`, `method`, `class`, `module`,
`interface`, `type`, `macro`, `constant`, `test`, `heading`, and more for the newer
languages: `object`, `key`, `table`, `element`, `selector`), its qualified
name (`GitManager::add_worktree`), its first and last line, its signature,
its doc comment, and whether it is a test: an attribute or annotation
(`#[test]` in Rust, `[Fact]`/`[Test]` in C#, `@Test` in Java and Kotlin), a
name (`test_` in Python, a `TestX` name in Go, a `test` name in PHPUnit, a
name starting with `test` in Swift — read on any class, not only an
`XCTestCase` subclass), or a call (`it(`/`test(` in TypeScript and
JavaScript, `describe`/`it` in Ruby, `test(` in Dart and Scala, `test "…" do`
in Elixir, a bats `@test` block in Bash). C, C++ and Lua have no common test
marker.

## The graph over the definitions

Every name a file names is resolved to the definition behind it, and the pair
becomes an edge: `calls`, `references`, `implements`, `extends` or `imports`.
The definition is looked for in four places, nearest first: the same file, the
same directory or module, the modules the file imports (`use` in Rust,
`import` and `from … import` in Python, `import` and `require` in TypeScript
and JavaScript, `using` in C#; every other language names none), and then
anywhere in the repository. One
definition there is an `exact` edge; several make a `heuristic` edge to each
of them, and the answer says how many matched. A name that matches more than
20 definitions says nothing about which one was meant and is left alone.

The tests of a definition are the test definitions at most two calls away, so
a test that calls a helper that calls the definition is one of its tests.

## Interactions between repositories

The store is one for every registered repository, so a change in one shows
its effect in another. Beyond its symbols, every file is read for what it
offers other repositories and takes from them:

- **Packages.** A manifest — `Cargo.toml`, `package.json`, `pubspec.yaml`,
  `go.mod`, `pom.xml`, `build.gradle` — names the package it defines and the
  packages it depends on. A dependency that names a package another
  repository defines is a `depends_on` edge: exact for a path dependency,
  a guess for one by name.
- **Routes.** A string literal like `/v1/items/{id}` in a call that registers
  a route (`.route(…)` in axum, `app.get(…)` in Express, `HandleFunc` in Go,
  `@app.route` in Flask) is a route template; the same literal in a request
  (`fetch(…)`, `axios.get(…)`, `client.get(…)`) is a route use. A use that
  fits a template is a `calls_route` edge, pointing at the handler the
  registration names: exact where every segment matches, a guess where
  `{id}`, `:id` or `${id}` stood in for one.
- **Environment variables.** A variable set in a `.env` file, a compose file,
  a workflow or `env::set_var`, and read elsewhere by the language's own call
  (`std::env::var`, `process.env.X`, `os.environ[…]`,
  `Environment.GetEnvironmentVariable`, `Platform.environment[…]`), is a
  `sets_env` edge, exact.
- **Names.** A name a repository uses but defines nowhere, and another
  repository defines, is a `references` edge, always a guess. Where the
  manifest says which repository this one depends on, that repository's
  definition wins.

Another repository is read at its base branch, so a route added on a task
branch reaches its callers once it lands. `symbol --detail context` and
`impact` list the other repositories' hits under a heading of their own, and
`impact --diff` on a change to a route handler names the call sites in the
front end that requests it.

```sh
ariadne knowledge interactions ~/projects/web    # what joins web to the others
ariadne knowledge interactions ~/projects/web --format json
```

`interactions` lists every edge between the repository and the others, in
either direction, one row per edge: where it starts, its kind, the two ends
and how sure the match is. `--format json` prints the daemon's own groups, one
per kind.

## The tools agents get

Every seat has six tools:

- `search_code` finds definitions by name. Words, camelCase parts and
  snake_case parts all match, each as a prefix: `add_worktree`, `addWork` and
  `worktree` all find `add_worktree`. It searches the repositories of the
  session's goal, on the seat's own branch: an author's task branch, a
  reviewer's branch under review, the orchestrator's base branch. `repository`
  narrows it to one repository, `all` widens it to every registered one, and
  `kind`, `path` and `git_ref` narrow it further.
- `outline` lists the definitions of one file with their line ranges, so an
  agent reads a function by its range instead of the whole file. It takes the
  task's repository by default, then the goal's only repository, and asks for
  one where the goal has several.
- `symbol` reads one definition by name, in the task's repository: where it
  is, its signature and its doc. `detail=source` adds its text, and `detail=context` adds its callers,
  its callees, what implements it and the tests that reach it, 20 of each at
  most.
- `path` finds the shortest directed path between two symbols, six edges deep
  by default and ten at most. It follows calls, routes, references,
  implementations, extensions and imports, including routes into another
  repository.
- `impact` lists what a change reaches: the callers of a definition, by how
  many calls away they are. `symbol` names one definition, `diff` as
  `<base>..<head>` names every definition a diff changed, and a reviewer that
  passes neither reads the diff of its own task. `depth` walks further, 2 by
  default and 4 at most. A definition with more than 200 callers is not walked
  past, and the answer says so.
- `repo_map` reads a whole repository at once: the files that carry it,
  ranked, each with the definitions most of the repository points at. It is
  what an agent calls to learn a repository it has not seen. With no
  `repository` it maps every repository of the goal, each under a heading of
  its own. `path` ranks the neighbours of one file first, and `budget` says
  how long the map runs — 1000 tokens by default, 4000 at most.

The ranking is a PageRank over the references between the files, the same
idea as Aider's repo map: a file that much of the repository names collects
the rank of everything that names it, and a file nothing names keeps its
own. With `path` the walk restarts at that file instead, so its callers and
its callees come out on top. Each file's own definitions are the ones most
of the repository points at, ten a file at most, in line order, and the text
stops at the last whole line the budget holds.

`search_code` and `outline` answer plain text, one line per result, in the
form `path:line kind name signature` (`path:start-end` for an outline).
`symbol`, `path` and `impact` group their answer under headings, one per
repository (its path) and one per definition, with what another repository
contributes under that repository's own heading. A path prints one line per
hop as `path:line kind name <- edge_kind confidence`; its first hop has no
edge. An answer is cut at 8 KiB, and its last line then says how many results
were left out and to narrow the query.

Nothing is added to a prompt but a few lines. The session rules tell every
agent to find code with `search_code` and `symbol` before it reads a file.
They also tell it to load the tools it needs in one tool search before the
first call: Claude Code lists an MCP tool by name only until the agent loads
it. The skills name the tool of the step that needs it. `coding` tells the
agent not to search with the shell for what `search_code` found, and
`code-review` calls `impact`, `symbol` and `path` only for a question the
diff leaves open.

## From the CLI

```sh
ariadne knowledge status ~/projects/api           # what is indexed, and when
ariadne knowledge search add_worktree --repository ~/projects/api
ariadne knowledge search Manager --kind class --path src/
ariadne knowledge search parse --ref feat-parser  # a task branch
ariadne knowledge outline ~/projects/api src/lib.rs
ariadne knowledge symbol add_worktree --detail context
ariadne knowledge path add_worktree remove_worktree --repository ~/projects/api
ariadne knowledge impact --repository ~/projects/api --symbol add_worktree
ariadne knowledge impact --repository ~/projects/api --diff main..feat-parser
ariadne knowledge interactions ~/projects/web     # what joins web to the others
ariadne knowledge map ~/projects/api              # the files that carry it
ariadne knowledge map ~/projects/api --path src/lib.rs --budget 2000
ariadne knowledge reindex ~/projects/api          # drop it and build it again
```

`status` prints the repository's state (`idle`, `indexing`, `failed` or
`disabled`), every indexed ref with its commit, how many files and symbols
the index holds for it, the files per language, and each ref whose last run
failed, with its error. A run is of one ref, and so is its failure: a good
run of a task branch leaves a failed base branch failed, and named. `search` and `outline` are listings like every other: `--format json`
for the daemon's own objects, `-q` for the first column (`path:line`, or the
line range of an outline), `-o wide` and `--columns` for the layout.
`search` reads every repository unless `--repository` names one, at the base
branch unless `--ref` names another. `symbol` prints a block per definition,
with the lists `--detail context` asked for. `path` prints one row per hop,
with `--depth` six by default and ten at most; `-q` prints `path:line` and
`--format json` prints the daemon's path object. `impact` prints one row per
caller, led by its location, with how far away it is, names every definition
it stopped at in a note, and answers the daemon's own objects under `--format
json`. `map` prints the one text the daemon rendered, as it came, and
`--format json` adds the ref, the token count and how many files it names.

The same is on the API: `GET /v1/repositories/{id}/knowledge`, `POST
/v1/repositories/{id}/knowledge/reindex`, `GET /v1/knowledge/search`, `GET
/v1/knowledge/outline`, `GET /v1/knowledge/symbol`, `GET
/v1/knowledge/path`, `GET /v1/knowledge/impact`, `GET
/v1/knowledge/interactions` and `GET /v1/knowledge/map`, and the domain events
`knowledge_indexed` and `knowledge_failed` on the event stream (`ariadne
events` prints them). Each names the ref the run was of.

## When an index is not ready

A read of a ref that cannot answer is refused. An empty answer would read as
a fact about the code, so the daemon says which case it is, and names the
repository and the ref:

```
the index of main in repository /home/me/projects/api (01K…) is not ready: <reason>
```

- `no index run of this ref has started`: the ref was never indexed, or its
  run is still queued. A ref you named with `--ref` or `git_ref` that is no
  indexed branch reads like this too.
- `the first index run of this ref is in progress, try again later`: this
  holds for every ref of a repository during a reindex, which drops its rows
  first.
- `the last index run of this ref failed: <error>`: fix the cause, then run
  `ariadne knowledge reindex`.

A ref that has an index answers while a run updates it. The CLI prints the
sentence as it came, an agent reads it as the text of its tool, and the API
answers 409 with the code `knowledge_not_ready`. A search over several
repositories is refused where one of them is not ready: name a repository
to read the others. A store or a git that fails is a 500 with the code
`internal_error`, never a 4xx: the request was right.

A task branch that git can no longer resolve leaves the index with no
failure. Every other error of a run is recorded on its ref. When the index
falls behind the daemon's event stream, it reads every base branch and every
in-flight task branch again, so no branch move is lost.

In the desktop app, **Knowledge** in the sidebar opens the same data. Pick a
repository and a ref at the top. The **Overview** tab shows each
repository's status, each ref that failed beside its error, and a Reindex
button. The **Repositories** tab draws
how the repositories use each other: one node per repository, one edge per
pair and kind, with the count on it. A dashed edge is a guess only. Click an
edge to list the files at its ends. The **Impact & path** tab draws what a
change to a symbol reaches: name a symbol, pick a depth from 1 to 4, and the
callers show in layers left to right, each in the layer of its depth. A dashed
edge is a guess, and a definition marked as stopped has more than 200 callers,
so the walk went no further. Switch the mode to **Path**, name two symbols and
a depth up to 10, and the shortest path shows as a chain, each edge named by
its kind. Click a symbol to open it on the Symbols tab. The **Files** tab opens
with directories grouped at two path segments. Click one directory to expand
its files while the other directories stay grouped, and use **Collapse** to
close it. The URL keeps the expansion, and `?level=file` shows every file.
The tab shows the displayed and total node and edge counts.

## Turning it off

```toml
knowledge_enabled = false   # in ~/.ariadne/config.toml
```

With it off, nothing is indexed, no session is offered a knowledge tool,
`ariadne knowledge status` says `disabled`, and a search is refused with a
line that names the key. No text an agent reads names a knowledge tool
either: the session rules drop the line about `search_code`, and each skill
is written without its knowledge steps.

## The store

The index is one SQLite file, `knowledge.db`, beside the daemon's database.
It is disposable: a daemon whose schema changed deletes it and reads every
repository again, and `ariadne knowledge reindex` does the same for one
repository. Nothing else depends on it, so deleting it by hand while the
daemon is stopped costs one full index on the next start.
