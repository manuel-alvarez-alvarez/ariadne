# Proposed isolated Linux CI measurements

Status: proposal for user agreement; no implementation or dispatch is authorized.
The workflow was removed and was never dispatched; this page stays as history.
The reader must approve the experiment, its limits, and its runner budget before implementation.

## Question and recommendation

Which Linux stages dominate when downloads, build artifacts, and compiler caches have explicit initial conditions?
Approve three independent cold/warm pairs for Rust, Tauri, and both frontend packages.
Add a Rust compiler-cache replay to each pair.
Use runner-local experiment caches, with exact keys and no GitHub cache-service access.
This choice preserves shared caches and avoids quota-driven eviction of existing entries.
It measures retained local caches, rather than GitHub cache restoration latency.
A later cache-service experiment would need separate agreement.

## Evidence and remaining gaps

- The [audit](README.md#acceptance-gaps) lacks controlled Linux repetitions and independent Linux linker attribution.
- The [baseline logs](ci-excerpts.md) show restored caches, 45.21-second builds, and 140.741-second test execution.
- Those Linux logs show zero sccache hits, twelve misses, and 48 non-cacheable calls.
- The [adjacent run](adjacent-ci.md) changes source revision and cannot provide baseline repetitions.
- The [local method](reproduction.md) retains download caches and supplies macOS linker-driver measurements only.
- The [contention report](contention.md) measures macOS checks, rather than isolated hosted Linux jobs.
- Current [CI](../../.github/workflows/ci.yml) uses floating toolchains and runner labels, dependency caches, and the sccache GitHub backend.
- CI restricts shared-cache writes to main; PRs restore dependency caches and use sccache read-only mode.

These observations do not establish cold download costs, Linux link costs, cache hit benefits, or same-revision distributions.
No new measurement supports a performance benefit estimate.

## Fixed source and environment

Use source revision `4f71da2465954107f113d639b5d7bf666c8446a9` for every observation.
Verify checkout HEAD before setup and each phase.
Record workflow implementation SHA separately; never substitute it for the measured source SHA.

This revision includes dependency fix `dad16ceee3d15683938cb40a07b262b86d7c8f73`.
That commit updates url to 2.5.9 and repairs a relay test's cross-filesystem hard link.
The current lockfile contains url 2.5.9; the historical baseline contains the advisory described in the audit.
Keep all historical observations separate, including the later macOS contention observations.
A later dependency fix requires a new source SHA and a complete new cohort.
Do not pool old and new observations, even when only a lockfile changes.

Proposed runner: standard GitHub-hosted x64 `ubuntu-26.04`, without containers or larger runners.
Require `ImageOS=ubuntu26` and `ImageVersion=20261116.202.1`, matching the recorded Linux Rust baseline.
The label selects an OS family; it does not pin that image build.
Abort before measurement if GitHub no longer supplies that version.
Ask for agreement on a replacement image version before another cohort.
Do not provision custom images or self-hosted runners.
Record CPU model, CPU count, RAM, kernel, filesystem capacity, image version, and runner version.
[GitHub documents](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) fresh standard VMs and their resource differences.

Pin Rust and Cargo to 1.100.0, with matching clippy and rustfmt components.
Pin cargo-nextest to 0.9.146, sccache to 0.16.0, and cargo-machete to 0.9.2.
Pin cargo-audit to 0.22.0, Node to 26.12.0, and npm to 11.20.0.
Use the image's Python 3, GNU time, GCC, binutils, and tmux; record their exact versions.
Install missing tmux and Tauri packages before timing, using CI's existing apt package lists.
Record resolved apt versions and reject different package manifests within the cohort.
Verify pinned tool availability during implementation review; unavailable pins require a revised proposal.
Pin checkout, tool installers, and artifact actions to reviewed immutable SHAs in the later implementation.
Record those SHAs in the evidence manifest.

## Proposed files and trigger

Add `.github/workflows/linux-latency-experiment.yml` after agreement.
Add `.github/experiments/linux-latency/run.py` for bounded timing and evidence collection.
Add `.github/experiments/linux-latency/linker.sh` for linker-driver timing.
These are proposed files, not files created by this task.
Keep the experiment separate from `ci.yml`, release workflows, hooks, and required checks.

Use only `workflow_dispatch`, with no push, PR, schedule, or workflow-run trigger.
Accept one input, `experiment_id`, matching `[a-z0-9-]{1,32}`.
Hard-code the source SHA and all versions above; do not accept arbitrary commands or source refs.
Use one nine-entry matrix: three lanes multiplied by repetitions 1, 2, and 3.
Set `max-parallel: 1`, `fail-fast: false`, and a separate experiment concurrency group.
Reject overlapping dispatches with the same experiment ID; never cancel ordinary CI.
A cold/warm pair stays on one VM and uses the same absolute checkout and wrapper paths.

## Cache keys and conditions

Define `K=latency-linux-v1-<experiment_id>-<run_id>-<run_attempt>-<source_sha>-ubuntu26-20261116.202.1-<lane>-<repetition>`.
Use exact local directory keys `K-cargo`, `K-target`, `K-sccache`, and `K-npm` beneath `$RUNNER_TEMP`.
Include tool versions and lockfile SHA256 values in each directory's manifest.
Never use fallback keys, restore prefixes, setup-node caching, rust-cache, or actions/cache.
Disable `SCCACHE_GHA_ENABLED` and every remote sccache backend.
Use `SCCACHE_DIR=K-sccache`, `CARGO_HOME=K-cargo`, and `CARGO_TARGET_DIR=K-target`.
Set npm's cache to `K-npm` for both frontend packages.
Use `CARGO_INCREMENTAL=0`, `RUSTC_WRAPPER=sccache`, and `RUST_BACKTRACE=1` for workspace Rust.
Keep Tauri's wrapper unset, matching CI.

| Phase | Downloads | Build artifacts | Compiler cache | Meaning |
| --- | --- | --- | --- | --- |
| Cold | Empty private Cargo/npm caches | Empty target; absent node_modules and TypeScript state | Empty local sccache | First complete check chain |
| Warm | Retain cold downloads | Retain complete cold artifacts | Retain populated sccache | Immediate identical chain repeat |
| Rust cache replay | Retain cold downloads | New empty `K-replay-target` | Retain cold/warm sccache | Compiler reuse without Cargo artifact reuse |

Install tools outside these private caches before starting the cold phase.
Do not prefetch project dependencies into cold caches.
Cold means empty application caches, not flushed OS page caches or cold registry servers.
Warm commands retain preceding phase state; individual steps are not independently cold builds.
Never label an npm warm installation as dependency-tree reuse: `npm ci` removes and recreates node_modules.
Retain TypeScript state between warm checks, and record any files npm scripts remove.

## Exact command chains

Run each chain once cold and once warm, in this order.
Run the Rust chain again with the replay target for compiler-cache attribution.
Record downloads separately from compilation; setup and upload time remain separate overhead.

| Lane | Commands, in order |
| --- | --- |
| Rust | `cargo machete .`; `cargo audit`; `cargo build --locked --workspace --bins --timings`; `cargo nextest run --workspace --profile ci --no-run`; `cargo nextest run --workspace --profile ci`; `cargo test --doc --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `scripts/check-unused-rust`; `scripts/check-migrations.sh` |
| Tauri | From `ui/src-tauri`: `cargo check --locked --timings` |
| Frontend | From `ui/`, then `ui/dashboard/`: `npm ci`; `npm run check:audit`; `npm run lint`; `npm run typecheck`; `npm run check:unused`; `npm test` |

Splitting nextest compilation adds a measurement boundary; the execution command can still compile residual units.
Report the runner's execution summary separately from command wall time and residual compilation.
Fetch full source history and main for the migration guard, as ordinary CI does.
Record the fetched main SHA because migration validation reads that moving reference.
Retain default assertions, isolation, worker counts, flaky markers, and nextest retry settings.
Add no retry policy and skip no tests.
Formatting remains in ordinary CI; frontend bundling and release builds remain outside this experiment.

## Repetitions, attribution, and artifacts

Use three independent VMs per lane, each with its own keyed caches.
This gives three cold and three warm observations per command, plus three Rust replay observations.
Do not silently replace failed observations or automatically redispatch failed pairs.
Mark downstream observations invalid when their required preceding phase fails.
Report every exit code, failure, timeout, and test count beside durations.
Report min, median, max, and paired warm/cold ratios only for comparable valid observations.
Three pairs describe this cohort; they do not establish general speedup or statistical significance.

Use monotonic wall timing and cumulative child user/system CPU timing around foreground process groups.
Bound each command to 600 seconds; terminate its process group and record timeout status 124.
Set Rust jobs to 50 minutes, Tauri jobs to 30 minutes, and frontend jobs to 25 minutes.
Reserve the final five minutes for failure evidence and upload; stop new commands at that deadline.
A command failure stops its chain, but evidence upload runs with `always()`.

Set `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER` to one stable experiment wrapper path.
Invoke `/usr/bin/cc` with unchanged arguments through GNU time.
Record one file per invocation, with monotonic start/end, PID, output artifact, elapsed, user, and system time.
Associate calls with the current command and output binary; preserve Cargo timing HTML and verbose compiler command logs.
Record `cc --version`, `ld --version`, linker selection, and rustc `-Vv`.
Treat these as linker-driver costs, including driver overhead, rather than pure linker internals.
Never subtract overlapping linker sums from command wall time.
Separate final binaries, build scripts, procedural macros, and other library links in the report.
Capture sccache JSON statistics before and after each command, including misses and non-cacheable reasons.
Compiler CPU and Cargo unit timelines supplement linker attribution; they do not provide exclusive compilation wall time.
The instrumentation adds overhead and must remain identical across all phases.

Upload `K-evidence` with 14-day retention and a two-GiB maximum per job.
Include manifests, exact commands, timestamps, CPU/resource samples, exit codes, raw command logs, and timing tables.
Include Cargo timing reports, linker records, cache statistics, JUnit files, test summaries, and lockfile hashes.
Sample CPU, memory, disk use, and I/O every second using a collector tied to its parent process.
Stop the collector on success, failure, cancellation, or timeout.
Never upload caches, tokens, full environment dumps, dependency trees, or compiled targets.
Record artifact omissions and upload failures; do not silently truncate primary timing evidence.

## Cost, permissions, dispatch, and cleanup

Planning estimate: Rust 3 × 30 minutes, Tauri 3 × 15 minutes, frontend 3 × 12 minutes.
Total estimate: 171 Linux runner minutes, including setup and evidence upload.
Job timeout ceiling: 315 runner minutes across nine jobs.
These are budget estimates, not measurements or permission to spend runner minutes.
Cold compilation and package downloads could exceed the estimate or hit command timeouts.
Abort and report that result rather than increasing limits without agreement.

Request only `contents: read`; use the normal artifact service without repository write permissions.
Use no secrets, OIDC, deployment environment, ruleset change, or cache-service token export.
The dispatch operator needs repository write access and separate user approval to start the experiment.
[GitHub requires](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow) the dispatch workflow on the default branch.
Landing that workflow is a separate, approved implementation action.
A workflow on this task branch alone does not satisfy that requirement.

After implementation review and explicit dispatch approval:

1. Verify the approved workflow commit, fixed source SHA, tool manifest, image version, and available runner budget.
2. Run `gh workflow run linux-latency-experiment.yml --ref main -f experiment_id=linux-v1-001`.
3. Record the run URL and workflow SHA, then collect all nine job artifacts.
4. Publish separate cold, warm, and replay tables for this source revision and environment.
5. Let runner teardown remove private caches and targets after each job.
6. Let artifacts expire after 14 days, or delete only this experiment's artifacts with separate operator authorization.
7. Remove the manual workflow through a later reviewed change when the experiment ends.

Do not delete, reset, or rewrite existing shared caches.
Do not alter ordinary CI triggers, checks, branch rules, or permissions.
This task creates only this proposal and its index link.
The displayed dispatch command is a future procedure; this task does not execute it.

## Confidence and decision

Confidence is high that these conditions separate artifact reuse from compiler-cache reuse.
Confidence is low for runtime forecasts and expected performance benefits.
The strongest counterargument is that local-cache replay differs from CI's dependency-only artifact pruning and remote cache transport.
Therefore, these results cannot establish rust-cache restoration speed or production sccache hit rates.
An obsolete hosted image could also prevent the proposed cohort from starting.

Approve the fixed cohort, nine jobs, local cache keys, 171-minute estimate, and 315-minute ceiling before implementation.
Choose a replacement image through proposal revision if the specified hosted version is unavailable.
Require another agreement before dispatch, infrastructure changes, or any cache-service experiment.
Tool availability, current hosted image availability, live advisory status, and repository dispatch permissions remain unchecked.
No local timing, workflow edit, dispatch, dependency change, or infrastructure action occurred during this proposal.
