# Memory

A memory is one useful fact, kept so a later session does not have to
rediscover it. It is never added to a prompt: an agent, or you, reads it only
by asking for it.

## Two scopes

A memory belongs to one registered repository, or to none, which makes it
global — true of every repository. A read of a repository's memory answers
that repository's own entries and the global ones together, so a global fact
never needs repeating per repository.

## Who writes one

An agent session saves a memory for a repository of its own task or goal,
through the `save_memory` tool; it is refused a global one. Its skills call
`save_memory` only for what is worth keeping: a trap, a working command, or a
convention no file states — never a report of the task itself, a change
summary, or a plan, and never what the code, a spec or `AGENTS.md` already
says. A task saves two memories at most, and a near-duplicate of an existing
memory is refused.

You write with no such limit: `ariadne memory add` saves a fact in any scope,
repository or global, from the CLI. The desktop app does the same: a
memory page lists, searches, adds and deletes memories of both scopes.

## The commands

```sh
ariadne memory add "Run the parser fixture before a knowledge change" --repo <repo-id>
ariadne memory add "The daemon needs a real git checkout" --global
ariadne memory add "…" --repo <repo-id> --expires 2026-12-31T00:00:00Z

ariadne memory ls --repo <repo-id>      # that repository and the global facts
ariadne memory ls --global              # only the global facts
ariadne memory ls                       # every scope

ariadne memory search parser --repo <repo-id>
ariadne memory search parser --global
ariadne memory search parser

ariadne memory delete <memory-id>
```

`add` requires exactly one of `--repo` and `--global`: a memory belongs
somewhere, and nothing here guesses where. `--expires` takes an RFC 3339
time; left off, the fact never expires. `ls` and `search` take `--repo` or
`--global`, or neither for every scope, but never both. `delete` finds the
entry by its id alone.

`search` ranks its matches, best first. Where no word in the query matches
anything, it falls back to the newest entries of the scope instead, and the
CLI says so above the table; `search_memory` tells an agent the same in its
answer.

These are listings and mutations like every other command: `--format json`
for the daemon's own objects, `-q` for the first column or the created and
deleted ids, `-o wide` and `--columns` for the layout. The list shows a
`scope` column, `global` or the repository, beside the id, the text, its age,
when it expires and its source.

## The store

Memory lives in the daemon's own database, under `/v1/memories`. A memory
outlives its source session, task or goal; a repository memory is removed
when its repository is, and a global one stays.
