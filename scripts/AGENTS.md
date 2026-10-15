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
- `check-migrations.sh` — guards `crates/ariadne-store/migrations/` against
  an edit, a deletion or a type change (such as a symlink swap) of a
  migration already on `main`, and checks that a new one is a correctly
  numbered regular file; see [`crates/AGENTS.md`](../crates/AGENTS.md).
- `install-githooks.sh` — sets `core.hooksPath` to `.githooks` from the
  repository root, so the hooks under `.githooks/` run; a repeat run that
  finds the setting already there changes nothing. See the root
  [`AGENTS.md`](../AGENTS.md#checks).
- `release-notes.sh` — run as `scripts/release-notes.sh TAG BODY_FILE
  CHECKSUMS_FILE...` by `release.yml`'s `notes` job, after every target has
  built. Prints, to stdout, an install section and an asset table (one row
  per line of every checksums file: a download link, the target and the
  SHA-256 digest) and a compare link to the previous tag when one is known
  from `BODY_FILE` or from `git`, wrapped in a pair of HTML comment markers,
  followed by `BODY_FILE`'s own release-please notes unchanged. Idempotent:
  a re-run on its own output replaces only what is between the markers,
  never stacking a second header. Reads `BODY_FILE` as `\r\n` or `\n`, and
  refuses a body that holds one marker but not the other rather than
  guessing at it. owner/repo for the asset links comes from the `origin`
  remote, the same way `install.sh` resolves a release repository
  (including its check that the parsed owner and name are non-empty),
  falling back to `GITHUB_REPOSITORY`.
- `tests/release-please-config.sh` — run as `bash
  scripts/tests/release-please-config.sh`; it builds notes from representative
  `perf:` and `ci:` commits, proving the first reaches ⚡ Performance and the
  second remains hidden.
- `tests/install-command.sh` — the stand-in for every external tool
  (`gh`, `git`, `tar`, `ldconfig`, `systemctl`, `security`, `codesign`,
  `launchctl`, …) the installer boundary tests put on `PATH`, driven by the
  arguments it is called with. Its `launchctl print` keeps the service label
  present for `TEST_PRINT_POLLS` calls (default 2) before reporting it gone,
  and its `launchctl bootstrap` fails the first call with the real
  "Bootstrap failed: 5: Input/output error" before succeeding — a bootout
  that returned before launchd actually dropped the job.
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
- `tests/install-service-macos.sh` — a macOS installer scenario, run as
  `bash scripts/tests/install-service-macos.sh`. The fake `launchctl` above
  keeps the old daemon's label present for a couple of polls after
  `bootout` and fails the first `bootstrap`, proving `install.sh` waits
  `ui_stop_daemon`'s launchd poll out, retries the bootstrap, and still ends
  with the service registered rather than calling `ui_die`.
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
  area's), `release-please-config-commit` (a changed config runs the release
  section test), `bad-subject` (`commit-msg` refuses a subject that is not a
  Conventional Commit and accepts one that is), `rust-push`, `ui-push`,
  `tauri-push` (a pushed range touching one area runs that area's pre-push
  commands, and no other area's), `push-isolation` (a linked-worktree commit
  and push leave its HEAD, refs, index and files unchanged while a cargo stand-in uses Git
  in another repository), `all` (`--all` runs every area), `fail` (a
  stand-in that exits 1 fails the hook and prints the failed command).
- `tests/githooks-command.sh` — the stand-in for `cargo`, `npm`, `npx` and
  `shellcheck` the githooks boundary tests put on `PATH`, and for
  `scripts/check-unused-rust` in their repository copy: it logs its own
  invocation and exits 1 only when `TEST_FAIL` names it exactly. With
  `TEST_GIT_PROBE=1`, its cargo clippy and nextest calls use Git in a
  separate repository to prove the hooks clear Git routing variables.
- `tests/check-migrations.sh` — run as `bash scripts/tests/check-migrations.sh
  CASE` for one of its cases: `reject-edit`, `reject-delete` (a migration
  already on `main` is modified or deleted), `accept-add` (a new, correctly
  numbered migration), `reject-low-number` (a new migration numbered no
  higher than one already shipped), `reject-bad-name` (a new file with no
  version prefix), `reject-symlink-replace` (a shipped migration replaced by
  a symlink), `reject-new-symlink` (a new migration that is a symlink),
  `reject-landed-after-squash` (a migration that landed on `main` through a
  squash, after the branch forked, is then edited on the branch),
  `reject-version-collision` (a branch's own new migration collides with a
  different one `main` shipped under the same version number while the
  branch was forked), `unrelated-change` (a change outside the migrations
  directory), `accept-approved-edit` and `reject-near-miss-edit` (the one
  named exception `check-migrations.sh` pins — see below — accepts exactly
  its own old-to-new blob pair and nothing else; a git blob hash is a pure
  function of its content, never of which repository holds it, so writing
  this repository's own pre- and post-edit `0001_init.sql` bytes into the
  throwaway one, verbatim, reproduces the exact same two blobs the real,
  fixed pin names, with no override of any kind). Builds a throwaway
  repository with one shipped migration on `main` and drives
  `check-migrations.sh` against a branch built for the case.

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
cover. `tests/install-sign-macos.sh`, `tests/install-service-macos.sh` and
`tests/make-signing-cert-scenarios.sh` are the same, on macOS, for each of
their cases. `tests/githooks.sh`, `tests/release-notes.sh` and
`tests/check-migrations.sh` are the same, for each of their cases, on any OS.
`tests/install-command.sh`, `tests/make-signing-cert-command.sh` and
`tests/githooks-command.sh` need no direct invocation; the scenario scripts
put them on `PATH` for the tools they stand in for.
