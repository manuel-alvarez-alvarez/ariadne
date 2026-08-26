# AGENTS.md

Instructions for coding agents working in this repository. Read them before
you commit.

## Where things are, and what checks them

The tree, area by area, is [`README.md`'s Workspace
layout](README.md#workspace-layout); `ui/` has its own
[`ui/README.md`](ui/README.md).

Before changing anything, read the surrounding code and match its style,
naming and tooling. Run `cargo test` and `cargo clippy --all-targets` for Rust
changes; in `ui/`, `npm test`, `npm run typecheck` and `npm run lint`.

## Commit messages

This is the one place the commit types are written down: `README.md` and
[`.github/RELEASING.md`](.github/RELEASING.md) point here rather than repeat
the list.

Write every commit subject as a [Conventional
Commit](https://www.conventionalcommits.org): `type(scope): subject`, with a
lowercase type and an imperative subject ("add", not "added"/"adds").

Allowed types, and what each one does to a release:

| Type | Effect while pre-1.0 |
| --- | --- |
| `feat` | minor bump (0.1.0 → 0.2.0) |
| `fix`, `perf`, `revert` | patch bump (0.1.0 → 0.1.1) |
| `docs`, `refactor`, `test`, `chore`, `style`, `build`, `ci` | hidden: they ride along in a release triggered by something else and never trigger one on their own |

Mark a breaking change with `!` before the colon (`feat!: …`) or a
`BREAKING CHANGE:` footer; while pre-1.0 that is still a minor bump.

The scope is optional but encouraged, and it is one of the repository's area
names: `ui`, `daemon`, `cli`, `store`, `api`, `core`, `mcp`, `prompts`,
`doctor`, `codex`, `opencode`, `install`, `scripts`.

Release notes and version bumps are generated from commit messages by
release-please, so write subjects a user can read in a changelog: say what
changed for them, not which file you edited. Anything that is not a
conventional commit is silently ignored — it neither appears in the notes nor
moves the version.

## History

Never create merge commits — history is linear. Land a task branch on its base
by squash or fast-forward, and give the commit that lands a conventional
subject of its own.

The full release loop is in [`.github/RELEASING.md`](.github/RELEASING.md).
