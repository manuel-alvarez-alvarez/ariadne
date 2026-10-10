# assets/AGENTS.md

Conventions for changing branding and the demo recordings. Commit-message and
history rules live in the root [`AGENTS.md`](../AGENTS.md).

## What is here

- `branding/` — `logo.svg` and `banner.svg`, hand-authored.
- `demo.gif` — the terminal demo in the root `README.md`, rendered from
  [`demo.tape`](demo.tape) with [vhs](https://github.com/charmbracelet/vhs):
  `vhs assets/demo.tape`. Read the file's own header comment for what it
  needs first — a release build of `ariadne` on `PATH`, and a throwaway
  daemon on its own `ARIADNE_HOME` (`/tmp/ariadne-demo-home`), started and
  stopped by the pid `daemon start` prints, never by pattern.
- `demo-ui.gif` — the desktop app demo in the root `README.md`, rendered by
  [`record-demo-ui.mjs`](record-demo-ui.mjs): `node assets/record-demo-ui.mjs`.
  It builds the release binaries, seeds a goal and a task over the daemon's
  REST API in a `mkdtemp` scratch directory (its own `ARIADNE_HOME`, its own
  repository, its own free TCP port), drives the UI dev server with
  Playwright, and removes the daemon, the dev server and the scratch
  directory itself when the recording ends or the script is interrupted.

## Rules

- Never edit a GIF by hand; it is rendered output. Change the tape or the
  recording script, then regenerate.
- Regenerate the GIF whose recording shows a change you just made to the CLI
  or the UI it drives — a stale GIF documents a screen that no longer exists.
- Never record against the real `~/.ariadne`: both recordings build their own
  throwaway `ARIADNE_HOME` and tear it down after, so a recording run never
  touches, and never depends on, your own daemon's data.
