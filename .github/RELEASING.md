# Releasing

Releases are automated. Nobody edits a version by hand, writes a changelog entry
or cuts a tag: [release-please](https://github.com/googleapis/release-please)
does all three from the commit history.

## The loop

1. You merge commits into `main`.
2. `.github/workflows/release-please.yml` runs and keeps one open pull request,
   `chore(main): release X.Y.Z`, holding the version bump and the changelog
   entry for everything released since the last tag. It is rewritten on every
   push, so it is always the current pending release.
3. Merging that PR makes release-please tag `vX.Y.Z` and publish a GitHub
   Release whose notes are the changelog entry. Attaching binaries to that
   Release is a separate workflow, `.github/workflows/release.yml`, which
   release-please dispatches on the new tag (a Release it publishes with the
   default `GITHUB_TOKEN` fires no `release: published` event of its own).
   Running it on the tag rather than on `main` is what makes the build
   provenance attestation name the tag as the origin of the assets; a rerun
   started by hand should pick the tag in the ref dropdown for the same reason,
   and warns if it does not.
4. Not merging it costs nothing — commits accumulate into the same PR.

## Commit messages

Only [conventional commits](https://www.conventionalcommits.org) are seen by
release-please. Everything else is silently ignored: it neither appears in the
notes nor moves the version.

**The allowed types and what each one does to a release are in
[`AGENTS.md`](../AGENTS.md#commit-messages)**, which is what anyone writing a
commit here reads first; they are written down there and nowhere else.

One thing here is configuration rather than convention: `feat!:` deliberately
does not jump to 1.0.0 while the project is pre-1.0, which is what
`bump-minor-pre-major` in `release-please-config.json` buys — drop it when 1.0
is the intent.

## What one bump touches

The project version lives in seven places and `release-please-config.json`
updates all of them from a single bump, so a release never leaves a stale
lockfile behind:

| File                          | How                                          |
| ----------------------------- | -------------------------------------------- |
| `Cargo.toml`                  | `[workspace.package] version` (all crates inherit it) |
| `Cargo.lock`                  | every `ariadne-*` package entry              |
| `ui/package.json`             | `version`                                    |
| `ui/package-lock.json`        | `version` and `packages[""].version`         |
| `ui/src-tauri/Cargo.toml`     | `[package] version`                          |
| `ui/src-tauri/Cargo.lock`     | the `ariadne-ui` package entry               |
| `ui/src-tauri/tauri.conf.json`| `version`                                    |

`release-type` is `simple` because no built-in strategy fits a repository that
is a Cargo workspace, an npm package and a Tauri app at once — the Rust strategy
chokes on a virtual manifest with no `[package]`, and the node one only knows
about `ui/`. Every location is therefore listed explicitly as an `extra-files`
entry with a jsonpath, and the `Cargo.lock` entry matches every `ariadne-*`
package, so a new workspace crate is picked up without touching the config.
(TOML jsonpaths address release-please's parsed TOML, in which every scalar is a
node — hence `@.name.value` rather than `@.name` in the lockfile filters.)
`version.txt`, which the `simple` strategy would otherwise update, does not
exist here and is skipped with a warning.

`.release-please-manifest.json` records the version last released; it is
release-please's source of truth and is updated by the release PR too.

`CHANGELOG.md` is created by the first release PR — it is not hand-written.

## One-time repository settings

After pushing this to GitHub, in **Settings → Actions → General**:

- **Workflow permissions** → tick **Allow GitHub Actions to create and approve
  pull requests**. Without it the workflow fails with
  `GitHub Actions is not permitted to create or approve pull requests` and no
  release PR ever appears.

No personal access token is needed: the workflow runs on the default
`GITHUB_TOKEN` with `contents: write` and `pull-requests: write`.
