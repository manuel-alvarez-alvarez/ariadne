# Build and test latency audit

The reader maintains Ariadne checks and must select optimization tasks from measured evidence.
Which stages dominate macOS development and Linux CI latency without reducing checks or test isolation?

The strongest measured candidate is Rust test execution, followed by test compilation and linking.
The evidence supports investigation tasks, but does not establish safe savings.
Obtain user agreement on task boundaries before implementation.

## Baseline and method

Baseline: `0561a6ab4a5864186724fab5bcf783030359298b`, measured on 2026-11-22.
The task branch started clean at that revision.
This directory contains reports only; no check or production behavior changes.
The follow-up measures the same source baseline from report revision `cb3a7efdcad2ca10d9a7ccf00b6d12acb0629556`.

Sources:

- [Proposed isolated Linux CI experiment](linux-ci-proposal.md) defines the cohort and decisions for user agreement.

- [macOS contention follow-up](contention.md) compares identical checks across two worktrees at a separately pinned revision.
- [CI step evidence](ci-evidence.md) records every job step and its duration.
- [CI excerpts](ci-excerpts.md) preserve compiler, runner, cache, and test summaries.
- [Sequential follow-up evidence](followup-evidence.md) records complete local checks with explicit repetitions and cache conditions.
- [Follow-up reproduction](reproduction.md) describes independent targets, snapshots, CPU timing, and linker measurement.
- [Adjacent CI observations](adjacent-ci.md) record another revision without treating it as a baseline repetition.
- [Local evidence](local-evidence.md) records commands, repetitions, elapsed times, and results.
- [Hooks](../../.githooks/pre-push), [commit checks](../../.githooks/pre-commit), and [routing](../../.githooks/lib.sh) define local checks.
- [CI](../../.github/workflows/ci.yml) defines hosted checks and caches.
- [Rust conventions](../../crates/AGENTS.md) and [UI conventions](../../ui/AGENTS.md) define reliability requirements.

Local environment: macOS 27.0.1, build 26A434, arm64, 16 logical CPUs, 64 GiB RAM.
The initial filesystem had 208 GiB available.
Rust was Homebrew 1.99.0, LLVM 23.1.2; nextest was 0.9.146.
Node was 26.12.0; npm was 11.20.0.
`CARGO_TARGET_DIR`, `RUSTC_WRAPPER`, and `CARGO_INCREMENTAL` were unset.
The worktree initially lacked `target`, `ui/node_modules`, and `ui/dashboard/node_modules`.
Cargo and npm download caches existed outside the worktree; their contents were not cleared.
Thus, “cold” means missing local build artifacts, not cold downloads or filesystem caches.
No machine-wide cache, compiler setting, or infrastructure configuration changed.

The original local runner used Python `time.monotonic()` around foreground subprocesses.
Each original command had a 240-second timeout and a separate `/tmp/latency-*.log` file.
Timeout handling sent SIGTERM to the process group.
Local repetitions used unchanged sources and retained artifacts.
The daemon build started after the core sample, so its dependency artifacts were partly warm.
Later UI runs overlapped daemon compilation; they are shared-host observations, not isolated comparisons.

CI observations use one successful existing run at the exact baseline.
Linux used `ubuntu-26.04`; macOS used `macos-26-arm64`.
Both Rust jobs used 1.100.0; local Rust therefore differs.
The excerpts preserve runner image versions.
API step timestamps have second resolution; runner summaries provide finer execution durations.
CI restored dependency artifacts and npm downloads.
Neither Rust job recorded an sccache hit: macOS had 18 misses; Linux had 12 misses.
A restored dependency cache does not prove a warm compiler cache.

The original observations below remain historical evidence.
Use the sequential follow-up for local condition comparisons.
The adjacent CI run changes manifests and lockfiles, so its observations cannot increase baseline repetition counts.
No new workflow run was dispatched; user agreement on infrastructure remains pending.

## Complete check chain

Commands below describe the baseline, rather than proposals to remove work.

