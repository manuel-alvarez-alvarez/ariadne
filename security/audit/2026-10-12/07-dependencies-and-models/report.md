# SUPPLY: dependencies and model artifacts

## Question

Can vulnerable or substituted dependencies and model artifacts compromise
Ariadne's builds or runtime execution?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (main, 2026-10-12 21:32:41 +0200),
checked out at `/Users/malvarez/.ariadne/worktrees/q4r9v3t3/w3anyvzc-eng` on
branch `audit-third-party-packages-and-model-anyvzc`.

Scan date for every advisory check below: 2026-10-12.

## Threat assumptions

- Installed coding-agent CLIs are trusted; this task does not attack them.
- Repository content, forge content, network responses, and third-party
  package/model registries are treated as potentially hostile.
- Documented permission or pinning guarantees are checked even for trusted
  installed agents.
- No attack is run against the user's live daemon, repositories, accounts,
  or external services; reproductions use disposable clones, local venvs,
  and read-only lookups against public registries (crates.io, npm, GitHub,
  Hugging Face).
- External technical claims (advisory text, repository metadata) are taken
  from primary sources: `cargo audit` against the RustSec advisory-db,
  `npm audit` against the npm registry, and the GitHub REST API — not from
  a web-page summary alone (see SUPPLY-002, which found a summarizer tool's
  narrative description of two repositories to be misleading on exactly the
  point this audit was checking).

## Coverage

### Reviewed

- `Cargo.toml`, `Cargo.lock` (workspace, 557 crate dependencies).
- `ui/src-tauri/Cargo.toml`, `ui/src-tauri/Cargo.lock` (465 crate
  dependencies, independent tree, excluded from the workspace).
