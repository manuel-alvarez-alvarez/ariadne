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
   Release is a separate workflow, `.github/workflows/release.yml`, started by
   the `release: published` event. A release event runs on the tag rather than
   on `main`, which is what makes the build provenance attestation name the
   tag as the origin of the assets; a rerun started by hand should pick the tag
   in the ref dropdown for the same reason, and warns if it does not.
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

## The release token

release-please acts as `RELEASE_PLEASE_TOKEN`, a fine-grained personal access
token, and not as the default `GITHUB_TOKEN`. Two things depend on that:

- **CI runs on the release PR.** A PR opened with `GITHUB_TOKEN` is authored by
  `github-actions[bot]`, whose author association is `CONTRIBUTOR` and not
  `OWNER`. With **Settings → Actions → General → Approval for running fork
  pull request workflows from contributors** set to *first-time contributors* —
  the default — every CI run on the release branch stops at `action_required`
  and waits for a maintainer to press approve. A release PR opened with a token
  of ours is our own PR, and its CI starts by itself.
- **The assets get built.** GitHub fires no workflow trigger for any event
  created with `GITHUB_TOKEN`, so a Release published with it would never start
  `release.yml`. A Release published with a PAT does.

Create the token at **Settings → Developer settings → Personal access tokens →
Fine-grained tokens**:

| Field | Value |
| --- | --- |
| Repository access | Only select repositories → this one |
| Repository permissions → Contents | Read and write (branches, tags, releases) |
| Repository permissions → Pull requests | Read and write (open and update the release PR) |
| Expiration | up to a year; the release workflow starts failing on bad credentials the day it lapses |

Nothing else: release-please touches no workflow file here, so it needs no
`Workflows` permission.

Then store it on the repository, as a secret named exactly that:

```sh
gh secret set RELEASE_PLEASE_TOKEN --repo <owner>/<repo>
```

The **Allow GitHub Actions to create and approve pull requests** setting under
**Settings → Actions → General → Workflow permissions** is no longer required —
it gates `GITHUB_TOKEN`, which no longer opens the PR — but leaving it ticked
costs nothing.

### Replacing it

When the token expires, `release-please.yml` fails with a credentials error and
no release PR is updated; nothing else in the repository is affected. Issue a
new token with the same two permissions, run the `gh secret set` above again,
and re-run the failed workflow.
