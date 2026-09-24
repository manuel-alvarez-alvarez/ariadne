# AGENTS.md

Ariadne is a docker-style orchestrator for AI coding agents: a daemon
(`ariadned`) that breaks goals into tasks and runs orchestrator, author and
reviewer agents on them until each one is finished, a CLI (`ariadne`) that
drives it, and a desktop app. What it is is [`README.md`](README.md), and how
it is used is the manual under [`docs/`](docs/README.md).

The conventions for changing it are split by area. This file holds what applies
everywhere; each area keeps its own, and names the checks to run there.

## Where the conventions are

Read the file for the area before changing anything under it. Only some agent
CLIs load a nested `AGENTS.md` by themselves, so a file that is not named here
is a file an agent may never see.

- [`crates/AGENTS.md`](crates/AGENTS.md) — the Rust workspace: what each crate
  holds, and the cargo commands that test and lint it.
- [`ui/AGENTS.md`](ui/AGENTS.md) — Ariadne Desktop under `ui/`: its layout, how
  it calls the daemon, query keys, the event stream, routes, keyboard chords,
  the shadcn setup, and the npm commands that check it.
- [`.github/RELEASING.md`](.github/RELEASING.md) — the release loop: how
  release-please turns commits into versions, tags and release notes.
- [`docs/`](docs/README.md) — the user-facing manual: installing, configuring
  and running Ariadne, and how a goal becomes a landed change. Every page a
  user reads lives here, and [`README.md`](README.md) is the front page over
  it: the pitch, the demos, a quick start, the top-level tree and the links.
- [`specs/`](specs/README.md) — what each subsystem does, as it stands, with
  every acceptance criterion tied to the test that proves it. Read the spec of
  the area you are changing, and change it in the same commit as the code.

Before changing anything, read the surrounding code and match its style,
naming and tooling.

## Processes you leave behind

Every agent and every check shares one machine. A process you start in the
background outlives your shell: a session can end, time out or be killed at
any line, so a `kill` at the end of the script may never run. Whatever it
started then keeps going, and every later task pays for it. Leftover busy
loops once held the load at 200 on 16 cores for three days, and every
`cargo` and `vitest` run crawled.

- To prove a flaky test holds under load, make each busy loop end with the
  shell that started it: `(while kill -0 $$ 2>/dev/null; do :; done) &`,
  never `(while :; do :; done) &`. `$$` is the starting shell even inside
  the subshell, so the loop stops within a moment when that shell dies,
  even by `kill -9`. Kill the loops yourself when the proof is done, too.
- To wait for a process, wait on its pid (`while kill -0 $pid; do sleep 5;
  done`), not on `pgrep -f '<pattern>'`. The pattern is in the waiting
  shell's own command line, so `pgrep -f` finds the waiter itself and the
  wait never ends.

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
