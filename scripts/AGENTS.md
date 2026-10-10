# scripts/AGENTS.md

Conventions for changing the installer. Commit-message and history rules live
in the root [`AGENTS.md`](../AGENTS.md).

## What is here

- `install.sh` / `uninstall.sh` — the installer and uninstaller: binaries, the
  daemon service, shell completions and the desktop app. What each installs,
  and its flags, are in its own `usage()`, which is what `--help` prints —
  read the file rather than this one for the current flag list.
- `lib.sh` — the numbered-step output both scripts share: a plan built from
  the flags, then each step reporting ✓ / ↷ / ✗, with noisy subcommands
  captured to a log and shown only on failure. Sourced, never executed.
- `check-unused-rust` — lists every `pub` item of a library crate that no
  other file names; see [`crates/AGENTS.md`](../crates/AGENTS.md).
- `tests/install-command.sh` — the stand-in for every external tool
  (`gh`, `git`, `tar`, `ldconfig`, `systemctl`, …) the installer boundary
  tests put on `PATH`, driven by the arguments it is called with.
- `tests/install-linux.sh` — a Linux installer scenario, run as `bash
  scripts/tests/install-linux.sh CASE [host|present|missing]` for one of its
  cases: `missing`, `source-missing`, `release`, `source`, `rejected`,
  `no-ui`. It builds a throwaway `$TEST_ROOT` (repo copy, fake `PATH`, fake
  `HOME`), runs the installer and the uninstaller against it, and never
  touches the caller's own `HOME`.

## Conventions

- Bash 3.2 (macOS's shipped bash), not 4+: no associative arrays, no
  `readarray`, nothing newer the two scripts and `lib.sh` do not already use.
- Add a step to `install.sh` or `uninstall.sh` in two places, in the same
  order: a `plan_add "title"` where the plan is built, and a `step_begin` /
  `step_ok` pair where the work happens. See `lib.sh`'s own header.
- What a run installed is recorded in `~/.ariadne/install.env`, which
  `uninstall.sh` reads to find what to remove; a step that installs
  something records it there.
- A `pub` item that `check-unused-rust` cannot otherwise see reached (a
  derive, serde, utoipa, or a public signature) goes in its `IGNORED`, with
  the reason, rather than being left to fail the check.

## Checks

```sh
shellcheck scripts/*.sh
```

`tests/install-linux.sh` is not in CI: run it by hand, on Linux, once for each
case above, since it exercises the systemd and GNOME paths `ci.yml` does not
cover. `tests/install-command.sh` needs no direct invocation; the scenario
script puts it on `PATH` for the tools it stands in for.