| Boundary | Ordered work | Scope and overhead |
| --- | --- | --- |
| Commit, Rust | `cargo fmt --all -- --check`; crate clippy with `--all-targets -- -D warnings` | Root manifests select workspace clippy; staged manifests also select `cargo machete .` |
| Commit, desktop | missing dependencies: `npm ci`; `npx biome check <staged files>`; `npm run typecheck` | Generated files select types alone |
| Commit, dashboard | same commands inside `ui/dashboard` | Separate dependency tree |
| Commit, Tauri | `cargo fmt --all -- --check` inside `ui/src-tauri` | Separate Cargo workspace |
| Commit, shell | `shellcheck -x <staged script>` | Includes hook scripts; release configuration also selects its scenario test |
| Commit message | Conventional Commit validator | Does not compile |
| Push, Rust | `cargo machete .`; `cargo audit`; `cargo build --locked --workspace --bins`; doctests; workspace clippy; fmt; unused-public audit; migration guard | Doctests use `cargo test --doc --workspace` |
| Push, desktop and dashboard | missing dependencies: `npm ci`; `npm run check:audit`; `npm run lint`; `npm run typecheck`; `npm run check:unused` | Each package runs separately |
| Push, Tauri | fmt; `cargo check` | Does not link or bundle the desktop app |
| Main push or `--full` | all push checks, workspace nextest with `--profile ci`, both `npm test` suites, desktop `npm run build` | Desktop build also builds dashboard; installs dashboard dependencies first |
| Task push | select changed areas from outgoing range | Omits normal tests and frontend build; retains doctests |
| `pre-push --all` | all areas, ordinary push checks | Does not select full test suites; no-input invocation also selects all areas |
| CI Rust, both OSes | install tools and tmux; restore artifacts; machete; audit; bins; nextest; doctests; clippy; unused-public audit; migrations | Adds checkout, cache, JUnit reporting, and sccache statistics |
| CI Rustfmt | workspace fmt; Tauri fmt | Ubuntu only |
| CI desktop and dashboard | setup Node; restore npm downloads; `npm ci`; audit; lint; types; unused; tests | Separate Ubuntu jobs; JUnit reporting |
| CI Tauri, both OSes | restore artifacts; Linux system dependencies; `cargo check` | Linux installation used 45 seconds |
| Shell checks | commit hook; manual `shellcheck scripts/*.sh` and `shellcheck .githooks/*` | Baseline CI has no shell job |
| Shell scenarios | installer, service, signing, hooks, migrations, release notes, release configuration | Manual boundary checks; complete commands and case names live in `scripts/AGENTS.md` |

[Release builds](../../.github/workflows/release.yml) also install both npm trees and run release Cargo builds and Tauri builds.
They add signing, packaging, attestations, and upload work.
[Source installation](../../scripts/install.sh) also uses release builds and optional desktop packaging.
Those paths were inventoried, but were not executed or timed here.
Their publication and installation side effects exceed this report's measurement scope.

## Measured stage contributions

These are observed durations, not predicted savings.
Do not add parallel job durations to estimate the workflow critical path.

| Stage | macOS CI seconds | Linux CI seconds | Interpretation |
| --- | ---: | ---: | --- |
| Install tmux | 16 | 6 | System dependency installation |
| Rust artifact restore | 21 | 11 | Download and extraction overhead |
| Shipped binary build | 88 | 45 | Compilation plus linking; no independent linker measurement |
| Nextest compilation | about 152 | about 72 | Step start to runner start, including enumeration overhead |
| Nextest execution | 318.028 | 140.741 | 2589 and 2587 tests; 13 binaries each |
| Complete nextest step | 471 | 213 | Includes compilation and runner overhead |
| Doctests | 10 | 3 | Compilation and execution combined |
| Clippy | 63 | 32 | Separate check artifacts and all targets |
| Unused-public audit | 18 | 13 | Source analysis overhead |
| Tauri check | 6 | 10 | No application link |

Linux desktop test execution reported 84.67 seconds.
Its tracked time split was tests 39%, imports 31%, environment 21%, transform 5%, and setup 4%.
Dashboard reported 2.47 seconds; imports accounted for 51%, while tests accounted for 3%.
Those percentages describe runner tracking, not independent sequential wall-time buckets.
CI installation took four seconds for desktop dependencies and seven seconds for dashboard dependencies.

The original core sample does not represent a full workspace cold build.
The follow-up measures workspace binaries and workspace test builds in independent empty targets, twice per condition.
It also measures separate linker-driver calls, cumulative child CPU, and all local check commands.
The CI table still combines compilation and linking; local linker data does not retrospectively split CI stages.
The follow-up table separates dependency installation, compilation, test execution, and check overhead.
Source-only first invocations are not cold compiler builds.

## Sequential local follow-up

Each cell shows its observed range and repetition count.
First invocation and warm repetition use the conditions in [the method](reproduction.md).
They do not mean cold network or filesystem caches.

