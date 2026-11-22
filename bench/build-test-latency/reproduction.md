# Follow-up measurement method

The follow-up measures the unchanged source baseline from report revision `cb3a7efdcad2ca10d9a7ccf00b6d12acb0629556`.
No Rust, UI, dependency, or workflow source changed after baseline `0561a6ab4a5864186724fab5bcf783030359298b`.
Each row in the follow-up evidence names its command and target.

## Artifact conditions

Each cold Rust row starts with a new empty target beneath `target/latency-v2`.
The corresponding warm row immediately repeats that command against the same target.
Two independent target pairs measure each operation.
The binary-build and test-build experiments use different targets.
Clippy and Tauri also use separate empty targets for each pair.
Global Cargo downloads remain available throughout; no OS cache flush occurs.
A cold run therefore means cold build artifacts, not cold disk or network caches.

Doctests use the first binary-build target after its cold and warm builds.
They measure first doctest invocation and its repeat, not a clean dependency build.
Source-only checks have no compilation cache to clear.
Their first and repeated invocations preserve download caches and advisory data.
The report must not call their first invocation a cold compiler build.

Frontend snapshots copy `ui/` beneath `target/latency-v2/frontend-corrected-1` and `frontend-corrected-2`.
Exclude `node_modules`, `dist`, `target`, and `*.tsbuildinfo` during the copy.
Retain `src-tauri/tauri.conf.json`, which desktop tests import.
Use snapshot names `frontend-corrected-1` and `frontend-corrected-2`.
Create `target/latency-v2/crates` as a symlink to the worktree's existing `crates` directory.
The dashboard imports its shipped dashboard JSON through that relative repository path.
The snapshot checks only read that source file.
Each snapshot retains source files, manifests, lockfiles, scripts, and dashboard patches.
Run the listed commands in their recorded snapshot directory.
The first `npm ci` starts without installed dependencies and retains the existing npm download cache.
Warm checks retain installed dependencies and TypeScript state.
After all checks and bundle builds, repeat `npm ci` in each snapshot package.
These repeated installation timings include removal and recreation of existing snapshot dependencies.
No original installation or global cache is removed.

The operator launches no other check during the sequential follow-up runs.
The machine still runs user applications and system services.
Thus, this follow-up is a sequential shared-host sample, not a guaranteed idle-host experiment.
No local-to-CI causal comparison uses it.

## Linker measurement

Use the same wrapper path for every measured Rust command.
Create the following temporary executable outside the worktree:

```sh
#!/bin/sh
exec /usr/bin/time -p -o "$LATENCY_LINK_LOG/$$.time" /usr/bin/cc "$@"
```

Set `CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER` to that executable.
Set `LATENCY_LINK_LOG` to a new empty log directory for each command.
Set `CARGO_TARGET_DIR` to the target named in the evidence.
The wrapper invokes the standard compiler driver without changing its arguments.
It measures linker-driver elapsed, user CPU, and system CPU time.
Its process startup adds measurement overhead.

The linker sum includes dependency build executables, procedural macros, libraries, and final binaries.
The maximum identifies the longest individual driver call.
Parallel linker calls overlap, so never subtract their sum from command wall time.
Cargo wall time minus linker sums does not yield compilation wall time.
Compilation, scheduling, and linking can overlap.
`cargo build --timings` also writes the unit timeline under each measured target's `cargo-timings` directory.
The separate linker data supplies attribution that Cargo's combined unit durations cannot provide alone.

## Bounded command timing

Use Python's standard library to run each command in the foreground.
Record monotonic elapsed time and `resource.getrusage(resource.RUSAGE_CHILDREN)` before and after each command.
The differences supply cumulative child user CPU and system CPU seconds.
Do not interpret CPU seconds as elapsed time.

Use `subprocess.Popen` with `start_new_session=True`, and direct stdout and stderr to a separate `/tmp` log.
Call `wait(timeout=600)` for each command.
On timeout, send SIGTERM to its process group and reap the command.
Record exit code 124 for a timeout.
Retain nonzero exits as failed observations; do not silently exclude them from the evidence.

## Repetition and scope

Each cold/warm Rust condition has two observations from separate targets.
Each frontend package condition has two observations from separate snapshots.
Each source-only check has two sequential observations.
The first and repeated doctest invocations have different artifact conditions; do not pool their durations.
Workspace execution summaries separate runner time from command totals and residual compilation.
Report both observations; report their range and repetition count.
Report min, median, and max only for groups with at least three comparable observations.
Do not combine different revisions, cache conditions, or commands to manufacture a larger sample.

No new hosted workflow run or infrastructure configuration is authorized here.
The baseline CI cells remain single observations.
The adjacent release run supplies context, with its changed revision explicitly identified.

## Corrected frontend setup

The first snapshot attempt omitted Tauri configuration and the dashboard's repository-relative JSON fixture.
Desktop tests failed with `ENOENT` for `src-tauri/tauri.conf.json`.
Dashboard types and tests failed with `TS2307` or a missing-module error for the shipped dashboard JSON.
The combined frontend build also failed because of that missing JSON fixture.
The operator stopped the measurement driver and its active subprocess group.
Those observations do not measure the complete check chain and are excluded from timing comparisons.
The corrected snapshots retain the configuration and expose the existing repository fixture through the read-only source path.
No production source or test assertion changed.
