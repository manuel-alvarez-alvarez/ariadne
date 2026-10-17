# .github/AGENTS.md

Conventions for changing CI and the release automation. Commit-message and
history rules live in the root [`AGENTS.md`](../AGENTS.md).

## The workflows

- [`workflows/ci.yml`](workflows/ci.yml) — runs on every push to `main` and
  every pull request: `fmt` checks `cargo fmt` on the workspace and on
  `ui/src-tauri`; `rust` runs `cargo machete .`, `cargo audit`, `cargo build
  --locked --workspace --bins`, `cargo nextest run --workspace`, `cargo test
  --doc --workspace`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `scripts/check-unused-rust` and `scripts/check-migrations.sh`,
  on Ubuntu and macOS; `ui` runs `npm run lint`, `npm run typecheck`, `npm
  run check:unused`, `npm run check:audit` and `npm test` under `ui/`;
  `tauri` runs `cargo check` under `ui/src-tauri` on Ubuntu and macOS.
- [`workflows/release-please.yml`](workflows/release-please.yml) — runs on
  every push to `main`; keeps the single pending release pull request current
  from the commits since the last tag.
- [`workflows/release.yml`](workflows/release.yml) — runs on `release:
  published` (and by hand, against a tag, for a rerun); builds `ariadne`,
  `ariadned` and Ariadne Desktop for each released target, imports the macOS
  signing certificate, signs and verifies the macOS assets, attests their
  provenance and uploads them to the release. Its `notes` job then runs
  `scripts/release-notes.sh` against the uploaded checksums files and the
  release's own current body, and writes the result back with `gh release
  edit --notes-file`, so the Release page gets install steps and an asset
  table in front of the changelog release-please wrote.
- [`dependabot.yml`](dependabot.yml) — a weekly check of four ecosystems:
  `cargo` at the repository root, `cargo` in `ui/src-tauri` (its own
  workspace, outside the root one), `npm` in `ui/`, and `github-actions` at
  the root. Each ecosystem groups its own minor and patch bumps into one
  pull request; a major bump still opens its own, since those more often
  need a manual look. Every commit carries the `chore(deps)` prefix, so
  release-please ignores it the same as any other `chore` commit (root
  [`AGENTS.md`](../AGENTS.md#commit-messages)) and a dependency bump alone
  never triggers a release. Dependabot only proposes; `ci.yml`'s `cargo
  audit` and `npm run check:audit` are what fail a build on an advisory, and
  that does not change here.

[`RELEASING.md`](RELEASING.md) is the human explanation of the loop these two
release workflows drive, the release token, and what to do when it expires.

## CI caches

`ci.yml` uses `Swatinem/rust-cache` for Cargo downloads and dependency build
artifacts, with separate keys for the Rust and Tauri jobs on each OS. Only
`main` saves these caches; pull requests restore them without saving private
copies that compete for the repository's cache quota.

The Rust job also runs the compiler through `sccache`, with incremental
compilation disabled and GitHub Actions as its backend. This reuses unchanged
workspace library compilations that `rust-cache` does not retain. Only `main`
writes to this cache; pull requests use `READ_ONLY`. The action's post step
reports cache statistics. Binary linking and test execution still run on
every build. Tauri keeps its dependency cache without a compiler wrapper:
its only compile step is `cargo check`.

The UI caches npm's downloads through `actions/setup-node`, keyed by
`ui/package-lock.json`, and runs `npm ci` on every build.

## Rules

- Every check `ci.yml` runs matches a command named in
  [`crates/AGENTS.md`](../crates/AGENTS.md) or
  [`ui/AGENTS.md`](../ui/AGENTS.md). A step that runs something neither file
  names is either undocumented there or redundant here. The pre-push hook
  runs every check `ci.yml` runs except the Rust test suite (`cargo nextest`)
  and the UI test suite (`npm test`): those take minutes, so CI alone runs
  them and a contributor runs either by hand before a push the change
  warrants it. A check added to `ci.yml` outside those two suites is added to
  the hook too.
- The project version changes only through `release-please-config.json` (and
  the manifest it points at): nobody edits a version field, `CHANGELOG.md` or
  a tag by hand. See [`RELEASING.md`](RELEASING.md) for what one bump touches.
- Do not change a workflow file as a side effect of a task that is not about
  CI or releasing.

## Pinned tool versions

- `cargo-machete@0.9.2` in `ci.yml`, matching the version
  [`crates/AGENTS.md`](../crates/AGENTS.md) tells a contributor to install.
- `sccache` `v0.16.0` via `mozilla-actions/sccache-action@v0.0.11` in
  `ci.yml`; the action configures GitHub's cache service and reports stats.
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