| Stage | Cold artifacts or first invocation, seconds | Warm artifacts or repeat, seconds |
| --- | ---: | ---: |
| Workspace binary build | 33.602–33.704 (n=2) | 0.207–0.208 (n=2) |
| Workspace test compilation | 49.655–49.856 (n=2) | 0.484–0.487 (n=2) |
| Clippy | 32.196–32.404 (n=2) | 0.257–3.792 (n=2) |
| Tauri compilation | 15.149–15.338 (n=2) | 0.140–0.141 (n=2) |
| Doctests | 23.245–23.245 (n=1) | 1.534–1.534 (n=1) |
| Desktop dependency installation | 2.337–2.401 (n=2) | 3.290–3.360 (n=2) |
| Desktop lint | 0.958–0.967 (n=2) | 0.300–0.312 (n=2) |
| Desktop types | 3.985–4.040 (n=2) | 3.841–3.883 (n=2) |
| Desktop tests | 35.550–35.581 (n=2) | 34.319–34.349 (n=2) |
| Dashboard dependency installation | 4.019–4.092 (n=2) | 6.290–6.363 (n=2) |
| Dashboard lint | 0.750–0.856 (n=2) | 0.230–0.248 (n=2) |
| Dashboard types | 1.125–1.128 (n=2) | 1.005–1.014 (n=2) |
| Dashboard tests | 2.063–2.110 (n=2) | 1.819–1.846 (n=2) |
| Combined frontend bundle | 11.862–12.643 (n=2) | 11.602–12.046 (n=2) |
| Dependency audit, failed | 2.512–2.512 (n=1) | 1.860–1.860 (n=1) |
| Unused dependencies | 0.111–0.111 (n=1) | 0.082–0.082 (n=1) |
| Unused public items | 9.044–9.044 (n=1) | 8.884–8.884 (n=1) |
| Migration guard | 0.531–0.531 (n=1) | 0.322–0.322 (n=1) |
| Root formatting | 0.877–0.877 (n=1) | 0.856–0.856 (n=1) |
| Tauri formatting | 0.230–0.230 (n=1) | 0.049–0.049 (n=1) |
| Shellcheck | 1.240–1.240 (n=1) | 1.159–1.159 (n=1) |

Each fresh npm installation uses the existing download cache.
Repeated `npm ci` runs started with existing snapshot installations and retained download caches.
The command removes and recreates those installations; these timings include that removal overhead.
Both packages also passed their advisory wrappers and unused-code checks; the evidence records every command.

Workspace runner execution took 56.379 and 55.800 seconds, with 2589 tests passing in each observation.
Their command totals were 64.660 and 61.706 seconds.
Residual compilation took 6.99 and 4.54 seconds; each command also invoked three linker-driver calls.
Those totals therefore do not represent pure runner execution.
Neither follow-up workspace run reported a slow test.

Cold binary builds recorded 80 linker-driver calls each, totalling 4.95 and 5.05 driver seconds.
Cold workspace test builds recorded 93 calls each, totalling 6.51 and 6.95 driver seconds.
The longest individual calls were 0.28–0.32 seconds across these cold builds.
These overlapping driver durations do not determine the compilation wall-time remainder.
The evidence preserves separate CPU totals and link measurements for each Rust command.

The first frontend snapshot setup failed because repository-relative inputs were missing.
Those invalid observations are excluded, and [the method](reproduction.md) records the correction.
No test, assertion, dependency, or check changed.

## Concurrency, caches, and temporary checkouts

The original concurrency sample overlapped daemon compilation and UI work.
It cannot measure contention between equal agent workloads.
The original UI measurements also cannot support an idle-host comparison with CI.
The historical evidence remains labelled, but no ranking or benefit claim uses those numbers.
An isolated host window requires orchestrator coordination; this session cannot stop unrelated host workloads.

The main-push hook creates a detached temporary worktree at the outgoing commit.
It shares the caller's Cargo target directory, then removes the temporary checkout.
It does not share either npm installation directory.
Therefore, every fresh main candidate needs both `npm ci` installations.
Temporary source paths can invalidate workspace build fingerprints; this is a hypothesis requiring repeated checkout measurements.
The original lock observations do not quantify the cost of target sharing between equal agent workloads.
No target-sharing recommendation follows from that sample.

CI keeps separate Rust and Tauri dependency caches per OS.
Only main saves caches; PRs restore them and use sccache read-only mode.
CI disables incremental compilation, while local development retains Cargo defaults.
The [development profile](../../Cargo.toml) retains workspace line tables and removes dependency debug information.
These profile differences prevent direct local-to-CI speedup claims.

