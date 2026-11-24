# Shell fake forge on macOS

Question: do the daemon tests that use the fake `gh` pass on macOS with the POSIX shell fake, and how much faster are they?

Result: pass. All 381 tests passed in three runs. One test needed its own retry in one run; the base had the same test fail and pass in the same round.

## Method

- Tested commit: `3050fabd5557d78d6a6069d6e760480663c51a21`, branch `cut-the-process-cost-of-the-fake-forge-cnkwwm` of origin. `git ls-remote origin` named it at that commit.
- Merge base of that commit and `origin/main` (`ce7bf6e4`): `de90ce670e5b62a9f3f769aaa013464384b821c4`.
- Each commit ran in its own detached worktree under `/tmp/fakeforge-proof`, with its own `CARGO_TARGET_DIR` outside the worktree. Both worktrees were removed after the runs. The task branch was not changed.
- Rounds alternated: after 1, base 1, after 2, base 2, after 3, base 3.
- Before each timed run, `cargo nextest run ... --no-run` built the tests. One build step (after 1 and base 1) still compiled one crate; that time is not in the timed run. Later builds compiled nothing.
- No code, test, hook, flaky marker, retry or skip was changed. The `forge_rate_limit::flaky::` marker and its retries are in the repository at both commits.

Per round, with `F` the filter of the task:

```sh
CARGO_TARGET_DIR=/tmp/fakeforge-proof/target-<after|base> \
  cargo nextest run --workspace --profile ci -E "$F" --no-run
CARGO_TARGET_DIR=/tmp/fakeforge-proof/target-<after|base> \
  cargo nextest run --workspace --profile ci -E "$F" --no-fail-fast
```

## Environment

- macOS 27.0.1, build 26A434, Apple M4 Max, 16 logical CPUs, 64 GiB RAM.
- rustc 1.99.0 (Homebrew), cargo-nextest 0.9.146.
- The fake `gh` at the tested commit starts with `#!/bin/sh` (`crates/ariadne-daemon/tests/it/common/forge.rs`, `STUB`), so `/bin/sh` ran it. `/bin/sh` is the macOS system shell. The base fake is `python3` behind an `/bin/sh` launcher.
- Other work ran on this Mac during the runs (the real daemon and other agent sessions). Do not compare these times with other hosts.

## Results

Wall time is the nextest `Summary` time.

| Run | Base `de90ce67` | After `3050fabd` |
| --- | --- | --- |
| 1 | 31.080 s, 380/380 pass | 30.528 s, 381/381 pass |
| 2 | 32.735 s, 380/380 pass (1 flaky) | 30.612 s, 381/381 pass (1 flaky) |
| 3 | 31.250 s, 380/380 pass | 29.990 s, 381/381 pass |
| Mean | 31.688 s | 30.377 s |

The mean gain is 1.31 s (4.1 %). The ranges do not overlap: the slowest after run (30.612 s) is faster than the fastest base run (31.080 s), by 0.47 s. Three runs per side are few, and other work ran on this Mac.
The after commit runs one more test than the base (381 against 380).

## Failures

No run had a final failure. In round 2, both commits had the same test fail on try 1 and pass on try 2:

- `ariadne-daemon::it forge_rate_limit::flaky::a_failed_fetch_in_timer_mode_backs_off_and_a_wake_still_fetches_at_once`
  - After, try 1 (2.947 s): `forge_rate_limit.rs:370:14: the capped timer fetch must complete: Elapsed(())`.
  - Base, try 1 (3.324 s): `forge_rate_limit.rs:379:14: the first success must reset the timer backoff: Elapsed(())`.
  - Both passed on try 2. The test is intermittent at the merge base too.

The two tunnel tests and the keeper test that failed once on Linux did not fail in these six runs.

## Conclusion

The shell fake passes the filter on macOS in three of three runs. On this Mac it saves about 1.3 s of 31.7 s. The gain is small, but all three after runs were faster than all three base runs.
