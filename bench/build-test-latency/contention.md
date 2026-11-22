# macOS check contention follow-up

This report measures identical daemon checks in two worktrees on one macOS host.
It measures check commands, rather than complete agent sessions.
The reader must decide whether concurrent checks need further investigation.

Concurrent separate-target daemon tests doubled individual command latency in this sample.
One concurrent command failed a relay assertion; all sequential commands passed.

## Warm comparisons

Each condition has two pair observations and four command observations.
Ranges and medians include the failed concurrent separate-target test command.
Pair totals add A and B phase durations for sequential observations.
Concurrent pair totals use the measured common phase duration.

| Workload | Targets | Schedule | Command range, s | Command median, s | Pair range, s | Failed commands |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| test | separate | seq | 53.674–56.560 | 54.344 | 107.374–111.574 | 0 |
| test | separate | con | 114.110–120.160 | 117.049 | 114.232–120.167 | 1 |
| test | shared | seq | 83.020–87.061 | 83.713 | 166.937–170.593 | 0 |
| test | shared | con | 53.777–113.396 | 83.210 | 111.848–113.402 | 0 |
| clippy | separate | seq | 0.236–0.353 | 0.270 | 0.488–0.665 | 0 |
| clippy | separate | con | 0.360–0.471 | 0.415 | 0.366–0.479 | 0 |
| clippy | shared | seq | 0.240–16.173 | 15.277 | 15.539–31.453 | 0 |
| clippy | shared | con | 0.348–30.876 | 15.576 | 15.448–30.883 | 0 |

Sequential tests passed all 1,483 tests per command.
Concurrent tests passed all tests except one command, which passed 1,482 and failed one.
All clippy commands passed.
The separate-target concurrent median test command took 2.15 times the sequential median.

Initial test builds in three empty targets took 45.855–46.438 seconds.
The following clippy preparations took 23.845–24.236 seconds.
B/shared preparation took 31.330 seconds for tests and 16.086 seconds for clippy.
Those preparation observations remain separate from warm comparisons.

The second shared clippy pair started with different artifact reuse from the first pair.
Its concurrent commands rebuilt sequentially, while its subsequent sequential A command reused artifacts.
Do not treat the pooled shared clippy medians as a controlled concurrency effect.

## Revision and environment

The operator resolved local `main` once, before preparation.
The pinned revision is `e274fe4b1046a146c10bf1591a6da50114c339a3`.
Both dedicated detached worktrees use that revision.
The original audit baseline remains `0561a6ab4a5864186724fab5bcf783030359298b`.
These measurements do not increase its repetition counts.

The host uses macOS 27.0.1, build 26A434, on an Apple M4 Max.
`sysctl` reports model Mac16,6, 16 logical CPUs, and 68,719,476,736 bytes of memory.
Initial available disk space was 208 GiB.
Rust and Cargo use Homebrew 1.99.0; LLVM is 23.1.2.
Clippy reports 0.1.99; nextest reports 0.9.146; Python reports 3.15.0.
No Cargo, Rust, or sccache environment overrides were present initially.

The reserved window ends at 2026-11-22 21:23 UTC, or 22:23 Europe/Madrid.
The runner uses an earlier cutoff of 21:22:30 UTC.
Preparation started at 20:24:37 UTC.
Existing applications and services remained active.
An initial `ps` sample showed Chrome processes at 64.9% and 20.4% CPU, and WindowServer at 24.5%.
It also showed WebKit at 13.7%, Codex at 13.3%, and Ariadne daemons at 6.7% and 6.1%.
These instantaneous values establish background activity, rather than reserved CPU capacity.
Initial `memory_pressure -Q` reported 63% system-wide memory free.
That percentage differs from unused physical pages because macOS can reclaim memory.

## Commands and controls

All measurement files reside beneath `/tmp/f852fr-contention`.
Worktrees `a` and `b` contain identical sources.
Targets `shared`, `separate-a`, and `separate-b` started absent.
The shared condition gives both worktrees the same target directory.
The separate condition assigns one target to each worktree.
Existing Cargo downloads and unrelated build caches remain intact.
No OS filesystem cache was flushed.

Each command runs from its named worktree with the specified `CARGO_TARGET_DIR`.
The exact test command is:

```sh
cargo nextest run --locked -p ariadne-daemon --profile ci --status-level fail --final-status-level fail
```

The exact clippy command is:

```sh
cargo clippy --locked -p ariadne-daemon --all-targets -- -D warnings
```

Preparation adds `--no-run` immediately after `cargo nextest run`.
Preparation orders A/shared, B/shared, A/separate-a, then B/separate-b.
Each preparation pair runs test compilation before clippy.
Only the first use of each empty target has cold build artifacts.
The B/shared preparation reuses dependencies but rebuilds workspace artifacts for its source path.
Warm comparisons retain all preparation artifacts.
A shared target can rebuild workspace artifacts when the active worktree changes.
Thus, warm means retained artifacts, rather than guaranteed compiler cache hits.

Each subprocess uses `/usr/bin/time -l` and a new process group.
Each phase has a 570-second limit and checks the UTC cutoff every 50 milliseconds.
A phase contains either one command or two concurrent commands.
Sequential comparisons run A, then B, in separate phases.
Concurrent comparisons launch A and B together.
Shared-target JUnit files can overwrite each other; per-command logs supply the reported results.
The runner records monotonic command durations and concurrent pair duration.
The runner terminates and reaps its subprocess groups after each phase.
It sends SIGKILL only if a task-owned group ignores SIGTERM for three seconds.
No unrelated process receives a signal.