Repeated fmt, types, clippy, and audits occur at commit, push, and CI boundaries.
Each boundary protects a different candidate or environment.
Desktop build starts with `tsc -b` after the separate typecheck command.
This can reuse TypeScript state; repeated command names do not prove repeated full compilation.
The current daemon integration layout already combines modules into one `it` binary.
The shipped daemon binary has `test = false`; nextest cannot replace its required link check.

## Waits, startup, fixtures, and cleanup review

The following review is static unless a timing is cited.
The source paths refer to the daemon integration [harness](../../crates/ariadne-daemon/tests/it/common/mod.rs),
[ACP stub](../../crates/ariadne-daemon/tests/it/common/acp.rs), and [host tests](../../crates/ariadne-daemon/tests/it/hosts.rs).
The frontend configurations are [desktop](../../ui/vite.config.ts) and [dashboard](../../ui/dashboard/vite.config.ts).

| Surface | Evidence | Assessment and preserved requirement |
| --- | --- | --- |
| Positive synchronization | `common/mod.rs:2383` onwards: `eventually`, `eventually_some` | Check immediately, then poll every 10 ms; maximum patience is not unconditional waiting |
| Negative assertions | `common/mod.rs:76`: `QUIET`, 100 ms | Preserve absence windows unless an ordered event proves absence |
| Event subscriptions | `common/mod.rs:1108`: subscribe before launch | Preserve ordering; late subscriptions can lose events |
| Database setup | `common/mod.rs:683` onwards: executable-keyed database template | One builder migrates and seeds; every harness copies a private database |
| Fixture synchronization | `common/mod.rs:850` onwards: nonblocking `flock` attempts | Preserve process-safe publication and recovery after builder death |
| Git fixtures | `common/mod.rs:745` onwards: private repository and bare-origin copies | Already avoids repeated init and commits; preserve per-test mutation isolation |
| Process startup | `common/acp.rs:435` onwards: Python ACP stub | Startup cost remains unseparated; shared executable launcher already avoids executable creation per test |
| Stub timers | `common/acp.rs:453`, `:466`, `:578`, `:654` | Parent watchdog, intentional startup delay, and marker synchronization have different purposes |
| External commands | `hosts.rs:4430` onwards: four concurrent maintenance cases | Shell, SSH, runtime, and fixture startup need tracing before blaming a timer |
| Cleanup | `common/mod.rs:116` onwards: relay stop and private tmux termination | Keep failure-path cleanup; do not remove it to reduce latency |
| Telemetry cleanup | `common/mod.rs:139` onwards: shutdown on another thread | Already avoids blocking the test thread; shutdown cost remains unmeasured |
| UI isolation | both Vite configurations: four workers; per-file setup | Preserve local mocks, reset state, browser resolution, and separate package trees |

macOS CI marked three host tests slow:

- `hosts::an_update_names_the_runtime_that_homebrew_did_not_provide`.
- `hosts::an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed`.
- `hosts::daemon_version_installs_the_recorded_release_of_a_release_daemon`.

The first test runs two cases sequentially; the runtime-upgrade test already runs four cases concurrently.
These observations contradict a blanket proposal to parallelize every fixture case.
A slow marker proves elapsed time, not avoidable waiting or an incorrect synchronization method.
Linux recorded one existing flaky result; this report adds no marker or retry policy.
The second original daemon run reported one slow test without naming it under failures-only output.
Use `--final-status-level slow` on a later run to retain slow test names without printing every passed test.
A later slow result cannot identify the original unnamed result retrospectively.
Both local daemon runs passed all 1483 tests; the slow test identity remains unavailable.
The local UI runner suggests disabling isolation, but that suggestion conflicts with this task's isolation requirements.
Module import reduction within existing isolation remains a valid investigation boundary.

## Ranked proposed boundaries

Expected benefits remain hypotheses until controlled comparisons verify them.
The measured contribution gives an investigation ceiling, not a savings promise.

