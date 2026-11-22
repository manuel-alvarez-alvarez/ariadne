# Local measurement evidence

Baseline: `0561a6ab4a5864186724fab5bcf783030359298b`.

Times measure complete commands with `time.monotonic()`; each command had a 240-second limit.

| Run | Directory | Command | Seconds | Exit |
| --- | --- | --- | ---: | ---: |
| core-cold | . | `cargo nextest run -p ariadne-core --status-level fail --final-status-level fail` | 5.278 | 0 |
| core-warm-1 | . | `cargo nextest run -p ariadne-core --status-level fail --final-status-level fail` | 0.34 | 0 |
| core-warm-2 | . | `cargo nextest run -p ariadne-core --status-level fail --final-status-level fail` | 0.311 | 0 |
| ui-install | ui | `npm ci` | 2.417 | 0 |
| ui-test-1 | ui | `npm test -- --reporter=dot` | 34.901 | 0 |
| ui-test-2 | ui | `npm test -- --reporter=dot` | 39.242 | 0 |
| ui-types-1 | ui | `npm run typecheck` | 4.183 | 0 |
| ui-types-2 | ui | `npm run typecheck` | 4.005 | 0 |
| daemon-build | . | `cargo nextest run -p ariadne-daemon --no-run` | 47.221 | 0 |
| daemon-test-1 | . | `cargo nextest run -p ariadne-daemon --status-level fail --final-status-level fail` | 54.408 | 0 |
| daemon-test-2 | . | `cargo nextest run -p ariadne-daemon --status-level fail --final-status-level fail` | 55.185 | 0 |

## Historical concurrent sample; excluded from comparisons

Two core commands shared this worktree target during daemon compilation and UI measurement.
This sample cannot measure equal-agent contention.
The UI timings above also overlap compilation and cannot support an idle-host comparison.
The revised report excludes these numbers from workload conclusions.
Each core command used the table command; both passed.
The pair used a 120-second subprocess timeout.
Combined elapsed time: 36.855 seconds.

```json
{
  "command": [
    "cargo",
    "nextest",
    "run",
    "-p",
    "ariadne-core",
    "--status-level",
    "fail",
    "--final-status-level",
    "fail"
  ],
  "workers": [
    {
      "worker": 0,
      "seconds": 36.842,
      "exit": 0
    },
    {
      "worker": 1,
      "seconds": 36.845,
      "exit": 0
    }
  ],
  "wall_seconds": 36.855,
  "note": "Overlaps daemon compilation and UI measurement; shared worktree target"
}
```

## Selected command output

The excerpts retain build totals, runner summaries, and observed Cargo locks.

### core-cold

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.49s
    Starting 49 tests across 1 binary
     Summary [   0.043s] 49 tests run: 49 passed, 0 skipped
```

### core-warm-1

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
    Starting 49 tests across 1 binary
     Summary [   0.031s] 49 tests run: 49 passed, 0 skipped
```

### core-warm-2

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
    Starting 49 tests across 1 binary
     Summary [   0.031s] 49 tests run: 49 passed, 0 skipped
```

### ui-install

```text
added 557 packages, and audited 564 packages in 2s
```

### ui-test-1

```text
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  33.86s (tests 37%, import 34%, environment 20%, setup 4%, transform 4%)
     Import  715 modules were evaluated 33602 times · 40.72s total, 34% of tracked time
```

### ui-test-2

```text
 Test Files  142 passed (142)
      Tests  1497 passed (1497)
   Duration  38.90s (tests 40%, import 34%, environment 19%, setup 4%, transform 3%)
     Import  715 modules were evaluated 33625 times · 47.04s total, 34% of tracked time
```

### ui-types-1

```text
```

### ui-types-2

```text
```

### daemon-build

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 45.88s
```

### daemon-test-1

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
    Starting 1483 tests across 2 binaries
     Summary [  53.981s] 1483 tests run: 1483 passed, 0 skipped
```

### daemon-test-2

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
    Starting 1483 tests across 2 binaries
     Summary [  54.763s] 1483 tests run: 1483 passed (1 slow), 0 skipped
```

### concurrent-0

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `test` profile [unoptimized + debuginfo] target(s) in 36.45s
    Starting 49 tests across 1 binary
     Summary [   0.052s] 49 tests run: 49 passed, 0 skipped
```

### concurrent-1

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `test` profile [unoptimized + debuginfo] target(s) in 36.39s
    Starting 49 tests across 1 binary
     Summary [   0.046s] 49 tests run: 49 passed, 0 skipped
```
