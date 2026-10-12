# SUPPLY: dependencies and model artifacts

## Question

Can vulnerable or substituted dependencies and model artifacts compromise
Ariadne's builds or runtime execution?

## Examined commit

`a17283486d5fe89591edfff08832712936d73030` (main, 2026-10-12 21:32:41 +0200),
checked out at `/Users/malvarez/.ariadne/worktrees/q4r9v3t3/w3anyvzc-eng` on
branch `audit-third-party-packages-and-model-anyvzc`. Unchanged from the
version the reviewer examined.

Scan date for every advisory/metadata check below: 2026-10-12. RustSec
advisory-db at commit `57dcc0f2192e025138907a495c1ac9a7e1e3e123`
(2026-10-12 20:06:48 +0200), 1,307 advisories, via `cargo-audit` 0.22.2.

## Corrections since the reviewed version (commit c2d88566)

The reviewer found two factual errors and five coverage gaps. Rather than
patch around them, this revision re-did the affected work. What changed, so
the mapping from the old finding IDs is traceable:

- **Old SUPPLY-003 ("shadcn misclassified as a production dependency") is
  withdrawn.** It was wrong: `ui/src/index.css:3` imports
  `shadcn/tailwind.css`, and `ui/AGENTS.md:447-449` states exactly why
  `shadcn` stays in `dependencies`. A JavaScript-only `grep` for an import of
  the package missed the CSS `@import`. See **Withdrawn findings** below for
  the full correction, and **SUPPLY-006** (new) for what the npm advisory
  data actually shows once shadcn's own CSS-only reachability is accounted
  for correctly.
