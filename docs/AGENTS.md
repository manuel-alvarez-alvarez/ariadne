# docs/AGENTS.md

Conventions for changing the manual. Commit-message and history rules live in
the root [`AGENTS.md`](../AGENTS.md).

## What is here

The user-facing manual lives under `docs/`: installing, configuring and
running Ariadne, and how a goal becomes a landed change. Every page a user
reads lives here; nothing outside `docs/` is a page of the manual.
[`README.md`](README.md) indexes every page.

## Conventions

- A new page adds a row to both [`README.md`](README.md)'s table and the
  root [`README.md`](../README.md)'s Documentation table — the two must list
  the same pages.
- A rule belongs in a spec, with the test that proves it; a page here
  explains the rule for a reader and links the spec rather than restating its
  acceptance criteria.
- Update the page in the same commit as the flag, key or command it
  documents. A page written later describes what the author remembers, not
  what landed.
- Commits that change only a page use the `docs` type.

## Checks

- Every relative markdown link under `docs/` resolves.
- A CLI example on a page matches what `ariadne --help` (or the relevant
  subcommand's `--help`) actually prints.
