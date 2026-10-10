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
cover. `tests/install-sign-macos.sh` and `tests/make-signing-cert-scenarios.sh`
are the same, on macOS, for each of their cases. `tests/install-command.sh`
and `tests/make-signing-cert-command.sh` need no direct invocation; the
scenario scripts put them on `PATH` for the tools they stand in for.