| Rank | Proposed task | Measured contribution | Confidence and expected benefit | Risk and boundary |
| --- | --- | --- | --- | --- |
| 1 | Profile daemon execution and host-maintenance fixtures | CI execution: 318 s macOS, 141 s Linux | High contribution confidence; savings unknown | Medium reliability risk; retain assertions, private fixtures, cancellation, and cleanup |
| 2 | Attribute test compilation and linking by binary | About 152 s macOS, 72 s Linux | High stage confidence; link share unknown | Low investigation risk; preserve daemon binary link and integration organization |
| 3 | Reduce desktop test import and environment setup costs | 84.67 s CI; 31% import and 21% environment tracking | Medium attribution confidence; savings unknown | Medium risk; retain isolation and four-worker cap until separately measured |
| 4 | Diagnose zero sccache hits across repeated same-revision runs | 18 macOS and 12 Linux misses | High observation confidence; benefit depends on reusable requests | Low investigation risk; preserve cache permissions and avoid secret-bearing logs |
| 5 | Measure temporary-checkout reuse and concurrent targets | No controlled workload measurement | Hypothesis only; savings unknown | Medium infrastructure risk; preserve outgoing-commit isolation and every required check |
| 6 | Profile unused-public audit | 18 s macOS, 13 s Linux | High stage confidence; savings unknown | Low scope risk; preserve every finding and exit status |

The strongest counterargument is that compilation and process startup may dominate fixtures already using correct synchronization.
Cache restoration already works, and sccache cannot cache every crate type or final link.
Dashboard tests are too small here to justify an early optimization task.
Reducing mandatory assertions, suppressing slow tests, disabling isolation, or adding retries is outside every proposed boundary.

## Live dependency audit failure

Both follow-up `cargo audit` invocations failed on the unchanged baseline lockfile.
The command reported `url 2.5.8`, advisory `RUSTSEC-2026-0592`, and a suggested upgrade to `>=2.5.9`.
The follow-up evidence records the failed command durations and exit codes.
The earlier successful CI run does not establish that today's live advisory check passes.
No dependency fix, advisory exception, or check suppression belongs in this report-only task.
The orchestrator received this finding separately.

## Acceptance gaps

The report does not claim that all measurement criteria pass.
Criterion 1 now has paired local artifact conditions; baseline CI still has one observation.
Criterion 3 still lacks controlled cold/warm CI repetitions and independent CI linker attribution.
Criterion 4 still lacks isolated equal-workload agent runs and resource-contention attribution.
The original confounded workload numbers supply no comparative evidence.
Existing adjacent CI data supplies context, not same-revision repetitions.
Infrastructure choices and an isolated host window still require orchestrator coordination and user agreement.

## Unavailable evidence and decisions

The following measurements remain unavailable:

- Guaranteed idle-host macOS repetitions; sequential shared-host measurements do not provide that guarantee.
- Repeated Linux CI runs at this revision, including controlled cold and warm cache conditions.
- Controlled complete agent workloads with process, CPU, memory, and I/O sampling.
- Main-push temporary checkout overhead, fingerprint invalidation, and repeated npm installation distributions.
- Release, signing, installer, and Tauri bundle timings.
- Per-test subprocess counts and database migration, copy, startup, and cleanup duration distributions.

Read-only GitHub access supplied existing CI evidence; this report did not dispatch new workflows or change cache contents.
The orchestrator must arrange controlled CI reruns and approve cache conditions before those measurements proceed.
The orchestrator must also select representative concurrent agent tasks and an isolated measurement window.
Do not substitute local Linux containers for hosted CI measurements without explicit agreement.

Recommendation: approve the first three investigation boundaries, then select implementation tickets after measured attribution.
Confidence is high for this baseline's stage ordering and low for predicted savings or cross-environment causation.
Repeated CI results, linker attribution, or contention traces could change the ranking.
This report supports scope agreement; it does not establish complete performance acceptance or authorize optimizations.

## Reproduction

1. Check out the baseline in a dedicated task worktree.
2. Record `uname -a`, `sw_vers`, CPU count, memory, tool versions, free space, and cache variables.
3. Record whether `target` and both `node_modules` directories exist.
4. Run the exact local commands from the evidence with a monotonic timer and bounded process-group timeout.
5. Retain artifacts and repeat the warm commands twice.
6. Retrieve CI steps with `gh run view 44987645207 --json jobs`.
7. Retrieve CI output with `gh run view 44987645207 --log`.
8. Subtract each step's `startedAt` from `completedAt`.
9. Separate nextest compilation from the runner's execution summary.
10. Obtain orchestrator approval for additional CI runs and controlled cache resets.

No executable benchmark runner or new tooling forms part of this report.

## Report checks

Run `git diff --check` for whitespace.
Check each relative Markdown target in this directory and its index.
No executable source changed; no additional runtime suite is required for this report.
