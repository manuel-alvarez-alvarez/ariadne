# SUPPLY: dependencies and model artifacts

## Question

Can vulnerable or substituted dependencies and model artifacts compromise
Ariadne's builds or runtime execution?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (main, 2026-10-12 21:32:41 +0200),
checked out at `/Users/malvarez/.ariadne/worktrees/q4r9v3t3/w3anyvzc-eng` on
branch `audit-third-party-packages-and-model-anyvzc`. Unchanged across every
revision of this report.

Scan date for every advisory/metadata check below: 2026-10-12. RustSec
advisory-db at commit `57dcc0f2192e025138907a495c1ac9a7e1e3e123`
(2026-10-12 20:06:48 +0200), 1,307 advisories, via `cargo-audit` 0.22.2.

## Finding identifiers are stable across revisions

Per the review of commit `0e664891`: once an ID is assigned to a claim, it
keeps referring to that claim, even if the claim is withdrawn — it is never
reassigned to a different, later finding. This revision's identifiers:

- **SUPPLY-001** — benchmark Laya/Kev installs unpinned and unverified.
- **SUPPLY-002** — upstream trust profile of `kev`/`laya` (process concern).
- **SUPPLY-003 — retired.** The commit-`c2d88566` claim that `shadcn` is
  misclassified as a production dependency. Withdrawn in that same revision;
  stays retired here, under this ID, rather than being reused.
- **SUPPLY-004** — base-model `snapshot_download` has no `allow_patterns`.
- **SUPPLY-005** — four non-blocking RustSec warnings.
- **SUPPLY-006** — npm advisories outside shadcn's unreached CLI chain.
- **SUPPLY-007** (new this revision) — `torch`'s `weights_only` unpickler CVE
  within Kev's pinned dependency range. This was previously, incorrectly,
  filed under a reused "SUPPLY-003" in commit `0e664891`; it gets its own,
  previously-unused number here instead.
- **SUPPLY-008** (new this revision) — `bench/ai-permissions/run.sh`'s
  default cache/venv home collides with the production AI-permissions home.
- **SUPPLY-009** (new this revision) — the installer does not detect or
  repair a virtual environment left without `pip`.

## Corrections since the reviewed version (commit 0e664891)

The reviewer found one more script defect and five additional coverage
gaps, on top of what the previous revision already fixed. What changed:

- **The SUPPLY-001 reproduction had a real bug, not flaky git/pip
  behavior.** Every "check installed content" step ran `python -c "import
  demo_pkg2"` while the shell's current directory was still the *source
  checkout* used to build that same package — Python's `-c` mode prepends
  the current directory to `sys.path`, so the import silently read the
  working tree's file, not anything pip installed. This explains the
  inconsistent results the previous revision disclosed and mis-attributed
  to a `git maintenance --auto` race. That explanation is now removed from
  both the script and this report. The script now runs every check with
  `python -I` from a directory holding no package source, and asserts the
  imported module's `__file__` resolves under the venv's own
  `site-packages` before checking its content — see `repro/supply-001-pip-integrity.sh`
  and the regenerated, now fully deterministic (3 consecutive clean runs)
  `repro/supply-001-transcript.log`. Both negative-test steps (hash
  rejection, VCS-hash rejection) now assert the exact exit status and
  diagnostic text, and the script itself fails loudly if either case
  unexpectedly succeeds.
