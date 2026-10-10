---
id: install-service-and-release
status: current
updated: 2026-10-10
areas: [daemon, install, scripts, store]
commits: [affda30b, 7ac6b2e3, 60905e41, b0ab8333, 1bbd6251, d5d971a5, 03f9c8b7]
tests:
  - crates/ariadne-daemon/src/resource.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-daemon/tests/it/agents.rs
  - scripts/install.sh
  - scripts/tests/install-linux.sh
  - scripts/tests/install-sign-macos.sh
  - scripts/tests/make-signing-cert-scenarios.sh
  - .github/workflows/release-please.yml
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
   attestation name the tag as the origin of the assets. On macOS, it imports
   the release signing certificate into a temporary keychain, signs and
   verifies the desktop app and both command-line binaries, then deletes that
   keychain whether the job succeeds or fails.
6. Only conventional commits are seen by release-please; anything else is
   silently ignored, neither moving the version nor appearing in the notes.
   The allowed types live in `AGENTS.md` and nowhere else.
7. `feat!:` does not jump to 1.0.0 while the project is pre-1.0.
8. History is linear: no merge commits. A task branch lands on its base by
   squash or fast-forward, and the commit that lands carries a conventional
   subject of its own.
9. The schema is one squashed init migration. A database whose
   `_sqlx_migrations` records a version or a checksum this release does not
   ship is refused at open, with a sentence naming the file to delete: Ariadne
   is pre-1.0, so a database is recreated rather than migrated. The init
   migration holds the stats ledger, `stat_facts` (023). It was added to
   `0001` in place, so a database written before it is refused and must be
   removed; that loss is accepted.
10. `ariadne doctor` is the only thing still running when that happens, so it
    is what explains it (014).
11. At startup, the daemon raises its soft open-file limit towards the hard
    limit, capped at 4096, and logs the old and new values. The cap matters on
    macOS, where `setrlimit` refuses a value above `OPEN_MAX` when launchd
    reports an unlimited hard limit. The installed launchd and systemd
    services set the same soft limit, so reinstalling also protects an older
    daemon binary.
12. `scripts/make-signing-cert.sh` creates the self-signed "Ariadne Code
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

## Known gap

Editing the squashed migration invalidates every existing database, and the
built-in advice is to delete it. That is cheap for a fresh install and
expensive for a machine holding real goal history. The alternatives — adding a
successor migration rather than editing `0001`, or teaching `doctor` to repair
the checksum in place — are not implemented.

The installer fails late and unprompted on an unsupported OS, showing the log
tail. No script test covers that path; it is proven only by the commits that
fixed it (`fix(scripts): keep --purge unprompted and fail late on an
unsupported OS`, `fix(scripts): show the log tail when an unsupported OS
fails the service step`).

## Sources

`scripts/install.sh`, `scripts/lib.sh`, `scripts/uninstall.sh`,
`scripts/make-signing-cert.sh`, `.github/RELEASING.md`,
`crates/ariadne-store/src/lib.rs`,
`crates/ariadne-store/migrations/0001_init.sql`,
`crates/ariadne-daemon/src/resource.rs`, `crates/ariadne-daemon/src/main.rs`.
