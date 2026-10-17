# assets/AGENTS.md

Conventions for changing branding and the demo recordings. Commit-message and
history rules live in the root [`AGENTS.md`](../AGENTS.md).

## What is here

- `branding/` — `logo.svg` and `banner.svg`, hand-authored.
- `demo.gif` and `demo-ui.gif` — the two demos in the root `README.md`'s
  hero table, side by side: the CLI creating a goal, and the desktop app's
  own form doing the same thing. Both show the same goal, "Add rate limiting
  to the login endpoint", with the same two tasks, in the same small login
  HTTP service — one codebase and one plan, told through two interfaces.
- `seed-demo-repo.sh` — the login HTTP service both recordings seed into
  their scratch repository (a `package.json` and two source files), so the
  two GIFs show the same codebase: `bash assets/seed-demo-repo.sh <repo>`.
  The caller has already run `git init` there; this script writes the files
  and commits them.
- `seed-demo-goal.py` — adds the same two tasks to the goal each recording
  creates through its own user-facing path (the CLI types `ariadne goal
  create`; the UI fills the New goal form), then finalizes the plan the way
  a real orchestrator would once the user agreed to it, over the daemon's
  REST API: `python3 assets/seed-demo-goal.py <base-url> <model>`. It finds
  the goal by its title, so it can be started before that goal exists —
  `demo.tape` relies on exactly that to keep it off the recording (see
  below). No `jq` or other non-stdlib dependency: this repo's own tooling
  already assumes `python3` (`crates/AGENTS.md`), never a bare `jq`.
- `demo.tape`, rendered with [vhs](https://github.com/charmbracelet/vhs), and
  [`record-demo-cli.sh`](record-demo-cli.sh), the script that owns it:
  `bash assets/record-demo-cli.sh`. Never call `vhs` on the tape directly —
  `demo.tape` only types commands, it does not set up or tear down what they
  need. The script builds the release binary, creates a throwaway daemon on
  its own `ARIADNE_HOME` (`/tmp/ariadne-demo-home`, never the real
  `~/.ariadne`) with a loopback `tcp_listen` port, seeds its scratch
  repository with `seed-demo-repo.sh`, runs `vhs` over the tape, and removes
  the daemon and the scratch directories when the recording ends or the
  script is interrupted — the same lifecycle `record-demo-ui.mjs` owns for
  its own recording, below. The tape itself starts `seed-demo-goal.py` in the
  background, redirected and before anything is on screen: a `Hide`/`Show`
  pair only skips *capturing* frames, it does not clear the terminal's own
  scrollback, so a command typed and run between them would still show once
  `Show` resumes.
- `record-demo-ui.mjs`: `node assets/record-demo-ui.mjs`. It builds the
  release binaries, creates an isolated Ariadne home with a two-second inert
  `cli_bin` (the scheduler can create metadata rows for the demo, but it
  cannot start a real agent process), seeds its own scratch repository with
  `seed-demo-repo.sh`, fills the New goal form in the UI with the same goal
  `demo.tape` types, then runs `seed-demo-goal.py` the same way to add the
  same tasks and finalize. It drives the UI dev server with Playwright, and
  removes the daemon, the dev server and the scratch directory itself when
  the recording ends or the script is interrupted.

## Rules

- Never edit a GIF by hand; it is rendered output. Change the tape, the
  recording script, or the two shared seeding scripts, then regenerate both
  — a title or task changed in one and not the other makes the two demos
  disagree.
- Regenerate the GIF whose recording shows a change you just made to the CLI
  or the UI it drives — a stale GIF documents a screen that no longer exists.
- Never record against the real `~/.ariadne`: both recordings build their own
  throwaway `ARIADNE_HOME` and tear it down after, so a recording run never
  touches, and never depends on, your own daemon's data.