Monitors sample `top`, `iostat`, and `vm_stat` every five seconds.
Later phases cap each monitor at 120 samples.
Preparation monitors use the phase cleanup boundary.
The CPU summary excludes the first `top` sample.
The disk summary excludes the first `iostat` sample, which represents activity since boot.
Disk values describe host activity, rather than process-attributed I/O.
CPU values describe total host use, including background applications and monitors.
Short clippy runs can end before an interval sample exists.
The evidence marks those samples unavailable.

## Completion and resource observations

All workloads and phase monitors ended by 20:48:26 UTC.
Both dedicated measurement worktrees were removed at 20:48:47 UTC.
The final process snapshot showed no remaining executable from the measured targets.
The runner reaped all task-owned command and monitor groups.
The task branch retains only report changes.
New targets and external evidence logs remain available; existing caches remain intact.
All eight planned workload, target, and schedule conditions have two pair repetitions.
No comparison was omitted because of the deadline or resource limits.

The initial memory query reported 63% free; intermediate and final queries reported 65%.
Interval VM observations show no new swap-in or swap-out activity.
Existing compressed memory remained present throughout.
Low unused-page counts alone do not establish memory pressure on macOS.
The resource evidence lists CPU medians and maxima, disk throughput, unused-page minima, and compressor-page maxima per phase.
Subsecond clippy phases lack five-second CPU and disk interval samples.
No memory pressure grade or process-attributed disk trace was collected.

## Scope and limitations

These runs cover the daemon crate, including its integration tests, and all daemon clippy targets.
They do not cover workspace tests, frontend checks, release builds, or complete agent sessions.
They measure unchanged sources and retained artifacts, rather than incremental edits during active development.
The commands retain the existing assertions, nextest profile, test parallelism, and configured flaky-test retries.
No test was skipped or newly marked flaky.

The host reservation reduced competing planned work, but did not stop user applications or system services.
No controlled thermal, power, frequency, or filesystem-cache condition was established.
Five-second resource samples can miss short peaks.
Monitor startup and `/usr/bin/time` add overhead, especially to subsecond clippy observations.
The two target layouts also have different compilation reuse when worktree paths change.
The report does not attribute their entire latency difference to Cargo locks.
Cargo lock messages identify waits, but do not measure each wait duration.
Nextest can wait for the global package cache even with separate targets.
A shared build-directory lock does not serialize the complete test execution phase by definition.
Observed scheduling can overlap compilation in one worktree with test execution in another.

The first concurrent separate-target run failed one relay test.
Its assertion timed out while waiting for the relay socket to disappear after session termination.
The source is `crates/ariadne-daemon/tests/it/acp_runtime.rs:1094`.
The assertion helper reports the timeout at `tests/it/common/mod.rs:2418` within that crate.
The same unchanged revision passed both first-round sequential runs.
This is evidence of an intermittent failure during concurrency, rather than proof of a specific root cause.
The failed observation remains in all timing summaries, with its exit status in the evidence.
No retry replaced that observation.

## Evidence sources

[Command and resource evidence](contention-evidence.md) records every preparation and comparison command result.
[Reproduction](contention-reproduction.md) preserves the driver and worktree setup.
The original audit remains available through [its index](README.md).

Environment observations use these commands:

```sh
git rev-parse main
git worktree list
sw_vers
sysctl -n hw.model hw.memsize hw.ncpu machdep.cpu.brand_string
df -h .
ps -axo pid,ppid,%cpu,%mem,comm
rustc -Vv
cargo --version
cargo nextest --version
cargo clippy --version
python3 --version
env | rg '^(CARGO|RUST|SCCACHE)'
memory_pressure -Q
```

No matching environment override was printed.
The initial process table was sorted by CPU before selecting the displayed rows.

## Interpretation and recommendation

Prefer sequential full daemon test checks when individual check latency and stable completion matter on this host.
Keep separate targets as the comparison condition for future worktree measurements.
They avoid the observed workspace rebuilds when commands switch source paths.
This report makes no configuration or optimization change.

Concurrent separate-target daemon tests took roughly twice the sequential command time in both repetitions.
The failed first-round command did not alone cause that result.
Its peer passed at almost the same elapsed time.
Both second-round concurrent commands passed while retaining the slowdown and slow-test warnings.
These observations support host contention during concurrent full daemon checks.
They do not identify one operating-system bottleneck.

The strongest counter-evidence is the shared-target concurrent comparison.
Its pair completed faster than the corresponding sequential pair.
A command that could reuse the latest shared artifacts finished early.
The other command waited or rebuilt before executing tests.
Therefore, concurrency does not universally increase pair completion time, and separate targets do not universally improve test throughput.
Warm clippy observations also include very little work when artifacts match the source path.
Their subsecond results cannot establish the cost of concurrent clippy compilation after real edits.

Confidence is moderate for the observed daemon latency effect on this host and revision.
Confidence is low for extrapolation to other hosts, revisions, or complete agent sessions.
A larger randomized sample, isolated background activity, or materially different workload could change the recommendation.
Future comparisons should report individual command latency and pair completion time separately.
