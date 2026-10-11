# scripts/AGENTS.md

Conventions for changing the installer. Commit-message and history rules live
in the root [`AGENTS.md`](../AGENTS.md).

## What is here

- `install.sh` / `uninstall.sh` — the installer and uninstaller: binaries, the
  daemon service, shell completions and the desktop app. What each installs,
  and its flags, are in its own `usage()`, which is what `--help` prints —
  read the file rather than this one for the current flag list. On Darwin, a
  `--build-from-source` run also signs what it builds with the
  `make-signing-cert.sh` identity, when one is in the login keychain.
- `make-signing-cert.sh` — creates the self-signed "Ariadne Code Signing"
  identity `install.sh` signs local macOS builds with and the release
  workflow signs release assets with: the same certificate, read on each
  side from the same three repository secrets, `APPLE_CERTIFICATE`,
  `APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY`. A second run
  re-exports a valid identity rather than replacing it; `--force` replaces
  it regardless. Prints the three `gh secret set` commands by default, or
  runs them with `--set-secrets`. Darwin only.
- `lib.sh` — the numbered-step output both scripts share: a plan built from
  the flags, then each step reporting ✓ / ↷ / ✗, with noisy subcommands
  captured to a log and shown only on failure. Sourced, never executed.
- `check-unused-rust` — lists every `pub` item of a library crate that no
  other file names; see [`crates/AGENTS.md`](../crates/AGENTS.md).
- `install-githooks.sh` — sets `core.hooksPath` to `.githooks` from the
  repository root, so the hooks under `.githooks/` run; a repeat run that
  finds the setting already there changes nothing. See the root
  [`AGENTS.md`](../AGENTS.md#checks).
- `tests/install-command.sh` — the stand-in for every external tool
  (`gh`, `git`, `tar`, `ldconfig`, `systemctl`, `security`, `codesign`, …) the
  installer boundary tests put on `PATH`, driven by the arguments it is
  called with.
- `tests/install-linux.sh` — a Linux installer scenario, run as `bash
  scripts/tests/install-linux.sh CASE [host|present|missing]` for one of its
  cases: `missing`, `source-missing`, `release`, `source`, `rejected`,
  `no-ui`. It builds a throwaway `$TEST_ROOT` (repo copy, fake `PATH`, fake
  `HOME`), runs the installer and the uninstaller against it, and never
  touches the caller's own `HOME`.
- `tests/install-sign-macos.sh` — a macOS installer scenario, run as `bash
  scripts/tests/install-sign-macos.sh CASE` for one of its cases: `present`
  or `missing` (an "Ariadne Code Signing" identity is or is not in the login
  keychain) prove `install.sh --build-from-source` signs `ariadne` and
  `ariadned` when the identity is there, and skips with a step note — never
  failing the install — when it is not; `desktop-present` or
  `desktop-missing` prove the identity reaches `npm run tauri build` as
  `APPLE_SIGNING_IDENTITY` the same way, stopping right after — ditto'ing
  the bundle to `/Applications` is a system path no sandboxed test may touch.
- `tests/make-signing-cert-command.sh` — the stand-in for `openssl`,
  `security` and `gh` `tests/make-signing-cert-scenarios.sh` puts on `PATH`:
  no real keychain, network or cryptography is used, and a PKCS#12 file is a
  one-line stand-in holding its own password.
- `tests/make-signing-cert-scenarios.sh` — a `make-signing-cert.sh` scenario,
  run as `bash scripts/tests/make-signing-cert-scenarios.sh CASE` for one of
  its cases: `create`, `idempotent`, `replace`, `repair-invalid` (a
  certificate present but not a valid identity is replaced even without
  `--force`), `export` (a quote in the password reaches the `.p12` and the
  printed `gh secret set` command unmangled), `secrets` (`--set-secrets`
  runs the three commands).
- `tests/githooks.sh` — a `.githooks/` scenario, run as `bash
  scripts/tests/githooks.sh CASE` for one of its cases: `install`
  (`install-githooks.sh` sets `core.hooksPath`, and a worktree reads the same
  value), `rust-commit`, `ui-commit`, `tauri-commit`, `docs-commit` (a staged
  file of each area runs that area's pre-commit commands, and no other
  area's), `bad-subject` (`commit-msg` refuses a subject that is not a
  Conventional Commit and accepts one that is), `rust-push`, `ui-push` (a
  pushed range touching one area runs that area's pre-push commands, and no
  other area's), `push-isolation` (a linked-worktree commit and push leave
  its HEAD, refs, index and files unchanged while a cargo stand-in uses Git
  in another repository), `all` (`--all` runs every area), `fail` (a
  stand-in that exits 1 fails the hook and prints the failed command).
- `tests/githooks-command.sh` — the stand-in for `cargo`, `npm`, `npx` and
  `shellcheck` the githooks boundary tests put on `PATH`, and for
  `scripts/check-unused-rust` in their repository copy: it logs its own
  invocation and exits 1 only when `TEST_FAIL` names it exactly. With
  `TEST_GIT_PROBE=1`, its cargo clippy and nextest calls use Git in a
  separate repository to prove the hooks clear Git routing variables.

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
shellcheck .githooks/*
```

`tests/install-linux.sh` is not in CI: run it by hand, on Linux, once for each
case above, since it exercises the systemd and GNOME paths `ci.yml` does not
cover. `tests/install-sign-macos.sh` and `tests/make-signing-cert-scenarios.sh`
are the same, on macOS, for each of their cases. `tests/githooks.sh` is the
same, for each of its cases, on any OS. `tests/install-command.sh`,
`tests/make-signing-cert-command.sh` and `tests/githooks-command.sh` need no
direct invocation; the scenario scripts put them on `PATH` for the tools they
stand in for.