- **SUPPLY-002's counter-evidence was built on a false negative.** GitHub's
  `search/code` endpoint does not reliably index an arbitrary historical
  commit (it is scoped to a repository's default branch), so a 0-hit search
  for `torch.load` against the pinned Kev commit proved nothing. A direct
  clone of that exact commit (see Coverage) finds `torch.load` at
  `kev/checkpoint.py:76`. This is now **SUPPLY-003**, a confirmed, high
  severity finding: the exact torch version range Kev pins is within the
  affected range of a real CVE in `torch.load`'s `weights_only` unpickler.
  SUPPLY-002 itself is kept, but narrowed to what it can actually support,
  and demoted in priority relative to SUPPLY-003 per the review.
- **SUPPLY-004 is downgraded**, now that the exact file manifests of all
  four pinned base-model revisions are known (fetched via the Hugging Face
  metadata API, not downloaded): none contain anything an `allow_patterns`
  restriction would have excluded. The gap is real but currently inert.
- **SUPPLY-005 is corrected** to separate a proc-macro crate's build-time
  execution surface from runtime reachability, and to keep the advisory
  database's exact revision and `glib`'s call-site uncertainty on record
  explicitly, per the review.
- **SUPPLY-001 is corrected**: a wheel install does not run a build backend
  (only a VCS/sdist install does); the real execution sinks, the actual
  prerequisites, and an isolated reproduction are added (see
  `repro/supply-001-pip-integrity.sh`).
- **Production Python/model coverage is completed**: a real `pip install
  --dry-run` resolution of the pinned production install (not an estimate),
  an OSV.dev scan of every one of its ~75 resolved packages, and a reading of
  `bench/ai-opportunities/progress/bench_progress/kev_runner.py` (inside the
  assigned `bench/` path, missed in the first pass because it holds no
  literal `pip install`/`npm install` string).
- The typosquatting **rejected hypothesis is narrowed** to what registry/
  checksum integrity actually proves.

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
  metadata APIs, OSV.dev, and pip's own documentation and observed
  behavior — not from a web-page summary alone. A first pass using a
  summarizer tool to describe `jaredpalmer/kev` and `NandhaKishorM/laya`
  produced confident, favorable prose ("an established ML research
  project", "institutional-grade") built on the same inflated counters this
  audit is skeptical of; it is recorded in SUPPLY-002 as a cautionary
  example, not relied on.

## Coverage

### Reviewed

- `Cargo.toml`, `Cargo.lock` (workspace, 557 crate dependencies).
- `ui/src-tauri/Cargo.toml`, `ui/src-tauri/Cargo.lock` (465 crate
  dependencies, independent tree, excluded from the workspace).
- `ui/package.json`, `ui/package-lock.json` (718 npm packages: 537 prod, 181
  dev, per `npm audit`'s own classification), `ui/src/index.css`,
  `ui/AGENTS.md`, `ui/scripts/gen-api.mjs`.
- `crates/ariadne-daemon/src/ai_permissions/{install,flavours,server,python,mod,decide,hardware}.rs`.
- `bench/ai-permissions/run.sh`, `bench/ai-permissions/run.py`,
  `bench/ai-permissions/evaluators/{kev,laya}/*.py`,
  `bench/ai-opportunities/progress/bench_progress/kev_runner.py`,
  `bench/AGENTS.md`.
- `specs/022-ai-permission-mode.md` (full), `specs/024-advisory-failure-diagnosis.md`
  (scope and behavior sections — confirms it governs ACP error
  classification through the same Kev endpoint, not a dependency-advisory
  feature despite the filename).
- **A full local clone of `jaredpalmer/kev` at the pinned commit**
  `f1535963cea021439370c23127bc970b6788e730` (`git clone` + `git checkout`,
  not GitHub's code-search index — see Corrections above), `grep`ped
  repository-wide for `torch.load`, `torch.save`, `weights_only`,
  `trust_remote_code`, `pickle`, and `safetensors`. `kev/checkpoint.py` read
  in full.
- The pinned commit's own `pyproject.toml` (fetched at that exact revision).
- GitHub repository metadata (`gh api repos/...`) for `jaredpalmer/kev` and
  `NandhaKishorM/laya`: creation date, push date, star/fork/issue counts,
  license, owner type.
- Hugging Face model metadata (`huggingface.co/api/models/.../revision/...`)
  for all four pinned flavours' adapter and base repos: file manifests
  (`siblings`), `library_name`, author, download/like counts — fetched
  without downloading any model weights.
- PyPI metadata for every direct runtime/serve dependency in the pinned
  `pyproject.toml` (`accelerate`, `datasets`, `numpy`, `peft`, `pydantic`,
  `torch`, `transformers`, `fastapi`, `typesafe-sdk`, `uvicorn`, `mlx-lm`):
  publisher, release history, and (for `torch`) full OSV.dev advisory
  records.
- `crates/AGENTS.md`, `bench/AGENTS.md` for the checks and exposure rules
  each area declares for itself.

### Tested

- `cargo audit` (v0.22.2, RustSec advisory-db, live-fetched) against both
  Rust lockfiles.
- `npm audit --omit=dev` and `npm audit` (full) against
  `ui/package-lock.json`, with every advisory's GHSA id, affected range, and
  fix availability extracted, and dependency chains reconstructed from
  `ui/package-lock.json`'s `packages` map for every flagged package (not
  only the ones that happened to trace to `shadcn`).
- `npm view`/registry metadata for the `shadcn` package itself: its
  `exports` map, to confirm `./tailwind.css` resolves to a static asset
  (`dist/tailwind.css`) with no JS entry point, independent of its CLI,
  `mcp`, `utils`, `preset`, `registry` exports.
- `cargo tree -i <crate>` (workspace and, with `--target all`, the Tauri
  tree) for every crate named in a `cargo audit` warning, to find what pulls
  it in and whether that path is Linux-only, build-only, or
  always-reachable.
- **A real `pip install --dry-run --report` resolution** (not an estimate)
  of the exact production install command
  (`kev[serve] @ git+https://github.com/jaredpalmer/kev@f1535963...`) on
  Python 3.13 under macOS/arm64 — the platform this sandbox actually runs on
  and one of Kev's two supported interpreter versions
  (`crates/ariadne-daemon/src/ai_permissions/python.rs:14`). Produced a
  concrete list of 74 resolved packages and versions.
- An OSV.dev `querybatch` of all 74 resolved packages at their resolved
  versions.
- A manual range-resolution cross-check (via `packaging.specifiers`,
  independent of the dry-run) against every PyPI release of each direct
  dependency, to confirm the dry-run's choices were not an artifact of this
  one machine's package index cache.
- **An isolated, synthetic reproduction** for SUPPLY-001, entirely against
  local stand-ins (a bare git repo under `/tmp`, locally built wheels) —
  script and transcript committed at
  `repro/supply-001-pip-integrity.sh` / `repro/supply-001-transcript.log`.
- A direct `git clone`/`checkout` + `grep` of the pinned Kev commit (listed
  under Reviewed, repeated here because it is also a test of a specific
  claim: "does this code call an unsafe deserializer").

### Excluded, with reason

- Actually installing the Kev/Laya virtual environments and downloading
  model weights (multi-gigabyte, and most flavours need hardware this
  sandbox does not have). The `pip install --dry-run` test above is the
  closest substitute that is both real (not estimated) and bounded; it
  resolves every package pip would actually choose without downloading any
  of their multi-megabyte-to-gigabyte payloads.
- `crates/ariadne-daemon/tests/it/ai_permissions*.rs`: read for context via
  the spec's test list, not re-run. They exercise a stub installer, not the
  real `pip`/Hugging Face path.
- GitHub Actions / release pipeline, release artifact signing, and the
  installer scripts under `scripts/` — RELEASE's area per the cross-task
  contract.
- `docs/` and `README.md`. Not named in the starting sources and not a
  resolution/execution path for a dependency or model.
- `kev/full_ft.py`'s and `kev/mlx_model.py`'s own deserialization paths
  beyond the `torch.load`/`torch.save` grep already covering them: they are
  training/MLX-specific code (`kev.train --init_from`, the MLX backend's own
  adapter merge) that Ariadne's daemon never invokes (`server.rs`'s
  `device_env`/`entry_point` only ever run `kev.serve`). Listed as excluded
  rather than silently skipped.
- Re-deriving the exact root cause of a non-deterministic result observed
  while building the SUPPLY-001 reproduction (see the methodology note
  inside that finding). Recorded as an explicit, acknowledged gap rather
  than either hidden or asserted as a finding.

## Findings

### Confirmed

#### SUPPLY-003 — Kev's pinned torch dependency range is entirely within the affected range of a high-severity `torch.load` deserialization CVE

- **Affected:** `kev/checkpoint.py:75-76` (`read_meta`, calling
  `torch.load(f"{run}/head.pt", map_location="cpu")` with no explicit
  `weights_only` argument), at the pinned commit
  `f1535963cea021439370c23127bc970b6788e730`;
  `crates/ariadne-daemon/src/ai_permissions/install.rs:25` (`TORCH: &str =
  "torch>=2.6,<2.9"`), which the pinned commit's own `pyproject.toml`
  repeats as its own constraint.
- **The sink is real and reached in production.** `kev/checkpoint.py:167-170`
  (`Checkpoint.__init__`) calls `read_meta` unconditionally whenever a
  checkpoint is opened; the module's own docstring (lines 1-11) states that
  `kev.serve` — the process `crates/ariadne-daemon/src/ai_permissions/server.rs:218-225`
  launches on every model start (`entry_point`, `-m kev.serve`) — "goes
  through it". Every adapter and base-model revision the daemon installs
  (`install.rs:300-320`, `weights()`) ships a `head.pt` file (confirmed
  present in all four flavours' Hugging Face file manifests — see
  Coverage), and `read_meta` runs on it every time the server starts, every
  flavour/device switch, and every refresh.
- **The vulnerable version range is unavoidable under Kev's current pin.**
  `torch.load`'s `weights_only` parameter has defaulted to `True` since
  PyTorch 2.6 (PyTorch 2.6.0 release notes: "We are closing the loop on the
  deprecation that started in 2.4 and flipped `torch.load` to use
  `weights_only=True` by default" — primary source,
  `github.com/pytorch/pytorch/releases/tag/v2.6.0`), which is why this call
  site is not an obvious "arbitrary pickle" bug. However, **CVE-2026-24747**
  (`GHSA-63cw-57p8-fm3p`, `PYSEC-2026-2286`, CVSS 3.1 `AV:N/AC:L/PR:N/UI:R/S:U/C:H/I:H/A:H`
  = 8.8 High) is a vulnerability *in the `weights_only` unpickler itself*:
  "a vulnerability in PyTorch's `weights_only` unpickler allows an attacker
  to craft a malicious checkpoint file (`.pth`) that, when loaded with
  `torch.load(..., weights_only=True)`, can corrupt memory and potentially
  lead to arbitrary code execution" (OSV.dev `PYSEC-2026-2286`, aliasing
  `CVE-2026-24747`). It is fixed in **torch 2.10.0**. Kev's own pin
  (`torch>=2.6,<2.9`, both in `install.rs:25` and the pinned commit's
  `pyproject.toml`) excludes 2.10.0 entirely — every version Kev can
  install is affected, with no fixed version reachable within the declared
  range.
- **Confirmed by a real resolution, not an estimate.** `pip install
  --dry-run` of the exact production install command resolves `torch==2.8.0`
  (see Coverage). An OSV.dev query for `torch==2.8.0` returns 8 advisories;
  `PYSEC-2026-2286`/`CVE-2026-24747` is one of them, and is the only one of
  the 8 whose affected function (the `weights_only` unpickler) Kev's own
  code demonstrably calls. (The other 7 — memory corruption in
  `torch.lstm_cell`, `torch.jit.script`, `unpack_sequence`; DoS/overflow in
  `torch.linalg.lu`, `torch.rot90`/`torch.randn_like`,
  `torch.nan_to_num`; and a `pt2` loading-handler issue scoped to torch
  2.10.0 itself — require an attacker to control arguments to those
  specific tensor operations directly. Kev's serving API takes a text
  prompt through a fixed scoring pipeline, not arbitrary tensor ops; no
  evidence was found that any of these 7 are reachable, and none are
  pursued further here.) Every one of the other 73 resolved packages
  (`transformers==5.19.0`, `peft==0.21.2`, `accelerate==1.15.0`,
  `huggingface_hub==2.2.0`, `safetensors==0.8.0`, `mlx==0.32.3`,
  `mlx-lm==0.31.3`, `fastapi==0.143.0`, `typesafe-sdk==0.7.4`, and the rest)
  returned **zero** OSV.dev advisories.
- **Prerequisites:** the `head.pt` file loaded at some point must be
  attacker-crafted. This finding does not claim the *currently* pinned
  `head.pt` files (at the exact adapter revisions `install.rs`/`flavours.rs`
  pin) are malicious — no evidence of that was found or sought here; the
  finding is that the mechanism exists, is reached on every server start,
  and that the dependency range protecting it has no available fix. The
  realistic path to an attacker-crafted `head.pt` is exactly the trust
  chain SUPPLY-001 and SUPPLY-002 describe: an unpinned benchmark install,
  or a future production pin bump taken on faith from the upstream account.
- **Severity rationale:** CVSS 8.8 (High) is the upstream rating for the
  underlying CVE; this report rates the finding itself High rather than
  Critical because exploitation additionally requires attacker control over
  a specific checkpoint file, which is gated by the pinning/trust controls
  SUPPLY-001/002 already describe — it is not exposed to an arbitrary
  network attacker today.
- **Impact:** memory corruption and potential arbitrary code execution in
  the daemon-managed `kev.serve` child process, on every machine running
  the `ai` permission mode, triggered by loading a single crafted
  checkpoint file — a materially larger impact than this report's earlier
  framing ("could bias a permission decision"). This upgrades the stakes of
  SUPPLY-002's trust question: a hostile pinned revision would not just bias
  an allow/deny decision, it could execute code.
- **Confidence:** confirmed. The CVE, its affected range, its fix version,
  and the resolved `torch` version are all primary-sourced (OSV.dev, GitHub
  Security Advisory, PyTorch's own release notes). The call-site reachability
  is confirmed by direct inspection of the pinned source, not an estimate.
  What is not confirmed, and is not claimed, is that any *currently* pinned
  checkpoint file actually contains an exploit.
- **What would invalidate or change this finding:** a Kev release raising
  its torch ceiling to allow `>=2.10.0` (upstream problem, outside Ariadne's
  control to force); evidence that `transformers`/`peft`'s own loading path
  (rather than `kev/checkpoint.py`'s direct `torch.load` call) is what
  actually executes at runtime, which this audit's direct reading of
  `checkpoint.py` rules out; or evidence that PyTorch's weights_only
  unpickler fix was backported to a 2.6–2.8 point release after this scan
  date (not found in the versions PyPI serves as of 2026-10-12).
- **Remediation (smallest):** this is an upstream (`jaredpalmer/kev`)
  dependency-range problem, not something `install.rs` can fix by itself —
  Ariadne cannot force a wider torch range than Kev's own `pyproject.toml`
  declares without forking or patching Kev. Options, ranked by how little
  they touch: (1) ask upstream Kev to widen its torch ceiling past 2.10.0
  once it has validated compatibility, and bump `install.rs:25`'s
  `TORCH` constant to match; (2) if (1) is not available in a useful time
  frame, have Ariadne's own installer `pip install --upgrade
  torch>=2.10` *after* installing `kev[serve]`, accepting the risk that this
  runs Kev on a torch release it was not tested against; (3) treat this as
  an accepted risk bounded by the existing commit-pin trust chain, and
  revisit once Kev bumps its own ceiling. This is exactly the kind of
  compatibility tradeoff the task brief reserves for the user — see
  **Unresolved design questions**.
- **Regression criterion:** the resolved `torch` version in a fresh `pip
  install --dry-run` of the pinned Kev command no longer appears in an
  OSV.dev query, or appears with `PYSEC-2026-2286`/`CVE-2026-24747` absent
  from the result.

#### SUPPLY-001 — The benchmark's Laya and Kev installs are unpinned and unverified, unlike production

- **Affected:** `bench/ai-permissions/run.sh:21-22,98-147` (`LAYA_RELEASES`,
  `KEV_REPOSITORY`, `latest_laya_wheel`, `ensure_venv`).
- **Corrected execution model (per review item 5).** A **wheel** install
  (Laya, once `latest_laya_wheel` resolves a URL) does **not** run the
  wheel's `setup.py` or any build backend — a wheel is a prebuilt archive
  that `pip` unpacks directly into `site-packages`; there is no build step
  to execute arbitrary code at *install* time. The real execution sinks for
  a malicious Laya wheel are later: any top-level code in its `__init__.py`
  or an installed console-script entry point runs the moment the benchmark
  **imports** or **launches** it — which `bench/ai-permissions/run.py`'s
  evaluator registry and `run.sh`'s per-evaluator `python "$HERE/run.py"
  run ...` invocation do on every benchmark run. A **source/VCS** install
  (Kev's `git+https://github.com/jaredpalmer/kev`, unpinned) is different:
  pip clones the repository and runs its declared build backend
  (`setuptools.build_meta`, per the pinned `pyproject.toml`) to produce a
  wheel *at install time*, under the invoking developer's own user — this
  is where Kev's unpinned install really does execute arbitrary code
  immediately, not just later at import.
- **Prerequisites, corrected (per review item 5).** Exploiting this needs
  control over what `GET /repos/NandhaKishorM/laya/releases/latest`
  currently returns, or over `jaredpalmer/kev`'s default-branch tip at the
  moment `run.sh` runs — i.e., control of the upstream account, its release
  process, or a demonstrated compromise of GitHub's release/ref serving for
  that repository. An unrelated third party cannot gain anything by merely
  "racing two runs"; the earlier draft's wording to that effect is
  withdrawn. HTTPS transport is not the gap here — it protects the
  requests in transit; the gap is that *whatever the legitimate account
  currently serves* is trusted sight-unseen, with no pin and no hash.
- **Isolated reproduction** (script and full transcript committed at
  `repro/supply-001-pip-integrity.sh` / `repro/supply-001-transcript.log`,
  independently re-runnable, touches nothing outside `/tmp`):
  - **Unpinned mutability, proven cleanly and repeatably.** A synthetic git
    remote is installed unpinned twice, with a commit pushed to its default
    branch in between: the first install returns the original content
    (`benign-v1`); the second, otherwise identical, install returns the new
    content (`SWAPPED-v2-malicious`) with no error or warning of any kind.
    This is the core claim and it reproduced identically on every run.
  - **Hash-checking catches a substituted artifact, proven cleanly and
    repeatably.** A synthetic wheel is built, its sha256 recorded in a
    `--hash=sha256:...` requirement, and `pip install --require-hashes`
    against it succeeds. The wheel file at that same path is then replaced
    with different content (simulating a re-published release asset at an
    unchanged URL) and the identical command is re-run: pip refuses with
    `THESE PACKAGES DO NOT MATCH THE HASHES FROM THE REQUIREMENTS FILE. ...
    someone may have tampered with them`, naming the expected and actual
    sha256. This is the primary evidence that hash-pinning Laya's wheel URL
    (`LAYA_WHEEL` already exists as an override in `run.sh:52-53`) would
    have caught exactly this substitution.
  - **VCS requirements cannot be hash-checked, confirmed directly (not just
    from documentation).** `pip install --require-hashes` against a `git+`
    URL — pinned to a commit or not — is refused outright: `ERROR: Can't
    verify hashes for these requirements because we don't have a way to
    hash version control repositories`. This matches pip's documented
    behavior (`pip.pypa.io/en/stable/topics/secure-installs/`: hashing
    "notably prevents the combination of hashed requirements with local
    directories or VCS URLs") and is why Kev's remediation must be a commit
    pin, never a hash.
  - **Methodology note, disclosed rather than hidden.** A third claim —
    that pinning a VCS requirement to a commit SHA keeps installing that
    commit's content even after the remote's default branch moves past
    it — is exactly what `install.rs` already relies on for Kev, and is
    pip/git's documented behavior. Attempting to demonstrate it against a
    local `git+file://` stand-in gave inconsistent results across repeated
    runs in this sandbox: some runs correctly returned the pinned commit's
    original content, others returned the newer content despite an
    identical command and an isolated, never-before-used `PIP_CACHE_DIR`;
    adding `-v`/`GIT_TRACE=1` to one otherwise-identical failing case made
    it pass. The trace output showed git spawning a detached `git
    maintenance run --auto --quiet --detach` background process on every
    fetch; the most plausible explanation found is that this background
    process, run many times in quick succession against short-lived
    repositories sharing one machine's temp directory, raced with a later
    test's filesystem state — an artifact of rapid, repeated local-git
    testing against ephemeral `file://` remotes on one machine, not a
    documented property of git, pip, or of `install.rs`'s real
    `https://github.com/...` install. This report does not assert either
    way about this specific sub-claim from its own reproduction; it relies
    instead on pip's documented VCS pinning semantics and on Ariadne's own
    existing test (`flavours.rs::every_pin_is_a_full_sha_and_4b_matches_the_install`)
    for the claim that commit-pinning works. The flaky run's raw transcript
    is kept, unedited, in `repro/supply-001-transcript.log` rather than
    re-run until it looked clean.
- **Severity rationale:** high if exercised (unauthenticated code execution
  with no integrity check), capped by scope: `bench/AGENTS.md` states this
  path is explicitly dev/benchmark-only and never runs in CI.
  Production/benchmark separation confirmed: `grep -ril laya crates/ specs/`
  returns nothing; `laya` never appears outside `bench/`.
- **Confidence:** confirmed for the mechanism (unpinned resolution, hash
  availability and its VCS limitation); the pinning-survives-a-change
  sub-claim rests on documentation and the project's own test, not this
  audit's own flaky local reproduction (see above).
- **Remediation (smallest):** pin `KEV_REPOSITORY` to a commit SHA the same
  way `install.rs::KEV_COMMIT` does; replace `latest_laya_wheel`'s "latest
  release" lookup with a committed, specific wheel URL plus a
  `--hash=sha256:...` passed to `pip install` (`LAYA_WHEEL` already exists
  as the override path — make it, or an equivalent explicit pin, the only
  path, not a fallback from "latest").
- **Regression criterion:** a check that fails if `run.sh` contains the
  literal string `releases/latest`, or a `git+https://...kev` URL with no
  `@`-revision.
- **Compatibility decision for the user:** pinning means a human bumps
  Laya's wheel by hand to pick up a new release, matching what Kev already
  requires. Confirm this tradeoff before a remediation task changes it.

### Unverified candidates, reclassified as process hardening

#### SUPPLY-002 — The production AI-permission model's sole upstream has an anomalous trust profile (narrowed per review item 6)

- **What is confirmed, and only this:** `jaredpalmer/kev` and
  `NandhaKishorM/laya` (`gh api repos/...`, primary source, not inferred)
  were both created 2026-09-17/18 — three to four weeks before this audit —
  and as of 2026-10-12 show 9,001 stars / 599 forks and 32,542 stars / 2,906
  forks respectively, under personal GitHub accounts. These are the raw,
  independently re-verifiable facts; nothing more is asserted about them as
  fact.
- **What is not confirmed, and is explicitly withdrawn as a claim:** that
  these counts are inflated or manipulated. The prior version of this report
  used language like "anomalous" and leaned on the counts as if they were
  evidence of wrongdoing. They are not — a real repository can grow this
  fast, and this audit has no access to the kind of platform-side abuse
  signal that would distinguish the two. The counts are recorded as a
  reason to apply extra scrutiny to future changes from this upstream, not
  as proof of anything about the current pinned commit.
- **Why a hash pin already bounds the realistic attack surface.** Every
  production pin (`flavours.rs:26-49`, `install.rs:20`) is a full
  40-character commit hash, checked by `flavours.rs`'s own test
  (`every_pin_is_a_full_sha_and_4b_matches_the_install`). Git's content
  addressing means an account takeover *after* a commit was pinned cannot
  retroactively change what that pinned commit resolves to — fetching the
  same SHA from a compromised account still returns the same content, or
  git's own hash check fails loudly. The place this trust question actually
  bites is **the moment a human takes a new pin**: reviewing (or not
  reviewing) what a commit bump actually changes, before it is pinned. This
  is a process question, not a vulnerability in the current pin.
- **Why this is now explicitly lower priority than SUPPLY-003, per review
  item 6:** the realistic, demonstrated risk in this report is SUPPLY-003
  (a named CVE with a confirmed, reached call site and no available fix in
  Kev's declared range) — a concrete problem that exists today regardless
  of upstream intent. SUPPLY-002 is a "what if a future pin review is sloppy"
  process concern. Both matter; SUPPLY-003 is the one to act on first.
- **Confirmed counter-evidence (unchanged):** the pinned commit's own
  `pyproject.toml` declares ordinary, well-known dependencies under
  Apache-2.0; the full repository clone at that commit has zero occurrences
  of `trust_remote_code` (no opt-in to executing a model repo's own custom
  Python); `specs/022`'s "Sources" section already documents and avoids the
  separate, unrelated PyPI package also named `kev`; every pin is a full
  SHA. `typesafe-sdk` (a `kev[serve]` dependency) is published by a company
  with its own domain, docs, and GitHub org (`typesafe-ai/typesafe-sdk-python`,
  8 releases), and `mlx-lm` is Apple's own `ml-explore` project — neither
  matches the single-maintainer pattern raised about `kev`/`laya`
  themselves.
- **Recommendation (process, not code):** do not treat star/fork counts as
  evidence either way for this subsystem. Before taking a future `kev` pin
  bump, diff the new commit against the currently pinned one by hand. This
  is a design question for the user (see **Unresolved design questions**),
  not something a regression test can enforce.

#### SUPPLY-004 — The model installer places no restriction on what the base model's pinned revision may contain (confirmed currently inert)

- **Affected:** `crates/ariadne-daemon/src/ai_permissions/install.rs:300-320`
  (`weights`): the adapter `snapshot_download` call passes
  `allow_patterns=["*.json", "*.safetensors", "*.pt", "*.txt", "*.jinja"]`
  (matching Kev's own `kev/checkpoint.py:44` `resolve_run`, confirmed by
  direct comparison against the pinned source); the base-model
  `snapshot_download` call on the next line passes no `allow_patterns` at
  all.
- **Resolved, per review item 3 ("use model metadata APIs to inventory
  pinned files without downloading gigabytes").** The Hugging Face metadata
  API was queried for the exact pinned revision of all four flavours' base
  repos (`Qwen/Qwen3.5-0.8B-Base`, `-4B-Base`, `-9B-Base`, and
  `Qwen/Qwen3.8-27B`) and all four adapter repos
  (`jaredpalmer/kev-{0.8b,4b,9b,27b}`). Every adapter repo contains exactly
  `.gitattributes`, `README.md`, `adapter_config.json`,
  `adapter_model.safetensors`, `head.pt`, `provenance.json`, `result.json`,
  tokenizer/text files, and training logs; every base repo contains only
  `.gitattributes`, `LICENSE`, `README.md`, `config.json`, `merges.txt`,
  one or more `model*.safetensors` shards, `model.safetensors.index.json`,
  tokenizer/preprocessor config, and (for the 27B base) `chat_template.jinja`
  and `crc32.txt`. **No base-model revision contains a `.pt`/`.bin` file or
  anything else outside what `allow_patterns` would already select.** The
  gap is real (nothing in the code stops a future re-pinned base from
  including something broader) but, as of this scan, downloads nothing it
  should not.
- **Impact:** low. Not a reachable exposure today; a latent gap against a
  future re-pin.
- **Confidence:** confirmed inert for the current four pins (direct file
  manifest, not an estimate); the structural gap in the code (missing
  `allow_patterns`) is also confirmed by direct reading.
- **Remediation (smallest):** add the same `allow_patterns` list to the
  base `snapshot_download` call. (The earlier draft's suggestion to drop
  `"*.pt"` entirely is withdrawn pending the design question below — Kev's
  checkpoint format uses `head.pt` for every flavour including 27B, so
  `"*.pt"` cannot simply be dropped from the adapter side; it was never
  needed on the base side, since no inspected base revision has ever
  shipped one.)
- **Regression criterion:** a test asserting both `snapshot_download` calls
  in `weights()` pass the same `allow_patterns`.

### Hardening suggestions

#### SUPPLY-005 — Four Rust dependencies carry non-blocking RustSec advisories, with build-time and runtime surfaces kept distinct (corrected per review item 4)

- **Evidence:** `cargo audit` (v0.22.2, RustSec advisory-db commit
  `57dcc0f2192e025138907a495c1ac9a7e1e3e123`, 1,307 advisories, fetched live
  on 2026-10-12) found **zero** CVSS-scored vulnerabilities across both the
  workspace (557 deps) and the Tauri shell (465 deps). It reported four
  non-blocking warnings, each distinguished by where its code actually runs:
  - **`bincode 1.3.3`** — `RUSTSEC-2025-0141`, unmaintained. Pulled by
    `syntect` into `ariadne-console`, and from there into `ariadne-cli` and
    `ariadne-daemon` (`cargo tree -i bincode@1.3.3`). This **is** a
    production runtime dependency (syntax highlighting in the console) —
    distinct from the next item.
  - **`chacha20 0.10.1`** — yanked from crates.io. Pulled by `rand 0.10.2`
    into `ulid` into `ariadne-core`, reaching every workspace crate. Also a
    production runtime dependency. This audit could not retrieve the yank
    rationale (`curl` to `crates.io`'s API returned an empty body in this
    sandbox while the same tool worked against `github.com`/`api.github.com`
    and `pypi.org`) — kept as an explicit, unresolved tool/network gap, not
    papered over. A yanked version still builds from an existing
    `Cargo.lock`, but a `cargo update` without that lockfile may no longer
    be able to select it — a reproducibility risk independent of whether
    the yank was security-motivated.
  - **`proc-macro-error 1.0.4`** (`RUSTSEC-2024-0370`, unmaintained) — found
    only in `ui/src-tauri/Cargo.lock`, pulled in by `gtk3-macros`
    (`cargo tree` marks it explicitly `(proc-macro)`) from `gtk`, itself
    only reachable through Tauri's Linux WebKitGTK backend
    (`cargo tree -i proc-macro-error@1.0.4 --target all`). **Corrected
    framing, per review:** a proc-macro crate executes arbitrary Rust code
    *at compile time*, on whatever machine runs `cargo build`/`cargo check`
    with this dependency in its graph (a developer's machine or CI for a
    Linux Tauri build) — it is a build-time execution surface, not a
    runtime-reachable one in the shipped binary. The two are different
    threat models and this report no longer conflates them.
  - **`glib 0.18.5`** (`RUSTSEC-2024-0429`, unsound — a `NULL`-pointer
    dereference class issue in `VariantStrIter`, fixed in `glib >= 0.20.0`)
    — also Linux-WebKitGTK-only, via `wry → webkit2gtk/gio/gtk → glib`.
    **Call-site uncertainty is kept explicit, per review:** whether
    Ariadne Desktop's own code (or `wry`/`webkit2gtk`'s, on its behalf) ever
    exercises the specific unsound `VariantStrIter` iterator methods was
    not checked in this audit and is not claimed either way.
  - **No CVSS score is not treated as "no vulnerability."** `unmaintained`
    and `yanked` are maintenance-hygiene signals rather than exploit
    advisories, and RustSec's own `informational: unsound` classification
    for `glib` means "undefined behavior demonstrated, exploitability not
    assessed" — not "harmless." All four are reported as real findings
    with their actual, limited severity, not dismissed for lacking a score.
- **Remediation (smallest):** for `bincode`, track or replace `syntect`'s
  dependency choice upstream; for `chacha20`/`rand`, bump `rand` off the
  yanked release; for the Tauri-Linux pair, bump the GTK/WebKitGTK stack
  once `wry` ships a release pulling `glib >= 0.20.0`. None are urgent;
  group into routine maintenance.
- **Regression criterion:** `cargo audit` (both lockfiles) reports zero
  warnings for these four IDs.
- **Confidence:** confirmed for the advisory IDs, the exact dependency
  chains, and which are build-time vs. runtime vs. Linux-only; not
  independently confirmed as currently exploitable in Ariadne's own call
  sites (`glib`'s case explicitly; a dedicated reproduction per advisory was
  out of scope for this task's time budget).

#### SUPPLY-006 — npm advisories beyond shadcn's unreached CLI chain, traced to their real execution contexts (new, replaces the withdrawn npm-classification claim)

- **shadcn's own chain is confirmed unreached, correctly this time.** The
  registry's `exports` map for `shadcn@4.18.0` lists `"./tailwind.css":
  "./dist/tailwind.css"` as a direct static-file export — no `"types"`/
  `"default"` wrapper, no JS. `ui/src/index.css:3`'s
  `@import "shadcn/tailwind.css"` resolves only this static asset; `shadcn`'s
  separate `"."`, `"./mcp"`, `"./icons"`, `"./utils"`, `"./preset"`,
  `"./registry"` exports (its CLI, its own MCP server, etc. — which is why
  it depends on `@modelcontextprotocol/sdk` and, through that, `express`)
  are never touched by this import, and `grep -rn "from ['\"]shadcn"
  ui/src/` plus every `npm run` script in `ui/package.json` confirm nothing
  else in the project imports or invokes shadcn's JS. `proxy-addr`
  (critical), `undici`, `hono`, `qs`, `ip-address`, `ts-morph`/
  `@ts-morph/common`, `fast-glob`, `micromatch`, `braces` all trace to that
  unreached chain (`shadcn → {@modelcontextprotocol/sdk → {hono, express →
  {body-parser → qs, proxy-addr}}, ts-morph → @ts-morph/common → fast-glob
  → {micromatch → braces}}`) and remain correctly excluded as non-reachable.
- **What the prior version missed, per review item 4: not everything traces
  to shadcn.**
  - **`source-map-js 1.0.0-1.2.1`** (`GHSA-68fv-2mgg-jv7q`, high: event-loop
    DoS via indexed source-map section offsets) is also pulled by
    `@tailwindcss/node` (`ui/package-lock.json`: `node_modules/@tailwindcss/node
    -> source-map-js`), which is `@tailwindcss/vite`'s runtime dependency —
    a devDependency that Vite's build pipeline actually executes on every
    `npm run build` and `npm run dev`. This is genuine **build-time**
    reachability, with no `shadcn` involvement at all. Realistic exploit
    path is narrow (it would need an attacker-influenced source map
    processed during the project's own build, not an external input the
    build normally takes), but the chain is real and is recorded as such
    rather than folded into the shadcn-only claim.
  - **`js-yaml 4.0.0-4.3.1`** (`GHSA-2883-xcg3-v3hh`, high: `maxTotalMergeKeys`
    does not bound CPU use for empty merge sources) is pulled by
    `@redocly/openapi-core`, which `openapi-typescript` depends on
    (`ui/package-lock.json`), which `ui/scripts/gen-api.mjs` (run via `npm
    run gen:api`) uses to turn a **live daemon's OpenAPI document** — fetched
    from `http://127.0.0.1:7676/api-docs/openapi.json` by default, or any
    host/path a developer passes — into generated TypeScript types. This is
    a genuine **dev-tooling** reachable path: a developer who points
    `gen:api` at a compromised or malicious daemon (or spec file) could feed
    attacker-controlled YAML/JSON through `@redocly/openapi-core` and hit
    this CPU-exhaustion advisory (and, via the same dependency,
    `fast-uri`'s six advisories through `ajv`, and `postcss-selector-parser`'s
    one through a separate path). Low realistic severity (it needs an
    already-compromised or mistyped daemon target) but, again, not a
    `shadcn` finding.
  - **`@vitest/mocker`/`vitest` 2.1.0-4.1.10** (`GHSA-82fw-gwwq-j7x9`,
    moderate: path traversal / arbitrary file read via a mocker redirect)
    is reachable whenever `npm test`/`vitest` runs — i.e., on every
    `npm run check` and in CI. Exploiting it needs a malicious test file,
    which in this project's threat model means a malicious contribution to
    the test suite itself, not an external attacker; recorded for
    completeness, not elevated.
- **Remediation (smallest):** `npm audit fix` resolves `source-map-js`,
  `js-yaml`, `fast-uri`, `postcss-selector-parser`, and `@vitest/mocker`/
  `vitest` without a major bump (`fixAvailable: true` for all of them, per
  the audit JSON); the `shadcn`-rooted chain's fixes are all
  `isSemVerMajor: true` against `shadcn` itself and can be deferred since
  nothing reaches them.
- **Regression criterion:** `npm audit --json`'s per-package `fixAvailable`
  no longer lists `source-map-js`, `js-yaml`, `fast-uri`,
  `postcss-selector-parser`, or `@vitest/mocker`/`vitest`.
- **Confidence:** confirmed for every chain traced above (each is a direct
  read of `ui/package-lock.json`'s own recorded `dependencies`, not an
  inference). `ajv`'s remaining parent, `conf`, was not traced further
  (used by some CLI tool's config storage, likely `@tauri-apps/cli`'s or
  similar dev tooling) — left as an explicit minor gap rather than guessed
  at.

### Reviewed, no finding

- **Shared cache/venv boundary (`bench/ai-opportunities`), per review item 3.**
  `bench/ai-opportunities/progress/bench_progress/kev_runner.py` reuses the
  **production** Kev virtual environment (`~/.ariadne/ai-permissions/venv/bin/python3`)
  read-only — it never installs into or writes to it — while writing model
  weights into a **separate** `HF_HOME`
  (`~/.ariadne/ai-opportunities/hf`), distinct from the production
  `ai-permissions/hf` cache. Before running, `check_kev_revision` reads the
  installed `kev` package's `direct_url.json` and refuses to proceed if its
  `vcs_info.commit_id` does not match the required pinned revision exactly.
  An OS advisory lock (`fcntl.flock`) serializes access, released
  automatically on process exit by any means. This is a reasonable design:
  a benchmark sharing the production install for speed, with an explicit
  revision check standing in for "don't silently run against a different
  pin than you think you are."
- **Partial-install recovery (`install.rs`).** `venv()` (lines 220-248)
  rebuilds the virtual environment from scratch if the existing interpreter
  fails a version probe, which also covers a venv left in a half-built
  state by an interrupted `python -m venv`. `package()`'s `pip install`
  calls (lines 256-272) are idempotent — a second run against a
  partially-installed venv simply continues/overwrites — and the
  Linux `TORCH_DEVICE` marker file is written only *after* every `pip`
  call in the batch succeeds (line 267-269), so an interruption mid-install
  cannot leave a stale marker claiming a device was installed when it was
  not. No gap found here.
- **Cache/directory ownership.** `ai_permissions.home` and its `venv`/`hf`
  subdirectories are created with `std::fs::create_dir_all` under the
  daemon's own process user, with no explicit permission hardening beyond
  the default umask — standard for a single-user desktop daemon's own
  per-user data directory, and consistent with this being a single-user
  desktop application threat model rather than a multi-tenant service. Not
  raised as a finding.
- **PATH-based Python resolution (`python.rs:24-34`).** Resolves
  `python3.13`/`python3.12`/`python3`, or a configured `python_bin`, via
  the daemon's own `PATH` — the same trust model the daemon already applies
  to resolving `git` and other external tools elsewhere, not a distinguishing
  supply-chain gap specific to this subsystem.

### Rejected hypotheses

- **"The Rust lockfiles hide a substituted/typosquatted crate."** Narrowed
  per review item 4: what was actually checked is that every dependency in
  both lockfiles resolves from `registry+https://github.com/rust-lang/crates.io-index`
  or the workspace's own `path =` members, with the checksum crates.io
  itself recorded — i.e., no crate resolves from an unexpected source or has
  had its registry-recorded content silently substituted since it was
  locked. That is **not** the same claim as "no dependency name in this
  557+465-crate graph was ever chosen as a typosquat of a more popular
  crate" — this audit did not manually review all ~1,000 crate names for
  edit-distance to a popular alternative, and does not claim to have.
  Rejected only in the narrow sense tested; the broader claim is an
  unreviewed gap, not a pass.
- **"`pip install kev[serve] @ git+...` is vulnerable to dependency confusion
  with the unrelated PyPI `kev` package."** Checked: the installer
  (`install.rs:295`) always supplies the explicit `git+https://...@<sha>`
  URL, never a bare `kev` name PyPI's resolver could satisfy from the wrong
  package; `specs/022` documents this distinction explicitly. Rejected for
  the production path. (The benchmark's unpinned Kev install also always
  names the VCS URL — its gap is the missing commit pin, SUPPLY-001, not
  name confusion.)
- **"The model server (`kev.serve`) executes attacker-controlled code via
  Hugging Face's `trust_remote_code`."** Checked via a direct `grep` of a
  full local clone of the pinned commit (not GitHub's code-search index,
  per the Corrections above): zero occurrences of `trust_remote_code`
  anywhere in the repository. Rejected, with the caveat that this covers
  code present in the `kev` repository itself, not every line of
  `transformers`/`peft` it depends on.
- **"`shadcn`'s vulnerable transitive dependencies ship inside the Tauri
  desktop bundle, or are reachable via its CSS import."** Checked
  thoroughly this time (see Withdrawn findings and SUPPLY-006): the CSS
  subpath export carries no JS, no source file imports `shadcn`'s JS, and
  no build/dev/test script invokes it. Rejected as a runtime vulnerability.

## Withdrawn findings

### Former SUPPLY-003 — "`shadcn` misclassified as a production dependency" (withdrawn in full)

The reviewed version of this report claimed `ui/package.json` misclassifies
`shadcn` as a production dependency, that it is "never imported," and
recommended moving it to `devDependencies`. **This was wrong, and the
recommendation would have broken the build.** `ui/src/index.css:3` reads:

```css
@import "shadcn/tailwind.css";
```

and `ui/AGENTS.md:447-449` states exactly why, in the project's own words:

> `shadcn` itself stays in `dependencies` rather than `devDependencies`:
> `src/index.css` imports `shadcn/tailwind.css`, so it is a runtime
> dependency and not only the scaffolding CLI.

The original evidence-gathering step (`grep -rn "from ['\"]shadcn"
ui/src/`) only looked for a JavaScript import and missed the CSS
`@import`, which resolves through the same `node_modules` package. The
corrected, narrower claim — that `shadcn`'s *own CLI dependency chain*
(`@modelcontextprotocol/sdk`, `express`, `ts-morph`, etc.) remains unreached
regardless of the CSS import, because CSS and JS resolve through disjoint
`exports` subpaths — is still true and is carried forward correctly in
**SUPPLY-006**.

## Recommendation

**SUPPLY-003** (the `torch.load`/CVE-2026-24747 finding) is the headline
result of this revision and should be treated as the priority: it is a
confirmed, high-severity, reached vulnerability in a dependency range Kev's
own upstream `pyproject.toml` cannot currently escape, not a hypothetical.
**SUPPLY-001** remains the next most actionable item — benchmark-only, but
zero-friction to fix, since production code already demonstrates the
correct pattern (commit pinning) for the one half of it (hash-checking a
direct wheel URL) this audit could cleanly prove end to end. **SUPPLY-002**
is a real but lower-priority process question — do not let it compete with
SUPPLY-003 for attention, per the review. **SUPPLY-004**, **SUPPLY-005**,
and **SUPPLY-006** are routine hardening, none urgent.

## Confidence

High confidence in every finding's facts: each cites a command, its
output, a primary-source advisory record, or a file and line, gathered
this revision from primary sources rather than a search index or a
summary. The two places confidence is intentionally split, both disclosed
rather than smoothed over: SUPPLY-002 (metrics confirmed, intent not, and
explicitly not claimed) and SUPPLY-001's third reproduction claim (the
mechanism is well-documented and relied on by the project's own tests, but
this audit's own attempt to demonstrate it locally was inconclusive, and
says so).

## Unresolved design questions for the orchestrator

1. **SUPPLY-003 (new, highest priority):** Ariadne has no direct way to
   raise Kev's torch ceiling past the CVE-2026-24747 fix version (2.10.0)
   without either upstream Kev widening its own declared range, or Ariadne
   forcing a torch version Kev was not tested against. Which tradeoff does
   the user want — wait on upstream, force an untested torch version, or
   accept the risk bounded by the existing pin/trust chain, with a note to
   revisit when Kev's range changes?
2. Should the benchmark's Laya/Kev pinning (SUPPLY-001) be a follow-up task
   now, given `bench/AGENTS.md` already states this path never runs in CI?
3. Does the user want a standing review process (not a one-time fix) for
   future `kev` pin bumps, given SUPPLY-002's trust concern and SUPPLY-003's
   demonstration that a hostile pinned revision's impact is now known to be
   "potential arbitrary code execution," not merely "a biased decision"?
4. SUPPLY-004's remediation proposes adding `allow_patterns` to the base
   `snapshot_download` call, keeping `"*.pt"` in both lists (since `head.pt`
   is load-bearing for every flavour including 27B). Confirm this is
   acceptable before a remediation task changes it.

## Cross-boundary evidence for RELEASE

None found. This audit's work stayed inside package and model-artifact
resolution (`Cargo.*`, `ui/package*.json`, `ui/src-tauri/Cargo.*`, the
AI-permission install/serve code, and `bench/`), not GitHub Actions,
release artifacts, or application installation — RELEASE's area per the
cross-task contract. No finding here implicates a GitHub Actions workflow,
a release artifact, or the installer/uninstaller scripts under `scripts/`.
