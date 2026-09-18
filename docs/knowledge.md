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

The languages, by extension:

| Language | Extensions | Definitions |
| --- | --- | --- |
| Rust | `rs` | functions, methods, structs, enums, unions, type aliases, traits, modules, macros |
| TypeScript, TSX | `ts`, `mts`, `cts`, `tsx` | functions, methods, classes, interfaces, modules, constants, `it(`/`test(` calls |
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
`interface`, `macro`, `constant`, `test`, `heading`, and more for the newer
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

## The tools agents get

Every seat has four tools, beside the memory tools:

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
- `impact` lists what a change reaches: the callers of a definition, by how
  many calls away they are. `symbol` names one definition, `diff` as
  `<base>..<head>` names every definition a diff changed, and a reviewer that
  passes neither reads the diff of its own task. `depth` walks further, 2 by
  default and 4 at most. A definition with more than 200 callers is not walked
  past, and the answer says so.

`search_code` and `outline` answer plain text, one line per result, in the
form `path:line kind name signature` (`path:start-end` for an outline).
`symbol` and `impact` group their answer under headings, one per repository
and one per definition. An answer is cut at 8 KiB, and its last line then
says how many results were left out and to narrow the query.

Nothing is added to a prompt: an agent calls the tools when it needs them.

## From the CLI

```sh
ariadne knowledge status ~/projects/api           # what is indexed, and when
ariadne knowledge search add_worktree --repository ~/projects/api
ariadne knowledge search Manager --kind class --path src/
ariadne knowledge search parse --ref feat-parser  # a task branch
ariadne knowledge outline ~/projects/api src/lib.rs
ariadne knowledge symbol add_worktree --detail context
ariadne knowledge impact --repository ~/projects/api --symbol add_worktree
ariadne knowledge impact --repository ~/projects/api --diff main..feat-parser
ariadne knowledge reindex ~/projects/api          # drop it and build it again
```

`status` prints the repository's state (`idle`, `indexing`, `failed` or
`disabled`), every indexed ref with its commit, how many files and symbols
the index holds for it, the files per language, and the error of a failed
run. `search` and `outline` are listings like every other: `--format json`
for the daemon's own objects, `-q` for the first column (`path:line`, or the
line range of an outline), `-o wide` and `--columns` for the layout.
`search` reads every repository unless `--repository` names one, at the base
branch unless `--ref` names another. `symbol` prints a block per definition,
with the lists `--detail context` asked for; `impact` prints one row per
caller, led by its location, with how far away it is, names every definition
it stopped at in a note, and answers the daemon's own objects under
`--format json`.

The same is on the API: `GET /v1/repositories/{id}/knowledge`, `POST
/v1/repositories/{id}/knowledge/reindex`, `GET /v1/knowledge/search`, `GET
/v1/knowledge/outline`, `GET /v1/knowledge/symbol` and `GET
/v1/knowledge/impact`, and the domain events `knowledge_indexed` and
`knowledge_failed` on the event stream (`ariadne events` prints them).

## Turning it off

```toml
knowledge_enabled = false   # in ~/.ariadne/config.toml
```

With it off, nothing is indexed, no session is offered a knowledge tool,
`ariadne knowledge status` says `disabled`, and a search is refused with a
line that names the key.

## The store

The index is one SQLite file, `knowledge.db`, beside the daemon's database.
It is disposable: a daemon whose schema changed deletes it and reads every
repository again, and `ariadne knowledge reindex` does the same for one
repository. Nothing else depends on it, so deleting it by hand while the
daemon is stopped costs one full index on the next start.
