---
id: install-service-and-release
status: current
updated: 2026-09-04
areas: [install, scripts, store]
commits: [affda30b, 7ac6b2e3, 60905e41, b0ab8333, 1bbd6251]
tests:
  - crates/ariadne-store/tests/store.rs
  - scripts/install.sh
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
   completions, installs the Ariadne Desktop app, and has the user trust
   Ariadne's Codex hooks. On Linux it also registers the app with GNOME: a
   `~/.local/share/applications/dev.ariadne.ui.desktop` entry and an icon
   under `~/.local/share/icons/hicolor`, taken from the AppImage
   (`--appimage-extract`, which needs no FUSE) or, for a source build, from
   `ui/src-tauri/icons/`.
2. It is idempotent: safe to re-run after an upgrade or a config change, every
   step replacing what a previous run installed. What was installed where is
   recorded in `~/.ariadne/install.env`, which `uninstall.sh` reads.
3. Binaries come from a GitHub release by default, and from a local build with
   `--build-from-source`. Release assets are unsigned but carry a build
   provenance attestation, so every downloaded file is checked with
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
   attestation name the tag as the origin of the assets.
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
   is pre-1.0, so a database is recreated rather than migrated.
10. `ariadne doctor` is the only thing still running when that happens, so it
    is what explains it (014).

## Acceptance criteria

- A database from before the squash says which file to delete
  (`store.rs::a_database_from_before_the_squash_says_which_file_to_delete`).
- Built-in profiles are seeded into a fresh database on every default and are
  not recreated on reopen
  (`store.rs::a_fresh_database_is_seeded_with_the_built_in_profiles_on_every_default`,
  `::built_ins_are_not_recreated_on_reopen`), and so are the per-agent launch
  flags (`::agent_configs_are_seeded_with_the_defaults`).
- The installer fails late and unprompted on an unsupported OS, showing the log
  tail (`scripts/install.sh`, covered by
  `fix(scripts): keep --purge unprompted and fail late on an unsupported OS`,
  `fix(scripts): show the log tail when an unsupported OS fails the service step`).

## Known gap

Editing the squashed migration invalidates every existing database, and the
built-in advice is to delete it. That is cheap for a fresh install and
expensive for a machine holding real goal history. The alternatives — adding a
successor migration rather than editing `0001`, or teaching `doctor` to repair
the checksum in place — are not implemented.

## Sources

`scripts/install.sh`, `scripts/lib.sh`, `scripts/uninstall.sh`,
`.github/RELEASING.md`, `crates/ariadne-store/src/lib.rs`,
`crates/ariadne-store/migrations/0001_init.sql`.