- **The CVE finding is renumbered SUPPLY-007** (previously filed as a
  reused "SUPPLY-003"), per the stable-identifier requirement above. Its
  own claims are also corrected: the earlier wording said both the adapter
  *and* the base-model revision ship `head.pt`; only the adapter does
  (`kev/checkpoint.py`'s `Checkpoint.__init__` reads `head.pt` from
  `self.path`, which is the adapter's resolved directory — the base is a
  separate repo/revision read only by `hybrid_base()`/`load()` for its
  config and weights, never for `head.pt`). The MLX backend
  (`kev/mlx_model.py`) is now read and cited: it loads its own weights via
  `mx.load(...)` (MLX's safetensors reader) and `mlx_lm.utils.load_model`,
  not `torch.load` — but `Checkpoint.__init__` calls `read_meta`
  unconditionally, before any backend is chosen, so the `torch.load(head.pt)`
  exposure is not torch-backend-specific; it is reached by every backend,
  MLX included. The finding now states explicitly that no exploit was run
  and that the "attacker-controlled checkpoint" prerequisite is not
  demonstrated, only named.
- **A real, committed, structured resolution and advisory record** replaces
  the earlier abbreviated commit hash and hand-copied package list:
  `repro/supply-pytorch-evidence/` holds the exact command (full 40-character
  commit, not truncated), platform, pip's `--report` JSON parsed into a
  plain list of all 76 resolved packages, and the full OSV.dev
  `querybatch` result for every one of them — plus an explicit note on what
  this evidence does not cover (other platforms/devices, Python 3.12, Laya,
  build-isolation vs. target-environment placement of `setuptools`).
- **Two new findings, from gaps the review named specifically:**
  **SUPPLY-008** (the benchmark's default cache location is the production
  one) and **SUPPLY-009** (the installer does not detect a `pip`-less venv).
  The previous revision's "reviewed, no finding" verdict on partial-install
  recovery is withdrawn; see SUPPLY-009.
- **SUPPLY-004's wording is corrected**: `.gitattributes`, `LICENSE`, and
  `README.md` are present in every pinned base manifest and are, strictly,
  outside the proposed `allow_patterns` list — "nothing outside the
  patterns" was imprecise. The corrected claim is narrower: no file of a
  *risky* format was found, not that every file already matches the
  proposed allow-list.
- **SUPPLY-006 is substantially corrected** after an independent re-trace
  of `ui/package-lock.json` surfaced three wrong attributions and one
  unverified exploit-path claim in the prior revision:
  - `undici` is reachable through `jsdom` (a devDependency, nested version
    `8.10.0`) in addition to `shadcn`'s chain — the prior revision
    mentioned this once but it was lost in the rewrite; restated precisely.
  - `fast-uri` is reachable only via the plain `ajv@8.20.0` package (itself
    reached through `@modelcontextprotocol/sdk` and, separately, through
    `conf` — see next point — both ultimately `shadcn`-rooted), **not**
    through `@redocly/openapi-core`, which depends on a *different*,
    scoped package, `@redocly/ajv@8.11.2`, whose own dependency is
    `uri-js-replace`, not `fast-uri`. The prior revision conflated the two
    `ajv`s.
  - `conf`'s parent is `@dotenvx/dotenvx`, not "likely `@tauri-apps/cli`" as
    guessed previously. Traced one level further: `@dotenvx/dotenvx` is
    itself `shadcn`'s own dependency, so this chain is `shadcn ->
    @dotenvx/dotenvx -> conf -> ajv@8.20.0 -> fast-uri` — `shadcn`-rooted
    and unreached, like the rest of that cluster, not a mystery dev-tool
    path.
  - The `js-yaml`-via-`gen:api` exploit path is **withdrawn as unverified**:
    `ui/scripts/gen-api.mjs`'s `fetchSpec` calls `res.json()` (the
    platform's built-in JSON parser) and its local-file path calls
    `JSON.parse`; in both cases a plain JavaScript object is handed to
    `openapiTS()`. No YAML parsing of the daemon's (or any attacker's)
    response happens in this script, so `js-yaml` being present in
    `@redocly/openapi-core`'s own dependency tree does not, by itself, show
    this script ever invokes it on untrusted input. `js-yaml`'s
    CPU-exhaustion advisory is kept in the npm advisory inventory (it is a
    real resolved dependency with a real advisory) but moved from
    "confirmed reachable" to "present in the tree, no verified call path to
    attacker-controlled input found."
  - The claim "every `shadcn`-chain fix needs a major bump" is corrected:
    the npm audit data itself contradicts it for several packages in that
    same chain (`proxy-addr`, `undici`, `hono`, `qs`, `ip-address`,
    `postcss-selector-parser` all show a plain `fixAvailable: true` — a
    same-major-version bump resolves them without moving `shadcn`). Only
    the `ts-morph`/`@ts-morph/common`/`fast-glob`/`micromatch`/`braces`
    sub-cluster needs the major `shadcn` bump.
- **The "inflated counters" wording is removed from Threat assumptions**,
  which was asserting exactly the certainty SUPPLY-002 explicitly disclaims.

## Corrections since the reviewed version (commit a829ebf1)

- **The round-2 rejection of a `js-yaml`/`gen:api` exploit path was itself
  incomplete, not wrong about what it checked.** It correctly showed
  `gen-api.mjs`'s own JSON parsing of the *root* document never reaches a
  YAML parser, but did not follow `openapi-typescript`'s own bundling step,
  which resolves **external `$ref`s** through `@redocly/openapi-core` and
  parses whatever each one points at with a YAML parser that has
  merge-key support explicitly enabled — independent of the root
  document's own format. A bounded, harmless reproduction
  (`repro/supply-006-yaml-ref/`) now confirms this mechanism is reached;
  SUPPLY-006 states precisely what is and is not proven by it (parser
  reachability, not `GHSA-2883-xcg3-v3hh`'s specific resource-exhaustion
  behavior), and the prerequisite that narrows it (an externally-referenced,
  untrusted document — not the daemon's own default, self-contained one).
  The same trace surfaced `brace-expansion@2.1.4`'s real chain (via
  `@redocly/openapi-core -> minimatch`, the same `gen:api` devDependency
  path, not `shadcn`), which the previous revision's inventory omitted
  entirely.
- **The npm advisory inventory is now complete and committed as evidence**
  (`repro/supply-006-npm-evidence/`): every package's exact resolved
  version(s), every GHSA id (deduplicated, `semver`-checked against the
  exact resolved node, not assumed), its affected range, and its fix —
  including `@modelcontextprotocol/sdk`'s own direct advisory
  (`GHSA-6qxp-vccf-f47h`) and `brace-expansion`'s three, neither recorded
  in the previous revision — replacing vague counts like "nine GHSAs" (the
  real count for `undici` is 11) and prose that distinguished "major bump
  needed" from "plain fix available" only in general terms.
- **The SUPPLY-001 reproduction now asserts `direct_url.json`'s
  `commit_id`**, not just prints it, and states explicitly what it does
  and does not prove (pip/git behavior against synthetic stand-ins, not
  `run.sh`'s own branches or a real Laya install).
- **Two evidence-accuracy corrections in `repro/supply-pytorch-evidence/`:**
  the resolved-package count was 76, not 75 (a counting error in the
  previous revision's README, now fixed everywhere it appeared); and
  `setuptools`'s presence in that list is `torch`'s own runtime dependency
  (`dryrun-stdout.log:41`), not Kev's build-system requirement as an
  earlier draft of the README guessed. The OSV query's raw HTTP response is
  now committed separately from its derived per-package summary, correctly
  labeled as two different things.
- **SUPPLY-005 is restated with full detail inline** (the fix version for
  `glib`, `>= 0.20.0`, had been dropped when the previous revision
  condensed this section by reference to an earlier one — every revision
  now carries its own complete remediation detail rather than pointing a
  reader backward).
- **An unrelated test failure surfaced by this review's own
  `.githooks/pre-push --all` run is recorded here, not fixed:**
  `scheduler_attention::an_agent_that_reports_nothing_is_flagged_and_then_relaunched`
  (`crates/ariadne-daemon/tests/it/scheduler_attention.rs:1855`) failed in
  that run. This audit's diff touches only the assigned report directory
  and cannot affect that test; the failure is outside this task's scope per
  the review's own instruction ("This audit does not modify the failing
  test or application... Do not add application repairs to this
  report-only task") and is reported to the orchestrator for separate
  handling, not altered here. The full Rust/UI check suite otherwise passed
  in this revision's own run (1926/1926 tests, clippy, fmt, `npm run check`,
  Tauri build) before this one test's failure was found by the reviewer's
  independent run — recorded as the actual, current state rather than
  claimed clean.

## Threat assumptions

- Installed coding-agent CLIs are trusted; this task does not attack them.
- Repository content, forge content, network responses, and third-party
  package/model registries are treated as potentially hostile.
- Documented permission or pinning guarantees are checked even for trusted
  installed agents.
- No attack is run against the user's live daemon, repositories, accounts,
  or external services; reproductions use disposable clones, local venvs,
  synthetic git remotes and wheels, and read-only lookups against public
  registries (crates.io, npm, GitHub, Hugging Face, PyPI, OSV.dev).
- External technical claims are taken from primary sources — `cargo audit`
  against the RustSec advisory-db, `npm audit` against the npm registry, a
  direct clone of the pinned upstream source, the Hugging Face and PyPI
  metadata APIs, OSV.dev, and pip's own documented and directly observed
  behavior — not from a web-page summary alone. A first pass using a
  summarizer tool to describe `jaredpalmer/kev` and `NandhaKishorM/laya`
  produced confident, favorable prose ("an established ML research
  project", "institutional-grade") built on counters this report treats
  with skepticism but does not claim are manipulated (see SUPPLY-002); it
  is recorded as a cautionary example of over-trusting a tool's narrative
  summary, not as evidence either way about the repositories themselves.

## Coverage

### Reviewed

- `Cargo.toml`, `Cargo.lock` (workspace, 557 crate dependencies).
- `ui/src-tauri/Cargo.toml`, `ui/src-tauri/Cargo.lock` (465 crate
  dependencies, independent tree, excluded from the workspace).
- `ui/package.json`, `ui/package-lock.json` (718 npm packages: 537 prod, 181
  dev, per `npm audit`'s own classification), `ui/src/index.css`,
  `ui/AGENTS.md`, `ui/scripts/gen-api.mjs` (read in full).
- `ui/node_modules/openapi-typescript/dist/{index,lib/redoc}.mjs` and
  `ui/node_modules/@redocly/openapi-core/lib/{resolve.js,js-yaml/index.js}`
  — the installed dependency code itself, not just its `package.json`
  metadata, to trace exactly how (and whether) an external `$ref` reaches
  a YAML parser (SUPPLY-006).
- `crates/ariadne-daemon/src/ai_permissions/{install,flavours,server,python,mod,decide,hardware}.rs`.
- `crates/ariadne-daemon/src/config.rs` (the `~/.ariadne` home-directory
  default, for SUPPLY-008).
- `bench/ai-permissions/run.sh`, `bench/ai-permissions/run.py`,
  `bench/ai-permissions/evaluators/{kev,laya}/*.py`,
  `bench/ai-opportunities/progress/bench_progress/kev_runner.py`,
  `bench/AGENTS.md`.
- `specs/022-ai-permission-mode.md` (full), `specs/024-advisory-failure-diagnosis.md`
  (scope and behavior sections).
- **A full local clone of `jaredpalmer/kev` at the pinned commit**
  `f1535963cea021439370c23127bc970b6788e730` (`git clone` + `git checkout`,
  not GitHub's code-search index, which does not reliably index a
  historical commit). `kev/checkpoint.py` and `kev/mlx_model.py` read in
  full this revision (the latter was named by the review as a missed
  production path); repository-wide `grep` for `torch.load`, `torch.save`,
  `weights_only`, `trust_remote_code`, `pickle`, `safetensors`.
- The pinned commit's own `pyproject.toml`.
- GitHub repository metadata (`gh api repos/...`) for `jaredpalmer/kev` and
  `NandhaKishorM/laya`.
- Hugging Face model metadata (`huggingface.co/api/models/.../revision/...`)
  for all four pinned flavours' adapter and base repos: file manifests,
  `library_name`, author, download/like counts — no weights downloaded.
- PyPI metadata for every direct runtime/serve dependency in the pinned
  `pyproject.toml`.
- `crates/AGENTS.md`, `bench/AGENTS.md`.

### Tested

- `cargo audit` (v0.22.2, RustSec advisory-db, live-fetched) against both
  Rust lockfiles.
- `npm audit --omit=dev` and `npm audit` (full) against
  `ui/package-lock.json`, with every advisory's GHSA id, affected range,
  and fix availability extracted.
- **`npm ci` in `ui/` (installing `ui/node_modules` for the first time this
  audit) and `npm ls <pkg> --all`** for every vulnerable package name, to
  get exact lockfile-node paths and resolved versions directly from npm's
  own dependency resolver rather than by reading `package-lock.json` by
  eye — this is what surfaced the precise `undici`/`jsdom`,
  `fast-uri`/`ajv`-vs-`@redocly/ajv`, `conf`/`@dotenvx/dotenvx`, and
  `brace-expansion` chains in SUPPLY-006.
- A small Node script (logic described inline in SUPPLY-006, output
  committed at `repro/supply-006-npm-evidence/resolved-advisory-table.txt`)
  cross-referencing every `npm audit --json` advisory's affected-version
  range against each resolved node's exact version with the `semver`
  package already present in `ui/node_modules`, rather than assuming a
  range applies.
- `npm` registry metadata for `shadcn`'s `exports` map (`./tailwind.css` is
  a static-asset export with no JS).
- **A bounded, harmless reproduction of external-`$ref` YAML parsing**
  through the real, installed `openapi-typescript`/`@redocly/openapi-core`
  (`repro/supply-006-yaml-ref/run.mjs`, re-run three consecutive times,
  deterministic) — see SUPPLY-006 for what it proves and what it does not.
- `cargo tree -i <crate>` for every crate named in a `cargo audit` warning,
  re-verified this revision (not only carried forward by citation).
- **The full `.githooks/pre-push --all` suite**, run in the foreground with
  its output kept outside the worktree. It reached and passed `cargo
  machete`, `cargo build --workspace --all-targets`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
  `scripts/check-unused-rust`, `npm ci`, `npm run check`, and the Tauri
  shell's `cargo fmt`/`cargo check`; `cargo nextest run --workspace
  --profile ci` reached 1926/1926 passing in this run, but the review's own
  independent run of the same hook found one test,
  `scheduler_attention::an_agent_that_reports_nothing_is_flagged_and_then_relaunched`,
  fail — recorded in the Corrections section above and reported to the
  orchestrator, not fixed here (this task's diff cannot affect that test).
- **A real `pip install --dry-run --report` resolution**, using the full,
  untruncated pinned commit hash, of
  `kev[serve] @ git+https://github.com/jaredpalmer/kev@f1535963cea021439370c23127bc970b6788e730`,
  on Python 3.13, pip 26.2, macOS 26.6.2/arm64, 2026-10-12, with default pip
  configuration (no custom index). The structured `--report` JSON (not the
  scraped stdout) was parsed into the committed package list. Full evidence,
  including an explicit list of what it does not cover, is committed at
  `repro/supply-pytorch-evidence/`.
- An OSV.dev `querybatch` of all 76 resolved packages at their resolved
  versions; the raw response is recorded at
  `repro/supply-pytorch-evidence/osv-querybatch-raw-response.json`, and a
  readable per-package view at
  `repro/supply-pytorch-evidence/osv-results-normalized.json`.
- **A corrected, deterministic, independently re-runnable reproduction**
  for SUPPLY-001 — `repro/supply-001-pip-integrity.sh` — re-run three
  consecutive times this revision, each a clean exit 0 with every
  assertion passing; transcript at `repro/supply-001-transcript.log`.
- **An independent reproduction of the SUPPLY-009 partial-install gap**:
  `python3.13 -m venv --without-pip <dir>` followed by `<dir>/bin/python -m
  pip --version`, which fails with `No module named pip` while the
  interpreter itself still reports a supported version.
- A manual trace of `bench/ai-permissions/run.sh:17`'s `AI_HOME` default
  against `crates/ariadne-daemon/src/config.rs:92-97`'s and
  `crates/ariadne-daemon/src/ai_permissions/mod.rs:96`'s home-directory
  resolution, confirming both default to the same path unless overridden.

### Excluded, with reason

- Actually installing the Kev/Laya virtual environments and downloading
  model weights (multi-gigabyte, and most flavours need hardware this
  sandbox does not have).
- `crates/ariadne-daemon/tests/it/ai_permissions*.rs`: read for context, not
  re-run (they exercise a stub installer, not the real `pip`/Hugging Face
  path).
- GitHub Actions / release pipeline, release artifact signing, and the
  installer scripts under `scripts/` — RELEASE's area.
- `docs/` and `README.md`.
- `kev/full_ft.py`'s training-only deserialization path
  (`torch.load(..., weights_only=True)` explicitly, for `kev.train
  --init_from`, which Ariadne's daemon never invokes — `server.rs` only
  ever runs `kev.serve`).
- Linux CPU/CUDA and Python-3.12 resolutions of the pinned Kev install, and
  a similar dry-run/OSV scan of Laya's own dependency tree — named
  explicitly as gaps in `repro/supply-pytorch-evidence/README.md` rather
  than silently assumed equivalent to the one resolution this audit did
  run.
- Build-isolation vs. target-environment placement of `setuptools` in the
  resolved package set — flagged as unresolved rather than asserted either
  way.

## Findings

### Confirmed

#### SUPPLY-007 — Kev's pinned torch dependency range is entirely within the affected range of a high-severity `torch.load` deserialization CVE

- **Affected:** `kev/checkpoint.py:75-76` (`read_meta`, calling
  `torch.load(f"{run}/head.pt", map_location="cpu")` with no explicit
  `weights_only` argument), at the pinned commit
  `f1535963cea021439370c23127bc970b6788e730`;
  `crates/ariadne-daemon/src/ai_permissions/install.rs:25` (`TORCH: &str =
  "torch>=2.6,<2.9"`), repeated by the pinned commit's own `pyproject.toml`.
- **The sink is real, and reached by every backend, not only the torch
  one.** `kev/checkpoint.py:167-170` (`Checkpoint.__init__`) calls
  `self.meta = read_meta(self.path)` unconditionally, **before** any
  backend is chosen — `self.backend(device, opts)` and `self.load(...)`'s
  dispatch to `_load_torch`/`_load_mlx` run afterward. `kev/mlx_model.py`
  (read in full this revision) confirms the MLX backend loads its own
  weights via `mx.load(str(adapter_dir / "adapter_model.safetensors"))`
  (MLX's native safetensors reader) and `mlx_lm.utils.load_model` — neither
  calls `torch.load` — but this is irrelevant to the exposure, because
  `read_meta`'s `torch.load(head.pt)` already ran in `__init__`, for every
  checkpoint, on every backend, before `_load_mlx` or `_load_torch` is even
  selected. The module's own docstring (lines 1-11) states that
  `kev.serve` — the process `crates/ariadne-daemon/src/ai_permissions/server.rs:218-225`
  launches on every model start (`entry_point`, `-m kev.serve`, on every
  device including `mlx`, per `server.rs`'s `device_env`) — "goes through
  it".
- **Corrected from the previous revision: only the adapter, not the base,
  carries `head.pt`.** `self.path` (read by `read_meta`) is the resolved
  *adapter* directory (`resolve_run(run)` where `run` is
  e.g. `jaredpalmer/kev-4b@<adapter_revision>`); the base model
  (`self.meta.base`/`self.meta.base_revision`) is a separate repo read only
  for its config and weight shards in `hybrid_base()`/`_load_torch`/
  `_load_mlx`, never for `head.pt`. All four adapter revisions' Hugging
  Face file manifests (fetched this and the previous revision) confirm
  `head.pt` is present in each; none of the four base manifests contain it
  or any other `.pt`/`.bin` file (see SUPPLY-004).
- **The vulnerable version range is unavoidable under Kev's current pin.**
  `torch.load`'s `weights_only` parameter has defaulted to `True` since
  PyTorch 2.6 (PyTorch 2.6.0 release notes, primary source,
  `github.com/pytorch/pytorch/releases/tag/v2.6.0`: "We are closing the
  loop on the deprecation that started in 2.4 and flipped `torch.load` to
  use `weights_only=True` by default"). **CVE-2026-24747**
  (`GHSA-63cw-57p8-fm3p`, `PYSEC-2026-2286`, CVSS 3.1
  `AV:N/AC:L/PR:N/UI:R/S:U/C:H/I:H/A:H` = 8.8 High,
  `github.com/pytorch/pytorch/security/advisories/GHSA-63cw-57p8-fm3p`,
  `api.osv.dev/v1/vulns/PYSEC-2026-2286`) is a vulnerability *in the
  `weights_only` unpickler itself*: "a vulnerability in PyTorch's
  `weights_only` unpickler allows an attacker to craft a malicious
  checkpoint file (`.pth`) that, when loaded with `torch.load(...,
  weights_only=True)`, can corrupt memory and potentially lead to arbitrary
  code execution." Fixed in **torch 2.10.0**. Kev's pin
  (`torch>=2.6,<2.9`) excludes 2.10.0 entirely.
- **Confirmed by a real, committed resolution, not an estimate or an
  abbreviated citation.** `pip install --dry-run` of the exact, full,
  40-character pinned commit resolves `torch==2.8.0` (full command,
  platform, and structured output committed at
  `repro/supply-pytorch-evidence/`, not reproduced abbreviated here). An
  OSV.dev query for `torch==2.8.0` returns 8 advisories;
  `PYSEC-2026-2286`/`CVE-2026-24747` is the only one of the 8 whose
  affected function (the `weights_only` unpickler) Kev's own code
  demonstrably calls — the other 7 (memory corruption in
  `torch.lstm_cell`, `torch.jit.script`, `unpack_sequence`;
  DoS/overflow in `torch.linalg.lu`, `torch.rot90`/`torch.randn_like`,
  `torch.nan_to_num`; a `pt2`-loading-handler issue scoped to 2.10.0 itself)
  need an attacker to control arguments to those specific tensor operations
  directly, which Kev's fixed scoring API does not expose. Every one of the
  other 75 resolved packages returned **zero** OSV.dev advisories (full
  raw response: `repro/supply-pytorch-evidence/osv-querybatch-raw-response.json`;
  readable view: `repro/supply-pytorch-evidence/osv-results-normalized.json`).
- **What this finding does not claim, stated explicitly per review.** No
  isolated exploit of CVE-2026-24747 was run against any resolved package
  in this audit — the evidence above establishes that the vulnerable
  function is in the dependency tree and is reached by `read_meta` on
  every checkpoint load, not that a working exploit payload was built or
  tested. The finding also does not claim that producing an
  attacker-crafted `head.pt` and getting it into the install path (the
  realistic path SUPPLY-001/002/008 describe) has been demonstrated end to
  end — only that *if* that prerequisite is met, there is no version of
  torch within Kev's declared range that would stop it.
- **Severity rationale:** CVSS 8.8 (High) is the upstream rating for the
  underlying CVE; this report rates the finding itself High rather than
  Critical because exploitation additionally requires the unproven
  attacker-controlled-checkpoint prerequisite above.
- **Impact:** memory corruption and potential arbitrary code execution in
  the daemon-managed `kev.serve` child process, were that prerequisite met,
  on every device backend, every time the model installs, refreshes, or
  restarts. This upgrades the stakes of SUPPLY-002's trust question past
  "could bias a permission decision" to "could execute code."
- **Confidence:** confirmed for the CVE, its affected range, its fix
  version, the resolved `torch` version, and the call-site reachability
  (all primary-sourced or directly read from the pinned source, not
  estimated). Explicitly not confirmed, and not claimed: that any
  currently-pinned checkpoint file is itself malicious, or that the
  memory-corruption behavior was independently reproduced.
- **What would invalidate or change this finding:** a Kev release raising
  its torch ceiling to `>=2.10.0` (requires upstream action, outside
  Ariadne's control to force); evidence the fix was backported to a
  2.6–2.8 point release after this scan date (not found as of 2026-10-12).
- **Remediation (smallest):** an upstream (`jaredpalmer/kev`)
  dependency-range problem, not something `install.rs` can fix alone.
  Ranked by how little they touch: (1) ask upstream Kev to widen its torch
  ceiling past 2.10.0 once validated, and bump `install.rs:25`'s `TORCH`
  constant to match; (2) have Ariadne's installer `pip install --upgrade
  torch>=2.10` *after* `kev[serve]`, accepting the risk of running Kev on
  an unvalidated torch release; (3) accept the risk, bounded by the
  existing commit-pin trust chain, and revisit when Kev's own ceiling
  moves. See **Unresolved design questions**.
- **Regression criterion:** the resolved `torch` version in a fresh `pip
  install --dry-run` of the pinned Kev command no longer appears in an
  OSV.dev query, or appears without `PYSEC-2026-2286`/`CVE-2026-24747`.

#### SUPPLY-001 — The benchmark's Laya and Kev installs are unpinned and unverified, unlike production

- **Affected:** `bench/ai-permissions/run.sh:21-22,98-147`.
- **Execution model.** A **wheel** install (Laya) does not run a build
  backend — pip unpacks a wheel directly; the execution sinks are later,
  at import/launch time, which `run.py`'s evaluator registry and `run.sh`'s
  per-evaluator invocation reach on every benchmark run. A **source/VCS**
  install (Kev, unpinned) does run a build backend
  (`setuptools.build_meta`) immediately, under the invoking developer's own
  user.
- **Prerequisites.** Exploiting this needs control over what the GitHub
  releases API or default-branch tip currently serves for the relevant
  repository — account control, release-process control, or a demonstrated
  platform compromise. HTTPS transport is not the gap; trusting whatever
  the legitimate account currently serves, with no pin and no hash, is.
- **Isolated, deterministic, fully-asserted reproduction** (`repro/supply-001-pip-integrity.sh`,
  transcript at `repro/supply-001-transcript.log`, re-run three consecutive
  times this revision with identical clean results):
  - **Unpinned mutability**, asserted on the imported module's `__file__`
    (confirmed under the venv's own `site-packages`, not a working tree)
    and its content: install 1 returns `benign-v1`; after a push to the
    remote's default branch, an otherwise identical install 2 returns
    `SWAPPED-v2-malicious`.
  - **Commit-pinning holds**, asserted the same way: pinned to the first
    commit's SHA, the install returns `pinned-benign-v1` even after a
    second commit changes the remote's default branch — and
    `direct_url.json`'s `vcs_info.commit_id` matches the pinned SHA
    exactly. (The previous revision's attempt at this same assertion was
    unreliable; see Corrections above for the root cause and fix.)
  - **Hash-checking catches a substituted artifact**, with an explicit
    assertion on the exit status and refusal text, not just a transcript a
    human has to read: a wheel is installed via `--require-hashes` against
    its recorded sha256 (succeeds), the wheel at the same path is replaced
    with different content, and the identical command is asserted to fail
    with `THESE PACKAGES DO NOT MATCH THE HASHES...`.
  - **VCS requirements cannot be hash-checked**, asserted the same way:
    `--require-hashes` against a `git+` URL is asserted to fail, citing
    pip's own message, matching `pip.pypa.io/en/stable/topics/secure-installs/`.
- **Severity rationale:** high if exercised, capped by scope:
  `bench/AGENTS.md` states this path never runs in CI; `grep -ril laya
  crates/ specs/` confirms it never appears outside `bench/`.
- **Confidence:** confirmed — every claim in this finding now has a
  deterministic, asserted, independently re-runnable reproduction, with no
  remaining caveat about an unreliable sub-test.
- **Remediation (smallest):** pin `KEV_REPOSITORY` to a commit SHA, the
  same way `install.rs::KEV_COMMIT` does; replace `latest_laya_wheel`'s
  lookup with a committed wheel URL and a `--hash=sha256:...` line in a
  requirements file passed to `pip install --require-hashes` (the exact,
  proven syntax in `repro/supply-001-pip-integrity.sh`'s `requirements.txt`
  generation — a bare `--hash` flag on the `pip install` command line is
  not the right form; it must be a per-requirement option, as pip's own
  docs and this reproduction's working example both show).
- **Regression criterion (corrected — not a literal-string check):** a test
  that, like `repro/supply-001-pip-integrity.sh`'s Part B, builds or
  designates a known-good artifact, records its hash in `run.sh`'s
  requirements, substitutes different content at the same path, and asserts
  `pip install --require-hashes` refuses it. A literal-string grep for
  `releases/latest` can catch a regression of *intent* but cannot, by
  itself, prove the integrity control actually rejects substituted content;
  this is why the reproduction script asserts behavior, not just absence of
  a string.
- **Compatibility decision for the user:** pinning means a human bumps
  Laya's wheel by hand to pick up a new release, matching what Kev already
  requires.

### Unverified candidates, reclassified as process hardening

#### SUPPLY-002 — The production AI-permission model's sole upstream has an anomalous trust profile

- **What is confirmed, and only this:** `jaredpalmer/kev` and
  `NandhaKishorM/laya` (`gh api repos/...`) were both created 2026-09-17/18
  and as of 2026-10-12 show 9,001 stars / 599 forks and 32,542 stars /
  2,906 forks respectively, under personal GitHub accounts.
- **What is not confirmed, and is not claimed:** that these counts are
  inflated or manipulated. This audit has no platform-side abuse signal
  that would distinguish rapid organic growth from manipulated engagement;
  the counts are a reason for extra scrutiny of *future* changes from this
  upstream, not evidence about the current pinned commit.
- **Why a hash pin already bounds the realistic attack surface.** Every
  production pin (`flavours.rs:26-49`, `install.rs:20`) is a full
  40-character commit hash (`flavours.rs`'s own test,
  `every_pin_is_a_full_sha_and_4b_matches_the_install`). An account
  takeover *after* a commit was pinned cannot retroactively change what
  that pinned SHA resolves to. The place this trust question actually
  bites is the moment a human takes a **new** pin.
- **Why this is lower priority than SUPPLY-007:** SUPPLY-007 is a concrete
  problem that exists today in the currently pinned dependency range,
  independent of upstream intent. SUPPLY-002 is a "what if a future pin
  review is sloppy" process concern.
- **Confirmed counter-evidence (unchanged):** the pinned commit's own
  `pyproject.toml` declares ordinary, well-known dependencies; the full
  clone has zero occurrences of `trust_remote_code`; `specs/022` already
  documents and avoids the separate, unrelated PyPI package also named
  `kev`; every pin is a full SHA. `typesafe-sdk` is published by a company
  with its own domain, docs, and GitHub org; `mlx-lm` is Apple's own
  `ml-explore` project.
- **Recommendation (process, not code):** do not treat star/fork counts as
  evidence either way. Diff a future `kev` pin bump against the currently
  pinned commit by hand before taking it. See **Unresolved design
  questions**.

#### SUPPLY-004 — The model installer places no restriction on what the base model's pinned revision may contain

- **Affected:** `crates/ariadne-daemon/src/ai_permissions/install.rs:300-320`
  (`weights`): the adapter `snapshot_download` call passes
  `allow_patterns=["*.json", "*.safetensors", "*.pt", "*.txt", "*.jinja"]`
  (matching Kev's own `kev/checkpoint.py:44` `resolve_run`); the base-model
  call on the next line passes no `allow_patterns`.
- **Corrected claim (per review):** the Hugging Face metadata API confirms
  all four pinned base repos (`Qwen/Qwen3.5-0.8B-Base`, `-4B-Base`,
  `-9B-Base`, `Qwen/Qwen3.8-27B`) contain `.gitattributes`, `LICENSE`, and
  `README.md` — files that do **not** match the proposed `allow_patterns`
  list. "Nothing outside the patterns" (the previous revision's wording)
  was imprecise. The accurate, narrower claim: no base-model revision
  contains a `.pt`/`.bin` checkpoint or any other file this audit
  considers a risky deserialization format; the three files that *are*
  outside the proposed pattern list are small, static, inert metadata
  (a license text, a readme, a git attributes file), not something this
  finding treats as a vulnerability.
- **Impact:** low. Not a reachable exposure today for any pinned revision;
  a latent gap against a future re-pin to a base that ships something
  broader.
- **Confidence:** confirmed inert for the current four pins, and confirmed
  structurally present in the code.
- **Remediation (smallest):** add the same `allow_patterns` list to the
  base `snapshot_download` call (dropping the three harmless files above
  from what gets downloaded is a side effect, not the point). Keep
  `"*.pt"` in the adapter's list — `head.pt` is load-bearing for every
  flavour (SUPPLY-007).
- **Regression criterion:** a test asserting both `snapshot_download` calls
  in `weights()` pass the same `allow_patterns`.

### Hardening suggestions

#### SUPPLY-005 — Four Rust dependencies carry non-blocking RustSec advisories, with build-time and runtime surfaces kept distinct

- `bincode 1.3.3` (`RUSTSEC-2025-0141`, unmaintained, no fix published) —
  production runtime, via `syntect` into
  `ariadne-console`/`ariadne-cli`/`ariadne-daemon` (`cargo tree -i
  bincode@1.3.3`). Remediation: track or replace `syntect`'s own dependency
  choice upstream; there is no drop-in fix on Ariadne's side alone.
- `chacha20 0.10.1` (yanked from crates.io, no RustSec advisory ID — this is
  crates.io's own yank flag, not a CVE) — production runtime, via `rand
  0.10.2 -> ulid -> ariadne-core`, reaching every workspace crate. Yank
  rationale not retrievable (`crates.io`'s API returned an empty body in
  this sandbox; `github.com`/`api.github.com`/`pypi.org` all worked) —
  recorded as an explicit tool/network gap. Remediation: bump `rand` past
  whatever release re-pins a non-yanked `chacha20`.
- `proc-macro-error 1.0.4` (`RUSTSEC-2024-0370`, unmaintained, no fix
  published — the advisory's own suggested alternatives are `manyhow` or
  `proc-macro2-diagnostics`, not a newer release of this crate) —
  **build-time only**, via `gtk3-macros` (a `(proc-macro)` crate per `cargo
  tree`) from `gtk`, reachable only through Tauri's Linux WebKitGTK
  backend. Executes at `cargo build`/`cargo check` time, on a developer's
  or CI's machine, not in the shipped binary. Remediation: depends on
  `gtk-rs`/`wry` moving off it; no action available on Ariadne's side alone
  today.
- `glib 0.18.5` (`RUSTSEC-2024-0429`, unsound — a `NULL`-pointer-dereference
  class issue in `VariantStrIter`'s `Iterator`/`DoubleEndedIterator` impls,
  **fixed in `glib >= 0.20.0`**) — Linux-WebKitGTK-only, via `wry ->
  webkit2gtk/gio/gtk -> glib` (`cargo tree -i glib@0.18.5 --target all`).
  Call-site reachability of the specific unsound `VariantStrIter` methods
  from Ariadne Desktop's own code, or from `wry`/`webkit2gtk`'s use of
  `glib` on its behalf, was not checked and is not claimed either way.
  Remediation: bump the Tauri/WebKitGTK dependency stack once a `wry`
  release pulls `glib >= 0.20.0`; no direct dependency bump is available on
  Ariadne's side alone, since `glib` is pulled transitively.
- **Regression criterion:** `cargo audit` (both lockfiles) reports zero
  warnings for `RUSTSEC-2025-0141`, the `chacha20` yank, `RUSTSEC-2024-0370`,
  and `RUSTSEC-2024-0429`.
- **Confidence:** confirmed — advisory IDs, exact dependency chains, fix
  versions (where published), and which are build-time vs. runtime vs.
  Linux-only were all independently re-verified this revision, not carried
  forward by reference to an earlier revision's text.

#### SUPPLY-006 — npm advisories outside shadcn's unreached CLI chain, with a complete advisory inventory and one corrected reachability verdict

Full evidence this revision: `repro/supply-006-npm-evidence/npm-audit.json`
(the complete, unedited `npm audit --json` output this table is drawn
from — "Commit a sanitized scan snapshot... under the owned directory",
per review) and `repro/supply-006-npm-evidence/resolved-advisory-table.txt`
(every package's exact resolved lockfile node(s)/version(s) cross-checked
against each advisory's affected range with `semver.satisfies`, not
eyeballed — generated by a small script whose logic is described inline
below so the table is independently reproducible from the two committed
JSON files). `npm ls <pkg> --all` (which needs `ui/node_modules`, installed
this revision) gave the exact dependency-chain paths; `ui/package-lock.json`
gives the exact resolved versions at each node.

- **shadcn's own chain is confirmed unreached.** `shadcn@4.18.0`'s registry
  `exports` map lists `"./tailwind.css": "./dist/tailwind.css"` as a direct
  static-file export, no JS. `ui/src/index.css:3`'s `@import
  "shadcn/tailwind.css"` resolves only that asset. No source file imports
  `shadcn`'s JS and no `npm run` script invokes it.

- **Direct advisories, by package, with exact resolved version(s), every
  GHSA id, its affected range, and its fix:**

  | package | resolved node(s) | GHSA ids | fix |
  |---|---|---|---|
  | `@modelcontextprotocol/sdk` | `@modelcontextprotocol/sdk@1.30.0` | `GHSA-6qxp-vccf-f47h` (`>=1.12.0 <1.31.0`, OAuth client could send credentials to an attacker-chosen authorization server) | `fixAvailable: true` |
  | `proxy-addr` | `proxy-addr@2.0.7` | `GHSA-jqcg-44mw-7w3h` (`>=1.1.0 <2.0.8`, critical, IP spoofing via IPv4-mapped IPv6 trust subnet) | `true` |
  | `qs` | `qs@6.15.3` | `GHSA-x5fp-wj9c-mxmx` (`>=6.14.2 <=6.15.3`), `GHSA-4mjr-xmp4-gh2g` (`>=2.2.5 <6.16.0`) | `true` |
  | `hono` | `hono@4.13.2` | `GHSA-gqvv-2mrq-wpjv`, `GHSA-g6gw-c38x-mqfc`, `GHSA-crvj-82cr-hjcx` (all `<4.13.5`), `GHSA-hxh3-vqpv-xpqv` (`<4.13.7`) | `true` |
  | `ip-address` | `ip-address@10.5.0` | `GHSA-rpw4-54j3-4h4q` (`<=10.5.0`), `GHSA-2vr4-cq9g-pvrc` (`>=10.2.0 <=10.5.0`), `GHSA-j6r3-76f7-8jcv`, `GHSA-h3mg-xc3c-68pw` (both `<=10.7.0`) | `true` |
  | `postcss-selector-parser` | `postcss-selector-parser@7.1.5` | `GHSA-rj75-hqrm-r3gf` (`<7.1.6`) | `true` |
  | `undici` | `undici@7.29.0` **and** `jsdom`'s nested `undici@8.10.0` | 11 distinct ids — see below | `true` |
  | `fast-uri` | `fast-uri@3.1.5` | `GHSA-5jgf-p345-68v8`, `GHSA-f65p-4m7j-42xc`, `GHSA-fph4-wmhf-6fwf`, `GHSA-jqff-g426-hqxp`, `GHSA-qw65-cvwx-89v3`, `GHSA-hrr3-gc8f-f4qj` | `true` |
  | `js-yaml` | `js-yaml@4.3.1` | `GHSA-2883-xcg3-v3hh` (`>=4.0.0 <4.3.2`) | `true` |
  | `source-map-js` | `source-map-js@1.2.1` | `GHSA-68fv-2mgg-jv7q` (`>=1.0.0 <1.2.2`) | `true` |
  | `@vitest/mocker` / `vitest` | `@vitest/mocker@4.1.10`, `vitest@4.1.10` | `GHSA-82fw-gwwq-j7x9` (`>=2.1.0 <4.1.11`) | `true` |
  | `brace-expansion` | `node_modules/brace-expansion@2.1.4` **and** `@ts-morph/common`'s nested `brace-expansion@5.0.9` | `GHSA-q2hr-2g5m-vwhr`, `GHSA-qhr7-859c-m2p7`, `GHSA-6j4f-fj2g-mc7p` (each with a `2.x` and a `4.x`/`5.x` range; both resolved nodes fall in range for all three) | `true` |
  | `braces` | `braces@3.0.3` | `GHSA-vfj7-8cjw-p6xm` (`<=3.0.3`) | major `shadcn` bump |

  **Inherited** (vulnerable only because they depend on a package above, no
  advisory of their own): `@redocly/openapi-core` (-> `js-yaml`),
  `@ts-morph/common` (-> `fast-glob`), `fast-glob` (-> `micromatch`),
  `micromatch` (-> `braces`), `ts-morph` (-> `@ts-morph/common`), `shadcn`
  (-> `fast-glob`, `ts-morph`).

- **`undici`'s 11 distinct GHSA ids** (corrected from "nine" —
  `GHSA-3wwx-pv8p-q78v`, `GHSA-pmjh-fq2x-6v4x`, `GHSA-r53p-7pc4-xj5r`,
  `GHSA-rfgv-xxqx-mfg5`, `GHSA-3xpg-4rpp-hhhm`, `GHSA-2jfj-6hjv-fm6j`,
  `GHSA-2gqq-gqf2-x968`, `GHSA-w293-vg96-wgc3`, `GHSA-8436-99hf-9mmv`,
  `GHSA-rx4f-c7p8-82vq` apply to both the `7.29.0` and `8.10.0` resolved
  nodes; `GHSA-vp8m-p9jh-q5pm` (`>=8.10.0 <8.10.2`) applies only to the
  `8.10.0` node): reachable through **two** `shadcn`-rooted paths (`shadcn
  -> undici@7.29.0` directly, and `shadcn -> @dotenvx/dotenvx ->
  undici@7.29.0`, deduped to the same resolved version) **and**,
  separately, through the devDependency `jsdom@30.0.1 ->
  undici@8.10.0` (used only when `vitest`'s jsdom test environment runs).
  The `shadcn` paths are unreached; the `jsdom` path is real but
  test-time-only.

- **`fast-uri`** (via `ajv@8.20.0`): reachable through `shadcn ->
  @modelcontextprotocol/sdk -> ajv@8.20.0` and, independently, through
  `shadcn -> @dotenvx/dotenvx -> conf -> ajv@8.20.0`. **Both corrected
  chains** — the previous revision wrongly attributed `fast-uri` to the
  `@redocly/openapi-core`/`gen:api` path (that package depends on a
  *different*, scoped `@redocly/ajv@8.11.2`, whose own dependency is
  `uri-js-replace`, not `fast-uri`) and wrongly guessed `conf`'s parent as
  "likely `@tauri-apps/cli`" instead of `@dotenvx/dotenvx`, itself
  `shadcn`'s own dependency. Both real paths are `shadcn`-rooted and
  unreached. `ajv`'s remaining parent beyond `conf` and
  `@modelcontextprotocol/sdk` (whatever depends on `cosmiconfig`) was not
  traced further — recorded as an explicit minor gap.

- **`source-map-js`**: `@tailwindcss/node -> source-map-js`, where
  `@tailwindcss/node` is `@tailwindcss/vite`'s own dependency — a
  devDependency Vite's build pipeline actually executes on every `npm run
  build`/`npm run dev`. Real, build-time reachable, **not**
  `shadcn`-rooted.

- **`js-yaml`, `brace-expansion`, and `@redocly/openapi-core` — corrected
  reachability, per the review's independent reproduction.** The previous
  revision rejected any `js-yaml` exploit path through `gen:api`, reasoning
  that `gen-api.mjs`'s `fetchSpec` calls `res.json()`/`JSON.parse` and
  hands `openapiTS()` a plain object, never raw text. **That reasoning is
  correct about the root document but incomplete**: `openapi-typescript`'s
  `validateAndBundle` (`dist/lib/redoc.mjs`) calls `@redocly/openapi-core`'s
  `bundle`/`resolve`, which follows **external `$ref`s** in the document —
  regardless of whether the root was JSON or YAML — and parses whatever it
  fetches or reads for each one with `@redocly/openapi-core`'s own
  `parseYaml` (`lib/js-yaml/index.js`: `js_yaml_1.load` with a schema whose
  `implicit` types explicitly include `js_yaml_1.types.merge`, i.e.
  merge-key support is deliberately enabled). This also explains
  `brace-expansion@2.1.4`'s real chain: `openapi-typescript ->
  @redocly/openapi-core -> minimatch -> brace-expansion`, a glob-matching
  dependency of `@redocly/openapi-core` itself, reached by the same
  `gen:api` devDependency path — **not** `shadcn`-rooted, contrary to an
  earlier, incomplete trace.

  A bounded, harmless reproduction (`repro/supply-006-yaml-ref/`, run three
  consecutive times this revision, deterministic) confirms the mechanism:
  a plain JS OpenAPI object — the same shape `gen-api.mjs` builds — with a
  `$ref` to an external YAML file using `<<: *anchor` merge syntax, run
  through the real, installed `openapiTS()`, produces generated output
  containing a field that exists only because the merge was resolved. This
  proves the merge-key YAML parser **is reached** by this dependency chain
  given the right input; it does **not** reproduce `GHSA-2883-xcg3-v3hh`'s
  specific CPU-exhaustion behavior (an empty merge source `maxTotalMergeKeys`
  fails to bound) — no exhaustion payload was built or run, only a
  two-key merge proving the parser executes.

  **The prerequisite this narrows the finding to:** `gen-api.mjs`'s default
  target is the daemon's own self-generated OpenAPI document
  (`GET /api-docs/openapi.json`), a single self-contained JSON document
  with no external `$ref`s today — this path is not live against the
  default developer workflow. It becomes live only if a developer runs
  `npm run gen:api -- <url-or-path>` against a target whose document (or
  anything it `$ref`s) is not self-contained and points at content the
  developer has not reviewed, or if the daemon's own generated document
  ever gains an external `$ref` (not the case today). `js-yaml` and
  `brace-expansion` stay in the advisory inventory as present, real,
  build-reachable-given-that-prerequisite dependencies — confirmed reached
  by the parser, not confirmed as a demonstrated CPU-exhaustion exploit.

- **`@vitest/mocker`/`vitest`**: reachable on every `npm test`/`npm run
  check`/CI run. Needs a malicious test file to matter — a
  contribution-trust question, not an external attacker surface.

- **Remediation (smallest):** `npm audit fix` resolves every package in
  the table above marked `fixAvailable: true` without a major bump of
  anything; `braces` (and the inherited
  `ts-morph`/`fast-glob`/`micromatch`/`@ts-morph/common`/`shadcn` cluster)
  needs a major `shadcn` bump and can be deferred, since nothing reaches
  it. For the `gen:api` chain specifically (`js-yaml`, `brace-expansion`,
  transitively via `@redocly/openapi-core`/`minimatch`), `npm audit fix`
  also applies without a major bump; additionally, a developer-facing
  warning or an allowlist of trusted `gen:api` targets would close the
  external-`$ref` prerequisite this finding depends on, if that is judged
  worth doing for a tool invoked by hand against a developer-chosen target.
- **Regression criterion:** `npm audit --json` reports no advisory ID for
  any package in the table above — not merely that `fixAvailable` changed,
  which can be true even when the advisory is still present at a different
  resolved version.
- **Confidence:** confirmed for every chain, exact version, and advisory id
  in this revision (`npm ls --all` plus a direct, `semver`-checked
  cross-reference of `ui/package-lock.json` against a freshly captured
  `npm audit --json`, both committed as evidence — not inferred or
  eyeballed). The `js-yaml`/`brace-expansion` parser-reachability claim is
  confirmed by a committed, deterministic reproduction; the specific
  CPU-exhaustion behavior of `GHSA-2883-xcg3-v3hh` is not claimed to have
  been reproduced, only the parser's reachability given an external `$ref`.

### Hardening suggestions, new this revision

#### SUPPLY-008 — The benchmark's default cache/venv home is the production AI-permissions home, not an isolated one

- **Affected:** `bench/ai-permissions/run.sh:17`
  (`AI_HOME="${ARIADNE_AI_PERMISSIONS_HOME:-$HOME/.ariadne/ai-permissions}"`)
  against `crates/ariadne-daemon/src/config.rs:92-97` (`Config::load`,
  `root = endpoint::home(home_override)`, defaulting to `~/.ariadne`) and
  `crates/ariadne-daemon/src/ai_permissions/mod.rs:96`
  (`home: cfg.root.join("ai-permissions")`).
- **The collision is real and by default, not hypothetical.** Unless a
  user sets `ARIADNE_AI_PERMISSIONS_HOME`, `ARIADNE_HOME`, or passes
  `--home` differently between the daemon and the benchmark, both resolve
  to the identical path, `~/.ariadne/ai-permissions`. Within it,
  `run.sh`'s venvs (`laya-venv`, `kev-venv`) do not collide by name with
  `install.rs`'s own venv (`venv`), but `run.sh`'s `HF_HOME`
  (`$AI_HOME/hf`, line 88) is the **same path** `install.rs::weights()`
  uses (`ai_permissions.home.join("hf")`) — the Hugging Face model cache is
  shared between the benchmark and production by default.
- **Contrast with the correct pattern elsewhere in the same `bench/` tree.**
  `bench/ai-opportunities/progress/bench_progress/kev_runner.py` already
  does this correctly: it reuses the production **venv** deliberately
  (read-only, revision-checked) but writes model weights into a
  **separate** `HF_HOME` (`~/.ariadne/ai-opportunities/hf`), explicitly to
  avoid touching the production cache. `run.sh` has no such separation by
  default.
- **Impact:** low-to-moderate, operational rather than a security
  boundary break. HF's cache is revision-addressed, so this mostly causes
  additive cache growth, not corruption of the production daemon's pinned
  snapshots. But `install.rs::remove_other_flavours` (`install.rs:325-339`)
  deletes a whole `models--<org>--<repo>` folder — every revision under
  it — for any flavour not currently kept, keyed by repository id, not by
  revision. If a benchmark run (using Kev unpinned, SUPPLY-001) populates
  the shared `hf` cache with a revision of a repository the production
  side does not currently keep, a subsequent successful production install
  or flavour switch will silently delete it. Not a vulnerability, but a
  reproducibility surprise for anyone running the benchmark expecting it to
  be isolated.
- **Confidence:** confirmed — both default paths were read directly from
  source, not inferred from behavior.
- **Remediation (smallest):** default `bench/ai-permissions/run.sh`'s
  `AI_HOME` to something benchmark-specific (e.g.
  `~/.ariadne/ai-permissions-bench`), matching the pattern
  `bench/ai-opportunities` already uses, rather than relying on every user
  remembering to set `ARIADNE_AI_PERMISSIONS_HOME`.
- **Regression criterion:** `bench/ai-permissions/run.sh`'s default
  `AI_HOME` (and therefore `HF_HOME`) resolves to a different path than
  `crates/ariadne-daemon/src/ai_permissions/mod.rs`'s `home` default, with
  no environment variable set.
- **Compatibility decision for the user:** changing the default moves
  where benchmark runs cache weights/venvs on disk for anyone currently
  relying on the shared location (e.g., to avoid downloading the same
  weights twice). Confirm this tradeoff before a remediation task changes
  it.

#### SUPPLY-009 — The installer does not detect or repair a virtual environment left without `pip`

- **Affected:** `crates/ariadne-daemon/src/ai_permissions/install.rs:220-248`
  (`venv`), specifically `venv_interpreter_is_supported` (lines 250-254),
  which only probes the interpreter's own `--version` output.
- **Reproduced independently this revision:** `python3.13 -m venv
  --without-pip <dir>` produces a venv whose interpreter reports a fully
  supported version (`3.13.x`) but whose `-m pip` fails with `No module
  named pip`. `venv_interpreter_is_supported` would accept this
  environment as-is (the version probe is all it checks), so `venv()`
  would return it unchanged rather than rebuilding it, and the subsequent
  `package()` step's `python -m pip install ...` calls (`install.rs:261-265`)
  would then fail with the same `No module named pip` error — surfaced to
  the user as an opaque pip failure, not recognized as "this venv needs
  rebuilding."
- **How this state could arise in practice:** an interrupted `python -m
  venv` (the daemon killed, or the machine losing power, between the
  interpreter being written and `ensurepip`'s bootstrap completing) leaves
  exactly this shape — a working interpreter, no `pip` — and is exactly
  the kind of partial state an install path that downloads gigabytes in the
  background (per `install.rs`'s own module doc) should expect to hit
  occasionally.
- **Impact:** low — a failed install surfaced as a confusing error on
  retry, self-correctable today only by a user manually deleting the venv
  directory (which `venv()` *would* then correctly rebuild, since the
  `interpreter.is_file()` check in `venv()` itself would fail for a
  fully-removed directory). Not a security vulnerability — no untrusted
  input reaches this gap, and no incorrect installed content results from
  it, only a repair the installer does not perform automatically.
- **Confidence:** confirmed; this is a direct reproduction of the exact
  defect, not an inferred one. The previous revision's "reviewed, no
  finding" verdict on partial-install recovery is withdrawn — that review
  only considered the already-covered pip-call idempotency and the
  Linux-only `TORCH_DEVICE` marker, not this specific recovery gap.
- **The `TORCH_DEVICE` marker's own behavior, reviewed again at the
  reviewer's request:** `package()` writes the marker only after every
  `pip` call in a batch succeeds (`install.rs:267-269`), and never deletes
  a stale marker before attempting a device switch. In the scenarios this
  audit could construct (an `uninstall` succeeding but the following
  `install` failing; `torch` succeeding but Kev's own install failing
  afterward), a stale marker causes either a redundant extra
  uninstall/reinstall of `torch` on the next retry or no incorrect behavior
  at all — this audit did not find a case where the stale marker causes a
  wrong final installed state, only wasted work. Recorded for completeness,
  not escalated to its own finding.
- **Remediation (smallest):** have `venv_interpreter_is_supported` (or a
  sibling check in `venv()`) also probe `python -m pip --version`, and
  rebuild the venv (the same `std::fs::remove_dir_all` + recreate path
  already used for an unsupported interpreter version) if that probe
  fails.
- **Regression criterion:** a test constructing a `--without-pip`-style
  venv and asserting `venv()` rebuilds it rather than reusing it.

### Rejected hypotheses

- **"The Rust lockfiles hide a substituted/typosquatted crate."** Narrowed:
  every dependency in both lockfiles resolves from
  `registry+https://github.com/rust-lang/crates.io-index` or the
  workspace's own `path =` members, with crates.io's own recorded checksum.
  This is not the same claim as "no dependency name was ever chosen as a
  typosquat of a more popular crate" — not manually reviewed across
  ~1,000 crate names, and not claimed to have been.
- **"`pip install kev[serve] @ git+...` is vulnerable to dependency
  confusion with the unrelated PyPI `kev` package."** Rejected for the
  production path (`install.rs:295` always supplies the explicit
  `git+https://...@<sha>` URL; `specs/022` documents the distinction). The
  benchmark's unpinned Kev install also always names the VCS URL — its gap
  is the missing commit pin (SUPPLY-001), not name confusion.
- **"The model server (`kev.serve`) executes attacker-controlled code via
  Hugging Face's `trust_remote_code`."** Rejected: zero occurrences in a
  full local clone of the pinned commit.
- **"`shadcn`'s vulnerable transitive dependencies ship inside the Tauri
  desktop bundle, or are reachable via its CSS import."** Rejected: the
  CSS subpath export carries no JS, no source file imports `shadcn`'s JS.
- **"`gen-api.mjs`'s own `res.json()`/`JSON.parse()` of the root OpenAPI
  document feeds raw YAML text to `js-yaml`."** Rejected, specifically for
  the *root document*: both of `gen-api.mjs`'s input paths hand `openapiTS()`
  a plain object, never raw text. **Narrower than the previous revision's
  claim**, which rejected any path at all — see SUPPLY-006 for the real,
  corrected path: `@redocly/openapi-core`'s own external-`$ref` resolution
  parses YAML (with merge-key support) independent of how the root
  document was parsed, which this revision's reproduction confirms is
  reached, given a document that `$ref`s an external YAML file.

## Recommendation

**SUPPLY-007** (the `torch.load`/CVE-2026-24747 finding) remains the
headline result and the priority: a confirmed, high-severity, reached
vulnerability in a dependency range Kev's own upstream cannot currently
escape. **SUPPLY-001** is next — benchmark-only, but now backed by a fully
deterministic, assertion-based reproduction for every one of its claims.
**SUPPLY-008** and **SUPPLY-009** are small, concrete hardening items
surfaced by this review's specific tests. **SUPPLY-002** stays a real but
lower-priority process question. **SUPPLY-004**, **SUPPLY-005**, and
**SUPPLY-006** are routine hardening, none urgent.

## Confidence

High confidence in every finding's facts this revision: SUPPLY-001's
reproduction is now fully deterministic (three consecutive identical runs,
including an added assertion on the pinned commit's recorded metadata)
rather than disclosed-but-flaky, SUPPLY-007's evidence is a committed,
structured, full-hash resolution rather than an abbreviated one (and its
package count corrected from 75 to the actual 76), and SUPPLY-006's npm
advisory inventory is now a complete, `semver`-checked table backed by
committed raw evidence rather than vague counts. The places confidence is
intentionally split, each disclosed rather than smoothed over: SUPPLY-002
(metrics confirmed, intent not, and explicitly not claimed); SUPPLY-007
(the dependency/call-site chain is confirmed; a working exploit was not
built or run); and SUPPLY-006's `js-yaml`/`brace-expansion` reachability
(the parser is confirmed reached via an external `$ref`, by a committed
reproduction; the specific CPU-exhaustion behavior of `GHSA-2883-xcg3-v3hh`
is not claimed to have been reproduced, and the default developer workflow
does not trigger the external-`$ref` prerequisite at all).

## Unresolved design questions for the orchestrator

1. **SUPPLY-007 (highest priority):** Ariadne has no direct way to raise
   Kev's torch ceiling past the CVE-2026-24747 fix version (2.10.0) without
   upstream Kev widening its own declared range, or Ariadne forcing an
   untested torch version. Which tradeoff does the user want?
2. Should SUPPLY-001's benchmark pinning be a follow-up task now, given
   `bench/AGENTS.md` already states this path never runs in CI?
3. Does the user want a standing review process for future `kev` pin bumps
   (SUPPLY-002), now that SUPPLY-007 shows a hostile pinned revision's
   impact is "potential arbitrary code execution," not merely "a biased
   decision"?
4. SUPPLY-004's remediation keeps `"*.pt"` in both `allow_patterns` lists
   since `head.pt` is load-bearing for every flavour (SUPPLY-007). Confirm
   before a remediation task changes it.
5. SUPPLY-008's remediation changes where benchmark runs cache weights and
   venvs by default. Confirm this is acceptable before a remediation task
   changes it.
6. SUPPLY-006's `gen:api` finding is only live against a developer-chosen,
   non-default target. Does the user want a hardening task (a warning, or
   an allowlist of trusted targets) for `npm run gen:api -- <url-or-path>`,
   or is "a developer must already trust what they point this at" an
   acceptable existing control?

## Cross-boundary evidence for RELEASE

None found. This audit's work stayed inside package and model-artifact
resolution, not GitHub Actions, release artifacts, or application
installation — RELEASE's area per the cross-task contract.
