# .github/AGENTS.md

Conventions for changing CI and the release automation. Commit-message and
history rules live in the root [`AGENTS.md`](../AGENTS.md).

## The workflows

- [`workflows/ci.yml`](workflows/ci.yml) — runs on every push to `main` and
  every pull request: `fmt` checks `cargo fmt` on the workspace and on
  `ui/src-tauri`; `rust` builds the workspace and runs `cargo nextest run
  --workspace`, `cargo test --doc --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `scripts/check-unused-rust` and `cargo
  machete .`, on Ubuntu and macOS; `ui` runs `npm run lint`, `npm run
  typecheck`, `npm run check:unused` and `npm test` under `ui/`; `tauri` runs
  `cargo check` under `ui/src-tauri` on Ubuntu and macOS.
- [`workflows/release-please.yml`](workflows/release-please.yml) — runs on
  every push to `main`; keeps the single pending release pull request current
  from the commits since the last tag.
- [`workflows/release.yml`](workflows/release.yml) — runs on `release:
  published` (and by hand, against a tag, for a rerun); builds `ariadne`,
  `ariadned` and Ariadne Desktop for each released target, attests their
  provenance and uploads them to the release.

[`RELEASING.md`](RELEASING.md) is the human explanation of the loop these two
release workflows drive, the release token, and what to do when it expires.

## Rules

- Every check `ci.yml` runs matches a command named in
  [`crates/AGENTS.md`](../crates/AGENTS.md) or
  [`ui/AGENTS.md`](../ui/AGENTS.md). A step that runs something neither file
  names is either undocumented there or redundant here.
- The project version changes only through `release-please-config.json` (and
  the manifest it points at): nobody edits a version field, `CHANGELOG.md` or
  a tag by hand. See [`RELEASING.md`](RELEASING.md) for what one bump touches.
- Do not change a workflow file as a side effect of a task that is not about
  CI or releasing.

## Pinned tool versions

- `cargo-machete@0.9.2` in `ci.yml`, matching the version
  [`crates/AGENTS.md`](../crates/AGENTS.md) tells a contributor to install.
- `googleapis/release-please-action@45996ed1f6d02564a971a2fa1b5860e934307cf7`
  (`v5.0.0`) in `release-please.yml`.
- Every action `release.yml` uses is pinned to a commit SHA, not a floating
  tag: its run is anchored to a release tag and its build provenance
  attestation must name exactly what ran. `ci.yml` uses floating tags
  (`@v5`, `@stable`) instead, since nothing there is attested.

**Known gap:** `ci.yml`'s `ui` job installs Node 26 (`actions/setup-node@v5`,
`node-version: '26'`); `release.yml`'s build job installs Node 22
(`actions/setup-node@820762786026740c76f36085b0efc47a31fe5020`,
`node-version: 22`). The two should match; nothing today keeps them in sync.