- `ui/package.json`, `ui/package-lock.json` (718 npm packages: 537 prod, 181
  dev, per `npm audit`'s own classification).
- `crates/ariadne-daemon/src/ai_permissions/{install,flavours,server,python,mod,decide,hardware}.rs`.
- `bench/ai-permissions/run.sh`, `bench/ai-permissions/run.py`,
  `bench/ai-permissions/evaluators/{kev,laya}/*.py`, `bench/AGENTS.md`.
- `specs/022-ai-permission-mode.md` (full), `specs/024-advisory-failure-diagnosis.md`
  (scope and behavior sections — confirmed it governs error classification
  through the same Kev endpoint, not a dependency-advisory feature despite
  the filename; no further relevance to this task).
- `jaredpalmer/kev` at commit `f1535963cea021439370c23127bc970b6788e730`
  (the pinned production commit): `pyproject.toml` fetched at that revision;
  GitHub code search across the whole repository for `trust_remote_code`,
  `torch.load`, `safetensors` (0 hits each).
- GitHub repository metadata (`gh api repos/...`) for `jaredpalmer/kev` and
  `NandhaKishorM/laya`: creation date, push date, star/fork/issue counts,
  license, owner type.
- `crates/AGENTS.md`, `bench/AGENTS.md` for the checks and exposure rules
  each area declares for itself.

### Tested

- `cargo audit` (v0.22.2, RustSec advisory-db at 1,307 advisories, fetched
  live) against both Rust lockfiles.
- `npm audit --omit=dev` and `npm audit` (full) against `ui/package-lock.json`,
  cross-checked against `ui/package.json`'s own `dependencies`/`devDependencies`
  split.
- Dependency-chain reconstruction from `ui/package-lock.json`'s `packages`
  map (`npm ls` could not be used directly: no `node_modules` is installed
  in this worktree, so chains were traced by scanning each package's
  recorded `dependencies`/`optionalDependencies`).
- `cargo tree -i <crate>` (workspace and, with `--target all`, the Tauri
  tree) to find what pulls each flagged crate.
- `grep -rn "from ['\"]shadcn` and a scan of `ui/package.json`'s `scripts`
  for anything invoking `shadcn`, to establish whether that package is ever
  imported into application source or run by a build/dev script (see
  SUPPLY-003).
- `gh api repos/.../contents/pyproject.toml?ref=<pinned sha>` to read the
  pinned Kev commit's own declared dependency ranges.
- `gh api search/code` scoped to `repo:jaredpalmer/kev` for unsafe
  deserialization/remote-code markers.

### Excluded, with reason

- Actually installing the Kev/Laya virtual environments and downloading
  model weights. This machine is not configured with Python 3.12/3.13 for
  Kev or a GPU/Apple Silicon device for most flavours, the download is
  multiple gigabytes per flavour, and the install path's logic
  (`install.rs::pip_calls`, `::venv`, `::weights`) is already covered by
  the crate's own unit tests, which this audit read rather than duplicated.
  This is a coverage gap: the audit did not observe a live `pip install` or
  `huggingface_hub.snapshot_download` on the wire, so no independent
  confirmation exists that the installer's TLS/redirect behavior matches
  what the source implies. Recorded as an unresolved gap, not inferred.
- `crates/ariadne-daemon/tests/it/ai_permissions*.rs` were read for context
  (via the spec's test list) but not re-run; they exercise a stub installer,
  not the real `pip`/Hugging Face path, so re-running them would not have
  closed the gap above.
- GitHub Actions / release pipeline, release artifact signing, and the
  installer scripts under `scripts/`. Explicitly RELEASE's area per the
  cross-task contract; this report's SUPPLY-001..005 stay inside package
  and model-artifact resolution, not release provenance.
- `docs/` and `README.md`. Not named in the starting sources and not a
  resolution/execution path for a dependency or model.
- `bench/ai-opportunities/`. Not named in the starting sources; a quick
  `grep -r "pip install\|npm install\|curl.*|.*sh" bench/ai-opportunities`
  found no dynamic package or model fetch there, so it was not read further.
- Actually fetching `chacha20`'s yank rationale from `crates.io`'s API: the
  sandboxed `curl` to `crates.io` returned no body (silently empty, while
  `gh api` and `WebFetch` to other hosts worked). Recorded as a tool/network
  coverage gap in SUPPLY-005, not papered over.

## Findings

### Confirmed

#### SUPPLY-001 — The benchmark's Laya and Kev installs are unpinned and unverified, unlike production

- **Affected:** `bench/ai-permissions/run.sh:21-22,98-147` (`LAYA_RELEASES`,
  `KEV_REPOSITORY`, `latest_laya_wheel`, `ensure_venv`).
- **Prerequisites:** a developer runs `bench/ai-permissions/run.sh` (not CI —
  `bench/AGENTS.md` states "None of this runs in CI"), with network access,
  and either has no existing venv or passes `--rebuild`.
- **Path traced:** `curl -fsSL https://api.github.com/repos/NandhaKishorM/laya/releases/latest`
  (no auth, no pinned tag) → the first `.whl` asset URL from that response →
  `pip install "laya[serve] @ <that URL>"` with no `--hash`/`--require-hashes`
  → the wheel's `setup.py`/build backend and its own code run under the
  invoking developer's user, inside `$AI_HOME/laya-venv`. In parallel,
  `pip install "kev[serve] @ git+https://github.com/jaredpalmer/kev"` (no
  `@<commit>`) resolves to whatever commit is `HEAD` of the default branch
  at install time, not the commit the daemon pins
  (`crates/ariadne-daemon/src/ai_permissions/install.rs:20,295`).
  Contrast: the production installer never does this — `install.rs` always
  appends `@{KEV_COMMIT}`, a 40-character SHA that `flavours.rs`'s own test
  (`every_pin_is_a_full_sha_and_4b_matches_the_install`) checks for every
  flavour.
- **Impact:** whoever controls the content served at "latest release" of
  `NandhaKishorM/laya`, or at `HEAD` of `jaredpalmer/kev`'s default branch —
  the current maintainer, or anyone who compromises that account, or anyone
  who can race a release/push between this script's two runs — gets
  arbitrary code execution on a developer's machine the moment the script
  (or `--rebuild`) runs, with no hash or commit check to catch a swap.
- **Severity rationale:** high if exercised, because it is unauthenticated
  code execution with no integrity check at all; the blast radius is capped
  because this path is explicitly dev/benchmark-only, never reached by
  `ariadned` or by CI (`bench/AGENTS.md`).
- **Impact scope:** build/benchmark exposure only. Confirmed not reachable
  from production: `grep -ril laya crates/ specs/` returns nothing (see
  Coverage); `laya` never appears outside `bench/`.
- **Confidence:** confirmed. The script's own text is the evidence; no
  inference was needed.
- **Counter-evidence / mitigating controls:** this is explicitly out of CI
  and off by default — a developer has to run the script by hand. The
  production path for the same two dependencies (Kev) is correctly pinned,
  showing the project already knows the right pattern; it was just not
  carried into the benchmark harness.
- **Remediation (smallest):** pin `KEV_REPOSITORY` to a commit SHA the same
  way `install.rs::KEV_COMMIT` does, and replace `latest_laya_wheel` with a
  configured, committed wheel URL plus a `--hash=sha256:...` passed through
  to `pip install`. `LAYA_WHEEL` already exists as an override env var
  (`run.sh:52-53`); make it (or an equally explicit pin) the only path, not
  a fallback from "latest".
- **Regression criterion:** a test or script check that fails if `run.sh`
  contains the literal string `releases/latest` or a `git+https://...kev`
  URL with no `@` revision.
- **Compatibility decision for the user:** pinning Laya's wheel means a
  human has to bump it by hand to pick up a new Laya release, same as Kev
  already requires. Confirm this tradeoff is acceptable before a
  remediation task changes it.

#### SUPPLY-002 — The production AI-permission model's sole upstream has an anomalous trust profile

- **Affected:** `crates/ariadne-daemon/src/ai_permissions/flavours.rs:26-49`,
  `install.rs:20` (every `Flavour` pin, and `KEV_COMMIT`); by extension
  `bench/ai-permissions/evaluators/laya/*.py` for the benchmark-only Laya
  dependency.
- **Evidence (primary source, `gh api`, not a page summary):**

  | repo | created | last push | stars | forks | open issues | owner type |
  |---|---|---|---|---|---|---|
  | `jaredpalmer/kev` | 2026-09-17 | 2026-10-12 | 9,001 | 599 | 35 | User |
  | `NandhaKishorM/laya` | 2026-09-18 | 2026-10-12 | 32,542 | 2,906 | 116 | User |

  Both repositories were created roughly three to four weeks before this
  audit (2026-10-12) and already carry four- and five-figure star/fork
  counts under single personal GitHub accounts, not organizations. A first
  pass using `WebFetch` to summarize each repo's GitHub page described both
  as "an established ML research/engineering project" and "a mature,
  actively-maintained production-ready system with institutional backing" —
  language that reads as a reputation signal but is generated from the same
  inflated counters; it is recorded here as a cautionary example of how an
  automated summary can launder exactly the signal this check needs to be
  skeptical of, not as independent confirmation of trustworthiness.
- **Prerequisites:** none to observe the anomaly — it is visible today from
  public repository metadata. Exploiting it would require the `kev` (or, for
  the benchmark, `laya`) maintainer account to turn hostile, or an attacker
  to take it over; this audit did not attempt that and found no evidence
  either account already has.
- **Impact if the underlying trust assumption fails:** `kev` is the only
  model behind the `ai` permission mode in production (`specs/022`, rule 2
  and 6) — the model that autonomously allows or denies a coding agent's
  tool calls once a repository selects that mode. A future pin bump to a
  commit the maintainer published in bad faith, or a takeover of that
  account between review and merge of such a bump, would let that commit's
  training code, serving code, or published weights/adapters bias the
  allow/deny decision however the attacker wants, inside Ariadne's own
  process boundary, for every repository on `ai` mode.
- **Severity rationale:** high impact if it ever materializes (it sits
  behind a security control, not a feature), but the audit found no
  evidence it has materialized — the pinned commit's own `pyproject.toml`
  and a code search of the whole repository show nothing unusual (see
  Counter-evidence). Reported as a provenance/trust-process finding, not a
  vulnerability in the pinned commit itself.
- **Confidence:** confirmed for the metrics (`gh api`, not inferred);
  unverified candidate for "these metrics indicate manipulation" — a
  three-to-four-week-old repo with these counts is consistent with inflated
  engagement but is not proof of it, and the audit did not attempt to
  obtain platform-side evidence (e.g. GitHub's own abuse signals, which this
  task has no access to).
- **Counter-evidence:**
  - `jaredpalmer/kev`'s pinned commit's `pyproject.toml` declares ordinary,
    well-known dependencies (`torch`, `transformers`, `peft`, `accelerate`,
    `datasets`, `numpy`, `scikit-learn`, `fastapi`, `uvicorn`) under the
    Apache-2.0 license, with no `trust_remote_code`, `torch.load`, or
    `safetensors` literal anywhere in the repository (`gh api search/code`,
    0 hits for all three) — i.e. nothing in the code itself does custom
    pickle loading or opts into Hugging Face's "run the repo's own Python"
    feature. Weight loading is delegated to `transformers`/`peft`, which
    default to safetensors and a `weights_only=True` fallback on the torch
    versions Kev requires (`torch>=2.6`).
  - `specs/022-ai-permission-mode.md`'s "Sources" section already flags the
    name-collision risk with a different, unrelated PyPI package also
    called `kev`, and the install path never does `pip install kev` bare —
    always the explicit `git+https://github.com/jaredpalmer/kev@<sha>` URL
    (`install.rs:295`) — so dependency confusion on the package name is
    already closed.
  - Every production pin is a full 40-character commit/revision hash,
    checked by `flavours.rs`'s own test; a hash pin is immutable under
    git's content addressing regardless of what the account does after the
    pin was taken, short of a practical SHA-1 collision.
- **What would invalidate this finding:** independent confirmation that the
  stars/forks reflect organic adoption (e.g. a well-known org's public
  adoption announcement, security-researcher writeups, or GitHub's own
  trust signals if obtained through an authorized channel) would downgrade
  this to a non-finding; conversely, any report of the `kev` or `laya`
  account being compromised, or of secrets/backdoors in a past commit,
  would raise it.
- **Recommendation (process, not code):** treat star/fork counts as
  non-evidence for this subsystem specifically, because it sits behind a
  security control. Before taking any future pin bump for `kev`, diff the
  new commit against the old pinned one by hand (not just trust the
  maintainer's tag), and consider vendoring or mirroring the pinned source
  tree so a future takeover of the upstream account cannot silently change
  what a "refresh" re-downloads. No regression test can enforce a social
  trust judgment; this is a design question for the user, not a remediation
  task.

#### SUPPLY-003 — `shadcn` is listed as a production dependency but is dev-only CLI tooling, and carries the UI's only high/critical npm findings

- **Affected:** `ui/package.json:40` (`"shadcn": "^4.18.0"` under
  `"dependencies"`), `ui/package-lock.json`.
- **Evidence:**
  - `npm audit --omit=dev --json` against `ui/package-lock.json`: 17
    vulnerabilities in the declared "prod" tree (1 critical, 12 high, 4
    moderate; `npm audit` (full) adds 3 more dev-only ones, including a
    critical in `proxy-addr`).
  - Chain reconstruction from `ui/package-lock.json`'s `packages` map (see
    Tested) traces every high/critical finding to one root:
    `shadcn → {@modelcontextprotocol/sdk → {hono, express → {body-parser →
    qs, proxy-addr}}, undici, ts-morph → @ts-morph/common → fast-glob →
    {micromatch → braces}}`. `undici` is additionally pulled by the
    devDependency `jsdom` (test-only) and by `@dotenvx/dotenvx`.
  - `proxy-addr 2.0.7` (critical: IP spoofing via IPv4-mapped IPv6 trust
    subnet), `undici 7.29.0` (high: three DoS/response-splitting advisories),
    `hono 4.13.2` (moderate), `qs 6.15.3`, `ip-address 10.5.0` (moderate:
    SSRF/trust-boundary bypass via `express-rate-limit`'s dependency,
    `socks`), `fast-glob`/`micromatch`/`braces`/`ts-morph`/`@ts-morph/common`
    (high, via `shadcn`'s own `ts-morph` dependency).
  - `grep -rn "from ['\"]shadcn" ui/src/` and a scan of every `npm run`
    script in `ui/package.json`: zero hits. `shadcn` is never imported by
    application source and never invoked by a build, dev, or test script;
    it is the shadcn/ui component-generator CLI, run manually (`npx shadcn
    add ...`) per its own convention, with `components.json` as its config
    file.
- **Impact if misclassified:** none at runtime — Vite only bundles
  statically imported modules, so none of `express`, `proxy-addr`, `hono`,
  `qs`, `ip-address`, `fast-glob` reach the shipped Tauri desktop app or
  even `npm run dev`'s served app. The actual impact is on supply-chain
  tooling and process: any scanner keyed off `package.json`'s
  `dependencies`/`devDependencies` split (including the `npm audit
  --omit=dev` run this task performed) overcounts production exposure by
  these 14+ findings, which could mask a real production finding in the
  noise or waste remediation effort patching something nothing runs.
- **Severity rationale:** the vulnerabilities themselves are high/critical
  by npm's classification, but severity *to Ariadne* is low — this is a
  classification/hardening finding, not a reachable vulnerability. Recorded
  under Confirmed because the misclassification itself, and the resulting
  chain to those CVEs, are both directly verified facts.
- **Confidence:** confirmed (both the dependency chain and the absence of
  any import/invocation were directly checked, not inferred).
- **Remediation (smallest):** move `"shadcn"` from `dependencies` to
  `devDependencies` in `ui/package.json`. This changes nothing about what
  ships; it only fixes what `npm audit --omit=dev` (and any similarly
  configured scanner) reports as production risk.
- **Regression criterion:** `npm audit --omit=dev --json`'s
  `metadata.vulnerabilities.total` drops to 0 (or to only findings with an
  independently-traced production import path), and `ui/package.json`'s
  `dependencies` key no longer contains `shadcn`.

### Unverified candidates

#### SUPPLY-004 — The model installer places no restriction on what the base model's pinned revision may contain

- **Affected:** `crates/ariadne-daemon/src/ai_permissions/install.rs:300-320`
  (`weights`).
- **Observation:** `snapshot_download` for the adapter is called with
  `allow_patterns=["*.json", "*.safetensors", "*.pt", "*.txt", "*.jinja"]`,
  but the following line, `snapshot_download(repo_id={base:?},
  revision={base_revision:?})`, passes no `allow_patterns` at all — it
  downloads every file the pinned base-model revision contains, whatever
  type.
- **Why only a candidate, not confirmed:** the revision is pinned (a
  40-character hash per `flavours.rs`'s test), so the content at that
  revision cannot be swapped after the fact without changing the hash —
  this is not a "latest wins" exposure like SUPPLY-001. The practical
  question is only whether the base repo's pinned snapshot *already*
  contains something a reviewer would not have wanted fetched (a training
  notebook, an unrelated checkpoint format, a legacy `.bin` pickle file).
  This audit did not download the ~several-GB base weights
  (`Qwen/Qwen3.5-4B-Base` etc.) to inventory their file list, so it cannot
  say whether this is inert or live.
- **Prerequisites to matter:** the base repo's pinned revision would have to
  already contain a file type whose presence or parsing is itself risky
  (e.g. a pickle-format checkpoint parsed by something other than
  `transformers`' safe-loading default).
- **Impact if confirmed:** low-to-moderate — more data than necessary lands
  on disk, and if anything in it is pickle-formatted and something in the
  install or serve path ever parses it directly with raw `torch.load`
  (no evidence of that today; `kev.serve` uses `transformers`/`peft`
  loading, per SUPPLY-002's counter-evidence), it would be a latent
  deserialization surface.
  Base images are the general-purpose Qwen releases, not Kev/Laya-published
  content, which somewhat reduces (but does not remove) the single-point-of-trust
  concern SUPPLY-002 raises.
- **Confidence:** unverified candidate — the restriction gap is confirmed by
  reading the code; whether it currently fetches anything beyond
  `*.safetensors`/`*.json`/config files is not.
- **Remediation (smallest):** add the same `allow_patterns` list to the base
  `snapshot_download` call as the adapter already uses, dropping `"*.pt"`
  from both unless a specific flavour is confirmed to need the legacy
  format (`Qwen3.8-27B` ships full weights rather than a LoRA adapter per
  the Kev repo's description — check whether that flavour's base is
  safetensors-only before dropping `*.pt` workspace-wide).
- **Regression criterion:** a test asserting both `snapshot_download` calls
  in `weights()` pass the same `allow_patterns`, and that the list excludes
  any pattern not reachable by `kev.serve`'s own loading code.

### Hardening suggestions

#### SUPPLY-005 — Two Rust dependencies carry non-blocking RustSec advisories, reachable from production

- **Evidence:** `cargo audit` (v0.22.2, RustSec advisory-db, 1,307
  advisories loaded live from `github.com/RustSec/advisory-db` on
  2026-10-12) found **0 vulnerabilities** (blocking, CVSS-scored findings)
  across both the workspace (`Cargo.lock`, 557 deps) and the Tauri shell
  (`ui/src-tauri/Cargo.lock`, 465 deps). It did report four non-blocking
  warnings:
  - `bincode 1.3.3` — `RUSTSEC-2025-0141`, unmaintained. Pulled by `syntect`
    (workspace `Cargo.toml` comment: used for syntax highlighting in the
    console) into `ariadne-console`, and from there into both
    `ariadne-cli` and `ariadne-daemon` — production-reachable
    (`cargo tree -i bincode@1.3.3`).
  - `chacha20 0.10.1` — yanked from crates.io. Pulled by `rand 0.10.2` into
    `ulid` into `ariadne-core`, and from there into every workspace crate
    (`cargo tree -i chacha20@0.10.1`) — production-reachable. This audit
    could not retrieve the yank rationale: `curl` to `crates.io`'s API
    returned an empty body inside this sandbox while the same tool worked
    against `github.com` and `api.github.com` (recorded as a coverage gap,
    not resolved). A yanked version can still build from an existing
    `Cargo.lock`, but a fresh resolution without this lockfile (e.g. a
    `cargo update`) may no longer be able to select it, which is a
    reproducibility risk independent of whether the yank was security-motivated.
  - `proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`, unmaintained) and
    `glib 0.18.5` (`RUSTSEC-2024-0429`, unsound — a `NULL`-pointer dereference
    class issue in `VariantStrIter`, fixed in `glib >= 0.20.0`) — both in
    `ui/src-tauri/Cargo.lock` only. `cargo tree -i glib@0.18.5 --target all`
    traces it through `wry → webkit2gtk/gio/gtk → glib`, i.e. Tauri's Linux
    WebKitGTK backend; this crate does not appear on the macOS/Windows
    target graphs. Whether Ariadne Desktop's own code ever exercises the
    specific unsound `VariantStrIter` iterator methods was not checked —
    recorded as a gap; the advisory itself has no CVSS score and `cargo
    audit`'s default policy treats both as warnings, not failures.
- **Why hardening, not a vulnerability:** none of the four have an assigned
  CVSS/exploitability rating from RustSec; "unmaintained" and "yanked" are
  maintenance-hygiene signals, and "unsound" here is a crash/UB class issue
  with no demonstrated exploit path from Ariadne's own usage.
- **Remediation (smallest):** for `bincode`, wait on or replace `syntect`'s
  dependency choice upstream, or vendor-pin an alternative if `syntect`
  offers a `bincode`-free feature set; for `chacha20`/`rand`, bump `rand` to
  a release that does not depend on the yanked `chacha20 0.10.1`; for
  `glib`/`webkit2gtk` on Linux, bump the GTK webview stack once `wry`
  publishes a release pulling `glib >= 0.20.0`. None of these are urgent;
  group them into routine dependency maintenance.
- **Regression criterion:** `cargo audit` (both lockfiles) reports 0
  warnings for these four IDs.
- **Confidence:** confirmed for the advisory IDs and the production
  reachability of each; not independently confirmed as currently
  exploitable in Ariadne's own call sites (that would need a dedicated
  reproduction per advisory, out of scope for the time this task had).

### Rejected hypotheses

- **"The Rust lockfiles hide a substituted/typosquatted crate."** Checked:
  both lockfiles resolve every dependency from
  `registry+https://github.com/rust-lang/crates.io-index` or the workspace's
  own `path =` members; `cargo audit` reports the checksum crates.io itself
  recorded for each. No crate in either lockfile uses a git or alternate-registry
  source. Rejected.
- **"`pip install kev[serve] @ git+...` is vulnerable to dependency confusion
  with the unrelated PyPI `kev` package."** Checked: the installer
  (`install.rs:295`) always supplies the explicit `git+https://...@<sha>`
  URL, never a bare `kev` name that PyPI's resolver could satisfy from the
  wrong package, and `specs/022` documents this distinction explicitly.
  Rejected for the production path. (The benchmark's Kev install, `kev[serve]
  @ git+https://github.com/jaredpalmer/kev`, also always names the VCS URL —
  the gap there is the missing commit pin, SUPPLY-001, not package-name
  confusion.)
- **"The model server (`kev.serve`) executes attacker-controlled code via
  Hugging Face's `trust_remote_code`."** Checked via `gh api search/code`
  across the whole pinned `kev` repository: zero occurrences of
  `trust_remote_code`. Rejected, with the caveat that this only covers
  code present in the `kev` repository itself, not every line of
  `transformers`/`peft` it depends on (out of scope — those are themselves
  well-known PyPI packages with their own advisory tracking, not a
  substitution risk specific to Ariadne).
- **"`shadcn`'s vulnerable transitive dependencies ship inside the Tauri
  desktop bundle."** Checked (see SUPPLY-003): no source file imports
  `shadcn`, no build/dev script invokes it, and Vite's bundler only
  includes statically imported modules. Rejected as a runtime
  vulnerability; kept as SUPPLY-003, a classification finding.

## Recommendation

No confirmed vulnerability in this audit lets an attacker substitute a
dependency or model artifact that production code or CI would then execute
unverified. The strongest, most actionable item is SUPPLY-001 (benchmark-only,
but zero-friction to fix: add the commit pin and wheel hash the production
code already demonstrates the pattern for). SUPPLY-002 is the one item that
deserves the user's attention before any future action (a pin bump, or
adding a third flavour source): the project's only security-model dependency
sits on a single maintainer account with a trust signal (stars/forks) this
audit found reason to distrust, even though the pinned commit itself showed
nothing wrong. SUPPLY-003 is a quick, safe classification fix that reduces
noise for every future scan. SUPPLY-004 and SUPPLY-005 are routine hardening.

## Confidence

High confidence in every finding's facts (each cites a command, its output,
or a file and line); the one place confidence is split is SUPPLY-002, where
the metrics are confirmed but what they imply about intent is explicitly
not. See "What would invalidate this finding" there for what would move it
either way.

## Unresolved design questions for the orchestrator

1. Should the benchmark's Laya/Kev pinning (SUPPLY-001 remediation) be a
   follow-up task now, given it only affects a path `bench/AGENTS.md`
   already says never runs in CI?
2. Does the user want a standing process (not a one-time fix) for reviewing
   future `kev` pin bumps against the account/repo trust concern in
   SUPPLY-002 — e.g. vendoring the pinned source tree into Ariadne's own
   infrastructure rather than re-fetching from `jaredpalmer/kev` on every
   install?
3. SUPPLY-004's remediation proposes dropping `"*.pt"` from both
   `allow_patterns` lists; confirm whether `Kev27B`'s base
   (`Qwen/Qwen3.8-27B`, described as shipping full weights rather than a
   LoRA adapter) needs that pattern before a remediation task removes it
   workspace-wide.

## Cross-boundary evidence for RELEASE

None found. This audit's starting sources and work were scoped to package
and model-artifact resolution (`Cargo.*`, `ui/package*.json`,
`ui/src-tauri/Cargo.*`, the AI-permission install/serve code, and
`bench/`), not to GitHub Actions, release artifacts, or application
installation — RELEASE's area per the cross-task contract. No finding here
implicates a GitHub Actions workflow, a release artifact, or the
installer/uninstaller scripts under `scripts/`.
