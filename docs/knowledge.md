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
| Markdown | `md`, `markdown` | headings, each spanning to the next heading of its level |

Each definition carries its kind (`function`, `method`, `class`, `module`,
`interface`, `macro`, `constant`, `test`, `heading`), its qualified name
(`GitManager::add_worktree`), its first and last line, its signature, its
doc comment, and whether it is a test: `#[test]` in Rust, a `test_` name in
Python, an `it(`/`test(` call in TypeScript and JavaScript, `[Fact]`/`[Test]`
in C#.

## The tools agents get

Every seat has two tools, beside the memory tools:

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

Both answer plain text, one line per result, in the form `path:line kind name
signature` (`path:start-end` for an outline). An answer is cut at 8 KiB, and
its last line then says how many results were left out and to narrow the
query.

Nothing is added to a prompt: an agent calls the tools when it needs them.

## From the CLI

```sh
ariadne knowledge status ~/projects/api           # what is indexed, and when
ariadne knowledge search add_worktree --repository ~/projects/api
ariadne knowledge search Manager --kind class --path src/
ariadne knowledge search parse --ref feat-parser  # a task branch
ariadne knowledge outline ~/projects/api src/lib.rs
ariadne knowledge reindex ~/projects/api          # drop it and build it again
```

`status` prints the repository's state (`idle`, `indexing`, `failed` or
`disabled`), every indexed ref with its commit, how many files and symbols
the index holds for it, the files per language, and the error of a failed
run. `search` and `outline` are listings like every other: `--format json`
for the daemon's own objects, `-q` for the first column (`path:line`, or the
line range of an outline), `-o wide` and `--columns` for the layout.
`search` reads every repository unless `--repository` names one, at the base
branch unless `--ref` names another.

The same is on the API: `GET /v1/repositories/{id}/knowledge`, `POST
/v1/repositories/{id}/knowledge/reindex`, `GET /v1/knowledge/search` and
`GET /v1/knowledge/outline`, and the domain events `knowledge_indexed` and
`knowledge_failed` on the event stream (`ariadne events` prints them).

## Turning it off

```toml
knowledge_enabled = false   # in ~/.ariadne/config.toml
```

With it off, nothing is indexed, no session is offered `search_code` or
`outline`, `ariadne knowledge status` says `disabled`, and a search is
refused with a line that names the key.

## The store

The index is one SQLite file, `knowledge.db`, beside the daemon's database.
It is disposable: a daemon whose schema changed deletes it and reads every
repository again, and `ariadne knowledge reindex` does the same for one
repository. Nothing else depends on it, so deleting it by hand while the
daemon is stopped costs one full index on the next start.
