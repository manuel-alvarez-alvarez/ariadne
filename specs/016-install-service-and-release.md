---
id: install-service-and-release
status: current
updated: 2026-10-16
areas: [daemon, install, scripts, store, workflows]
commits: [affda30b, 7ac6b2e3, 60905e41, b0ab8333, 1bbd6251, d5d971a5, 03f9c8b7]
tests:
  - crates/ariadne-daemon/src/resource.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/agents.rs
  - scripts/install.sh
  - scripts/tests/install-linux.sh
  - scripts/tests/install-sign-macos.sh
  - scripts/tests/install-service-macos.sh
  - scripts/tests/make-signing-cert-scenarios.sh
  - scripts/tests/release-please-config.sh
  - scripts/tests/release-notes.sh
  - scripts/tests/githooks.sh
  - scripts/tests/check-migrations.sh
  - .github/workflows/release-please.yml
  - .github/workflows/release.yml
---

# Install, service and release

How Ariadne gets onto a machine, how it runs there, and how a version comes to
exist. Also the one rule that decides whether an existing database survives an
upgrade.

## Scope

In: the installer and what it installs, the user service, release
verification, the release-please loop, and the database migration policy.

Out: what the daemon does once running (009, 012).

## Behavior

1. `scripts/install.sh` installs the binaries, registers the daemon as a user
   service (launchd on macOS, `systemd --user` on Linux), installs bash and zsh
   completions, and installs the Ariadne Desktop app. On Linux, the installer
   checks for `libwebkit2gtk-4.1.so.0` when the app is requested. If missing,
   it skips only the app and GNOME registration, gives apt, dnf and pacman
   install hints, and includes the reason in the summary. Linux releases
   supply `ariadne-desktop-<target>.tar.gz`, verified before extraction.
   Its root contains `ariadne-desktop` and `icon.png`. The installer installs
   the binary as `$PREFIX/ariadne-desktop`, mode 755. Linux source builds
   disable bundling and install the plain binary. On Linux it also registers
   the installed app with GNOME: a
   `~/.local/share/applications/dev.ariadne.ui.desktop` entry and an icon
   under `~/.local/share/icons/hicolor`, taken from the tarball's `icon.png`
   or, for a source build, from
   `ui/src-tauri/icons/`. The entry names the window it belongs to
   (`StartupWMClass`, the installed app's own basename), without which the
   icon is right in the app grid and generic in the dash — the grid reads the
   entry, the dash has only the window (015).
2. It is idempotent: safe to re-run after an upgrade or a config change, every
   step replacing what a previous run installed. What was installed where is
   recorded in `~/.ariadne/install.env`, which `uninstall.sh` reads.
3. Binaries come from a GitHub release by default, and from a local build with
   `--build-from-source`. A local build is taken from the directory cargo
   wrote it to, `CARGO_TARGET_DIR` included: a checkout that builds elsewhere
   would otherwise install whatever stale binaries `target/release` still
   holds. macOS release assets are signed with the self-signed `Ariadne Code
   Signing` certificate and all release assets carry a build provenance
   attestation, so every downloaded file is checked with
   `gh attestation verify` before anything is installed — which makes the
   GitHub CLI a hard requirement of the default flow — and the macOS quarantine
   attribute is cleared from what is installed.
4. Output is a numbered step list; noisy subcommands go to
   `~/.ariadne/install.log` and are shown only when a step fails. `--verbose`
   streams them instead.
5. Releases are automated end to end: release-please keeps one open
   `chore(main): release X.Y.Z` pull request holding the version bump and the
   changelog entry, and merging it tags the version and publishes the release.
   The asset workflow runs on the **tag**, which is what makes the provenance
   attestation name the tag as the origin of the assets. On macOS, `npm ci`,
   the frontend build and the Tauri shell's own Rust compile all run before
   the release signing certificate is imported into a temporary keychain, so
   no install script and no `build.rs` ever runs with that keychain unlocked.
   The certificate is imported only to bundle and sign the app that is
   already built, and the workflow signs and verifies the desktop app and
   both command-line binaries, then deletes that keychain right away —
   whether the job succeeds or fails — rather than at the end of the job.
6. Only conventional commits are seen by release-please; anything else is
   silently ignored, neither moving the version nor appearing in the notes.
   The allowed types live in `AGENTS.md` and nowhere else.
   Release-please puts automatic breaking changes first.
   Its `changelog-sections` setting in `release-please-config.json` then adds
   ✨ Features, 🐛 Bug fixes, ⚡ Performance, and ⏪ Reverts. It hides `docs`,
   `refactor`, `test`, `chore`, `style`, `build`, and `ci` from the notes.
7. `feat!:` does not jump to 1.0.0 while the project is pre-1.0.
8. History is linear: no merge commits. A task branch lands on its base by
   squash or fast-forward, and the commit that lands carries a conventional
   subject of its own.
9. The schema is one squashed init migration, `0001`. A database whose
   `_sqlx_migrations` records a version or a checksum this release does not
   ship is refused at open, with a sentence naming the file to delete: Ariadne
   is pre-1.0, so a database is recreated rather than migrated. The eight
   migrations that had accumulated since the previous squash (`0002` through
   `0009`) were folded back into `0001`, completing the final, one-time
   squash of that nine-file chain (`0001` through `0009`) before 1.0; a
   database written anywhere on that chain, or on the chain before it, is
   refused the same way, and that loss is accepted. From this reset on,
   every schema change adds a new migration file: `0001` is not edited
   again, with one named exception. Narrowing the forge integration to
   GitHub alone (025) tightened `forge_integrations.kind`'s `CHECK` to
   `('github')` from a wider set of accepted kinds, in place, on `0001`
   itself.
   `scripts/check-migrations.sh` pins that edit's exact old and new git
   blob, so it is the one transition of the one file this rule ever lets
   through a second time; every other edit of `0001`, or of any other
   migration already shipped, still fails the guard the same as before. A
   database whose recorded checksum for version 1 is the pre-edit blob is
   refused at open the same way, naming the file to delete; that loss,
   too, is accepted.
10. `ariadne doctor` is the only thing still running when that happens, so it
    is what explains it (014).
11. At startup, the daemon raises its soft open-file limit towards the hard
    limit, capped at 4096, and logs the old and new values. The cap matters on
    macOS, where `setrlimit` refuses a value above `OPEN_MAX` when launchd
    reports an unlimited hard limit. The installed launchd and systemd
    services set the same soft limit, so reinstalling also protects an older
    daemon binary.
12. The launchd service is installed with `ProcessType` `Interactive`. A
    job without one is throttled to the utility level, which every agent the
    daemon starts inherits and cannot leave, so its builds and tests run on
    the efficiency cores. The systemd service needs no setting: a user
    service runs at the same weight as the user's own session.
13. `scripts/make-signing-cert.sh` creates the self-signed "Ariadne Code
    Signing" identity, Darwin only: a key and a code-signing certificate made
    with `openssl`, imported into the login keychain with `security import`
    and trusted for code signing with `security add-trusted-cert` - which
    macOS gates behind its own one-time authentication prompt, approved by
    hand. The key and certificate are also kept at
    `~/.ariadne/signing-cert`. Idempotent: a second run, finding a valid
    identity already there, exports it again rather than replacing it, so a
    lost `.p12` or forgotten password is not a reason to change the
    signature every build after carries; `--force` replaces it regardless,
    and a certificate present but not a valid identity is replaced even
    without `--force`. It prints the three `gh secret set` commands the
    release workflow's secrets are read from, or runs them with
    `--set-secrets`. On macOS, `install.sh --build-from-source` signs
    `ariadne`, `ariadned` and the Ariadne Desktop app with that identity when
    it is in the login keychain, and still produces a complete, unsigned
    build when it is not.
14. The Release page release-please publishes carries only the changelog
    entry, with no install steps and no asset list. Once every target in
    `release.yml`'s `build` job has staged and uploaded its
    `checksums-<target>.txt`, a `notes` job downloads every one of them,
    reads the release's current body, and runs `scripts/release-notes.sh
    TAG BODY_FILE CHECKSUMS_FILE...` to print a new body: an install section
    naming `scripts/install.sh --version <tag>`, a table with one row per
    line of every checksums file (the asset name as a download link, its
    target, and its SHA-256 digest), and a compare link to the previous tag
    when one is known from `BODY_FILE` or from `git`, in that order, all
    wrapped in a pair of HTML comment markers, followed by the
    release-please notes `BODY_FILE` already held, unchanged. `BODY_FILE` is
    read as `\r\n` or `\n`, since a body edited in the GitHub web UI comes
    back with `\r\n`; a body that holds one marker but not the other is
    refused rather than guessed at. The job writes the result back with `gh
    release edit --notes-file`. A re-run - by hand, or because a tag already
    carries the markers - replaces only what is between them, so the header
    and the table never stack a second time; the script is idempotent,
    giving the same body when run twice on its own output. Runs under both
    `release: published` and `workflow_dispatch`, same as the `build` job it
    depends on.
15. `ui_stop_daemon` waits for launchd to drop a label it just told to
    bootout, and for a pid-file daemon it just killed to exit — each up to
    ~30s, going on regardless so a wait that times out never fails the
    install: `launchctl bootout` and a plain `kill` both return before the
    old process is actually gone. `install.sh` retries a `launchctl
    bootstrap` that loses that race, up to 5 times, and calls `ui_die` only
    once the last try has also failed.
16. `scripts/check-migrations.sh` enforces rule 9's last sentence by machine
    rather than by convention alone. It compares
    `crates/ariadne-store/migrations/` on the latest commit of `main`
    directly against HEAD, never their merge-base, so a branch that forked
    before a migration landed still sees it as shipped rather than as a
    fresh addition it is free to overwrite. A path already on `main` must
    stay at the identical git mode and blob — catching an edit and a type
    change (a symlink swapped in for the file) alike — and a new path must
    be a regular file named `NNNN_description.sql`, with a version number
    greater than every migration `main` already ships and than every other
    new one in the same change. Pull-request CI and the pre-push hook run
    the identical command.

## Acceptance criteria

- Linux release and source installs use the plain desktop binary and its icon.
  Missing WebKitGTK skips only the app and GNOME registration. Extraction
  requires attestation verification, and uninstall removes the desktop files
  (`scripts/tests/install-linux.sh`: `release`, `source`, `missing`,
  `source-missing`, `rejected`, `no-ui`).
- A database from before the squash says which file to delete
  (`store.rs::a_database_from_before_the_squash_says_which_file_to_delete`).
- The shipped skills are seeded into a fresh database, each on the text
  Ariadne ships, and a reopen reseeds no row the database already holds (017)
  (`store.rs::a_fresh_database_is_seeded_with_every_shipped_skill_on_its_own_text`,
  `::a_reopen_reseeds_no_row_the_database_already_holds`). No agent flags
  are seeded: an agent nobody set flags for is listed and launched with none
  (007, `agents.rs::every_registry_agent_is_listed_with_its_flags_and_its_defaults`).
- A source build installs the binaries cargo wrote, under `CARGO_TARGET_DIR`
  as much as under `target/`
  (`scripts/tests/install-linux.sh`: `source`, which exports
  `CARGO_TARGET_DIR` and has its fake `cargo` write there).
- Daemon startup raises a low soft open-file limit
  (`resource.rs::daemon_start_raises_its_soft_open_file_limit`), and both
  installed service definitions set the same limit
  (`resource.rs::installer_services_raise_the_open_file_limit`).
- The launchd service runs as interactive work
  (`resource.rs::the_launchd_service_runs_as_interactive_work`).
- `scripts/make-signing-cert.sh` creates the self-signed "Ariadne Code
  Signing" identity. A second run, finding a valid one, exports it again
  rather than replacing it; `--force` replaces it regardless, and a
  certificate present but not a valid identity is replaced even without
  `--force`. It prints or, with `--set-secrets`, sets the three repository
  secrets the release workflow reads the same certificate from
  (`scripts/tests/make-signing-cert-scenarios.sh`: `create`, `idempotent`,
  `replace`, `repair-invalid`, `export`, `secrets`). Proven for real on a
  machine, not only against the stub: `security find-identity -v -p
  codesigning` lists the identity after a run.
- On Darwin, `install.sh --build-from-source` signs `ariadne` and `ariadned`
  with that identity when it is in the login keychain, and skips with a step
  note - never failing the install - when it is not
  (`scripts/tests/install-sign-macos.sh`: `present`, `missing`).
- An upgrade whose old launchd job is slow to drop, and whose first
  `launchctl bootstrap` loses the race against it, still ends with the
  service registered: `ui_stop_daemon` waits out the slow `print`, and
  `install.sh` retries the failed `bootstrap`, rather than either one
  calling `ui_die` (`scripts/tests/install-service-macos.sh`).
- In the `build` job of `.github/workflows/release.yml`, `npm ci`, the
  `Compile Ariadne Desktop` step (or, on Linux, `Build Ariadne Desktop (no
  bundle)`) and `Build ariadne and ariadned` all run before `Import the
  macOS signing certificate`; `Delete the temporary macOS signing keychain`
  runs right after `Verify the signed macOS assets`, before staging,
  attesting or uploading any asset (`.github/workflows/release.yml`).
- `release-please-config.json` defines ✨ Features, 🐛 Bug fixes, ⚡
  Performance, and ⏪ Reverts in that order. It hides the other allowed
  commit types from release notes. The pre-commit hook runs its section test
  when the config changes (`scripts/tests/release-please-config.sh`,
  `scripts/tests/githooks.sh`: `release-please-config-commit`).
- `scripts/release-notes.sh` turns a plain changelog body into one with an
  install section and an asset table ahead of it, and a re-run on a body
  that already carries the markers replaces the generated part without
  stacking a second header; an asset table built from checksums files of two
  different targets lists every line of both under their own target; a
  re-run on a body with `\r\n` line ends, as the GitHub web UI writes one,
  still finds the markers and keeps the release-please notes
  (`scripts/tests/release-notes.sh`: `first`, `rerun`, `two-targets`,
  `crlf`).
- A migration already on `main` cannot be edited, deleted or retyped (for
  instance replaced by a symlink), and a new one must be a regular file
  numbered higher than every migration already there — including one that
  landed through a squash after the checking branch forked, and one that
  collides with a same-numbered migration shipped under a different name
  (`scripts/tests/check-migrations.sh`: `reject-edit`, `reject-delete`,
  `accept-add`, `reject-low-number`, `reject-bad-name`,
  `reject-symlink-replace`, `reject-new-symlink`,
  `reject-landed-after-squash`, `reject-version-collision`). The one named
  exception in rule 9 is pinned by `0001_init.sql`'s exact pre- and
  post-edit git blob hash in `scripts/check-migrations.sh`'s own fixed
  `APPROVED_EDIT_FROM`/`APPROVED_EDIT_TO`, read from no environment or
  argument; a git blob hash is a pure function of its content, never of
  which repository holds it, so writing this same content, verbatim, into
  a throwaway repository exercises that real, unmodified pin directly —
  exactly that pair passes, and the same content with one further edit
  still fails
  (`scripts/tests/check-migrations.sh`: `accept-approved-edit`,
  `reject-near-miss-edit`).

## Known gap

The squash this reset performed invalidated every existing database, and the
built-in advice was to delete it. That was cheap for a fresh install and
expensive for a machine holding real goal history, which is why rule 9 now
retires squashing for good: every later schema change adds a new migration
file instead of touching `0001` again. Teaching `doctor` to repair the
checksum in place is not implemented, and rule 9 ending future squashes
removes most of the reason to build it.

The installer fails late and unprompted on an unsupported OS, showing the log
tail. No script test covers that path; it is proven only by the commits that
fixed it (`fix(scripts): keep --purge unprompted and fail late on an
unsupported OS`, `fix(scripts): show the log tail when an unsupported OS
fails the service step`).

## Sources

`scripts/install.sh`, `scripts/lib.sh`, `scripts/uninstall.sh`,
`scripts/make-signing-cert.sh`, `scripts/release-notes.sh`,
`scripts/check-migrations.sh`, `scripts/tests/check-migrations.sh`,
`scripts/tests/release-please-config.sh`,
`.github/workflows/release.yml`, `.github/RELEASING.md`,
`crates/ariadne-store/src/lib.rs`,
`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-daemon/src/resource.rs`, `crates/ariadne-daemon/src/main.rs`.
