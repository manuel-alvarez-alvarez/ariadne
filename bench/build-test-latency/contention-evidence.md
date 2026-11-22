# Contention measurement evidence

These observations use the revision and controls in [the report](contention.md).
All times use seconds.
Each row represents one command; concurrent rows share a phase label.
The phase duration measures the complete pair when two commands run together.

## Commands and resource observations

| Phase | UTC start | Seat | Command elapsed | Phase elapsed | Exit | Child user CPU | Child system CPU | Maximum RSS, MiB | Lock messages |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| prep-a-shared-test | 20:24:37 | a | 45.863 | 45.871 | 0 | 84.36 | 13.27 | 2254.1 | 0 |
| prep-a-shared-clippy | 20:25:23 | a | 23.845 | 23.856 | 0 | 28.57 | 3.20 | 1478.8 | 0 |
| prep-b-shared-test | 20:25:47 | b | 31.330 | 31.336 | 0 | 55.67 | 3.71 | 2254.4 | 0 |
| prep-b-shared-clippy | 20:26:18 | b | 16.086 | 16.103 | 0 | 23.17 | 0.95 | 1479.6 | 0 |
| prep-a-separate-a-test | 20:26:34 | a | 45.855 | 45.862 | 0 | 84.58 | 13.52 | 2256.0 | 0 |
| prep-a-separate-a-clippy | 20:27:20 | a | 24.107 | 24.125 | 0 | 28.80 | 3.37 | 1479.9 | 0 |
| prep-b-separate-b-test | 20:27:44 | b | 46.438 | 46.445 | 0 | 85.61 | 14.05 | 2253.6 | 0 |
| prep-b-separate-b-clippy | 20:28:30 | b | 24.236 | 24.252 | 0 | 28.80 | 3.37 | 1478.2 | 0 |
| test:separate:seq:1-a | 20:29:02 | a | 55.002 | 55.008 | 0 | 226.01 | 252.87 | 133.7 | 0 |
| test:separate:seq:1-b | 20:29:57 | b | 56.560 | 56.566 | 0 | 226.36 | 253.36 | 134.9 | 0 |
| test:separate:con:1 | 20:30:58 | b | 119.873 | 120.167 | 0 | 271.10 | 330.10 | 135.1 | 5 |
| test:separate:con:1 | 20:30:58 | a | 120.160 | 120.167 | 100 | 271.60 | 327.28 | 134.2 | 5 |
| test:shared:seq:1-a | 20:33:03 | a | 87.061 | 87.067 | 0 | 280.37 | 266.12 | 2254.1 | 0 |
| test:shared:seq:1-b | 20:34:30 | b | 83.520 | 83.526 | 0 | 266.81 | 259.36 | 2255.1 | 0 |
| test:shared:con:1 | 20:35:57 | b | 54.579 | 113.402 | 0 | 225.84 | 217.67 | 135.0 | 5 |
| test:shared:con:1 | 20:35:57 | a | 113.396 | 113.402 | 0 | 286.91 | 274.52 | 2224.0 | 6 |
| clippy:separate:seq:1-a | 20:38:05 | a | 0.353 | 0.359 | 0 | 0.17 | 0.12 | 135.2 | 0 |
| clippy:separate:seq:1-b | 20:38:05 | b | 0.300 | 0.306 | 0 | 0.17 | 0.11 | 135.2 | 0 |
| clippy:separate:con:1 | 20:38:05 | a | 0.361 | 0.366 | 0 | 0.12 | 0.09 | 134.0 | 3 |
| clippy:separate:con:1 | 20:38:05 | b | 0.360 | 0.366 | 0 | 0.12 | 0.09 | 134.0 | 3 |
| clippy:shared:seq:1-a | 20:38:06 | a | 16.173 | 16.179 | 0 | 22.28 | 0.91 | 1479.5 | 0 |
| clippy:shared:seq:1-b | 20:38:22 | b | 15.268 | 15.274 | 0 | 22.08 | 0.83 | 1478.9 | 0 |
| clippy:shared:con:1 | 20:38:37 | b | 0.348 | 15.448 | 0 | 0.12 | 0.10 | 135.1 | 3 |
| clippy:shared:con:1 | 20:38:37 | a | 15.442 | 15.448 | 0 | 22.08 | 0.83 | 1478.8 | 4 |
| test:shared:con:2 | 20:39:01 | a | 53.777 | 111.848 | 0 | 214.97 | 213.32 | 133.7 | 4 |
| test:shared:con:2 | 20:39:01 | b | 111.841 | 111.848 | 0 | 276.45 | 267.35 | 2199.5 | 6 |
| test:shared:seq:2-a | 20:40:52 | a | 83.905 | 83.911 | 0 | 265.86 | 263.46 | 2255.8 | 0 |
| test:shared:seq:2-b | 20:42:16 | b | 83.020 | 83.026 | 0 | 266.17 | 258.90 | 2253.2 | 0 |
| test:separate:con:2 | 20:43:44 | a | 114.110 | 114.232 | 0 | 230.70 | 321.33 | 135.7 | 5 |
| test:separate:con:2 | 20:43:44 | b | 114.225 | 114.232 | 0 | 230.43 | 320.52 | 134.2 | 5 |
| test:separate:seq:2-a | 20:45:39 | a | 53.674 | 53.680 | 0 | 215.68 | 256.58 | 133.7 | 0 |
| test:separate:seq:2-b | 20:46:32 | b | 53.687 | 53.694 | 0 | 214.73 | 258.62 | 134.9 | 0 |
| clippy:shared:con:2 | 20:47:39 | b | 15.710 | 30.883 | 0 | 22.26 | 0.88 | 1479.4 | 3 |
| clippy:shared:con:2 | 20:47:39 | a | 30.876 | 30.883 | 0 | 22.16 | 0.86 | 1479.5 | 4 |
| clippy:shared:seq:2-a | 20:48:10 | a | 0.240 | 0.247 | 0 | 0.12 | 0.10 | 135.4 | 0 |
| clippy:shared:seq:2-b | 20:48:10 | b | 15.286 | 15.292 | 0 | 22.10 | 0.84 | 1479.0 | 0 |
| clippy:separate:con:2 | 20:48:25 | a | 0.471 | 0.479 | 0 | 0.19 | 0.11 | 135.6 | 3 |
| clippy:separate:con:2 | 20:48:25 | b | 0.470 | 0.479 | 0 | 0.19 | 0.12 | 135.7 | 3 |
| clippy:separate:seq:2-a | 20:48:26 | a | 0.240 | 0.246 | 0 | 0.12 | 0.08 | 134.1 | 0 |
| clippy:separate:seq:2-b | 20:48:26 | b | 0.236 | 0.242 | 0 | 0.12 | 0.09 | 134.0 | 0 |

Child CPU values come from `/usr/bin/time -l`.
Maximum RSS describes the command resource statistic, rather than the sum of concurrent process memory.

## Host interval samples

CPU busy equals 100 minus idle; the first sample is excluded.
Disk throughput uses disk0 MB/s; the first sample is excluded.
VM swap counts exclude each cumulative row after a repeated header.
Compressed pages describe occupied compressed storage, rather than a memory pressure rating.
One page contains 16,384 bytes.
The monitors cannot attribute host activity to these commands.

| Phase | CPU samples | CPU busy median/max, % | Disk samples | Disk0 median/max, MB/s | Free pages min | Compressor pages max | Swap-in/out pages |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| prep-a-shared-test | 8 | 18.73/97.77 | 9 | 83.77/187.45 | 352153 | 1208933 | 0/0 |
| prep-a-shared-clippy | 4 | 17.90/98.23 | 4 | 36.37/50.54 | 316254 | 1208933 | 0/0 |
| prep-b-shared-test | 6 | 16.18/20.12 | 6 | 45.38/130.90 | 223837 | 1208933 | 0/0 |
| prep-b-shared-clippy | 3 | 14.20/15.57 | 3 | 24.01/48.87 | 214675 | 1208933 | 0/0 |
| prep-a-separate-a-test | 8 | 18.51/99.67 | 9 | 84.05/176.09 | 7767 | 1208933 | 0/0 |
| prep-a-separate-a-clippy | 4 | 18.92/99.28 | 4 | 38.59/45.95 | 52332 | 1208931 | 0/0 |
| prep-b-separate-b-test | 9 | 17.89/98.85 | 9 | 59.51/177.06 | 5494 | 1208931 | 0/0 |
| prep-b-separate-b-clippy | 4 | 20.51/99.10 | 4 | 42.47/46.55 | 6234 | 1208931 | 0/0 |
| test:separate:seq:1-a | 10 | 76.64/98.36 | 10 | 55.61/97.76 | 78559 | 1208929 | 0/0 |
| test:separate:seq:1-b | 10 | 63.79/100.00 | 11 | 55.37/99.17 | 56450 | 1208929 | 0/0 |
| test:separate:con:1 | 22 | 82.36/100.00 | 23 | 59.07/169.06 | 14199 | 1208914 | 0/0 |
| test:shared:seq:1-a | 16 | 46.76/99.90 | 17 | 55.98/135.75 | 4981 | 1208912 | 0/0 |
| test:shared:seq:1-b | 16 | 35.75/99.30 | 16 | 57.74/117.61 | 6086 | 1208912 | 0/0 |
| test:shared:con:1 | 22 | 70.56/99.75 | 22 | 70.68/137.88 | 10044 | 1208912 | 0/0 |
| clippy:separate:seq:1-a | 0 | n/a | 0 | n/a | 218693 | 1208910 | 0/0 |
| clippy:separate:seq:1-b | 0 | n/a | 0 | n/a | 205763 | 1208910 | 0/0 |
| clippy:separate:con:1 | 0 | n/a | 0 | n/a | 205951 | 1208910 | 0/0 |
| clippy:shared:seq:1-a | 3 | 12.27/13.60 | 3 | 40.32/53.27 | 102024 | 1208910 | 0/0 |
| clippy:shared:seq:1-b | 2 | 12.38/14.03 | 3 | 22.66/50.51 | 48945 | 1208910 | 0/0 |
| clippy:shared:con:1 | 2 | 12.32/13.55 | 3 | 22.73/48.29 | 47003 | 1208910 | 0/0 |
| test:shared:con:2 | 21 | 62.63/100.00 | 22 | 79.06/117.61 | 11421 | 1208910 | 0/0 |
| test:shared:seq:2-a | 16 | 37.94/99.61 | 16 | 58.84/116.97 | 7241 | 1208910 | 0/0 |
| test:shared:seq:2-b | 16 | 38.89/99.83 | 16 | 61.77/117.56 | 19382 | 1208910 | 0/0 |
| test:separate:con:2 | 21 | 70.17/100.00 | 22 | 63.88/155.41 | 66162 | 1208910 | 0/0 |
| test:separate:seq:2-a | 10 | 72.17/98.28 | 10 | 61.05/92.08 | 87872 | 1208908 | 0/0 |
| test:separate:seq:2-b | 10 | 71.24/99.39 | 10 | 59.51/91.65 | 87874 | 1208908 | 0/0 |
| clippy:shared:con:2 | 6 | 11.47/15.32 | 6 | 35.86/52.52 | 35534 | 1208908 | 0/0 |
| clippy:shared:seq:2-a | 0 | n/a | 0 | n/a | 162684 | 1208907 | 0/0 |
| clippy:shared:seq:2-b | 2 | 13.10/13.80 | 3 | 22.69/50.73 | 49455 | 1208907 | 0/0 |
| clippy:separate:con:2 | 0 | n/a | 0 | n/a | 144377 | 1208907 | 0/0 |
| clippy:separate:seq:2-a | 0 | n/a | 0 | n/a | 133470 | 1208907 | 0/0 |
| clippy:separate:seq:2-b | 0 | n/a | 0 | n/a | 133556 | 1208907 | 0/0 |

## Cargo and nextest excerpts

These excerpts retain build durations, runner summaries, and every observed Cargo lock message.
The logs include no passing-test listings.
Raw logs remain outside Git under `/tmp/f852fr-contention`.

### prep-a-shared-test / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 44.46s
```

### prep-a-shared-clippy / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.73s
```

### prep-b-shared-test / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 29.88s
```

### prep-b-shared-clippy / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.94s
```

### prep-a-separate-a-test / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 44.41s
```

### prep-a-separate-a-clippy / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.01s
```

### prep-b-separate-b-test / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 44.98s
```

### prep-b-separate-b-clippy / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.11s
```

### test:separate:seq:1-a / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Summary [  54.453s] 1483 tests run: 1483 passed, 0 skipped
```

### test:separate:seq:1-b / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Summary [  56.015s] 1483 tests run: 1483 passed, 0 skipped
```

### test:separate:con:1 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.23s
     Summary [ 119.197s] 1483 tests run: 1483 passed (16 slow), 0 skipped
```

### test:separate:con:1 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
        FAIL [  32.563s] ( 877/1483) ariadne-daemon::it acp_runtime::a_tmux_session_runs_acp_through_its_relay
    test acp_runtime::a_tmux_session_runs_acp_through_its_relay ... FAILED
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1099 filtered out; finished in 32.54s
    thread 'acp_runtime::a_tmux_session_runs_acp_through_its_relay' (224221816) panicked at crates/ariadne-daemon/tests/it/common/mod.rs:2418:9:
    timed out waiting for relay pane stopped
     Summary [ 119.584s] 1483 tests run: 1482 passed (19 slow), 1 failed, 0 skipped
        FAIL [  32.563s] ( 877/1483) ariadne-daemon::it acp_runtime::a_tmux_session_runs_acp_through_its_relay
error: test run failed
```

### test:shared:seq:1-a / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 30.53s
     Summary [  55.082s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:seq:1-b / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 28.29s
     Summary [  53.855s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:con:1 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Summary [  54.021s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:con:1 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `test` profile [unoptimized + debuginfo] target(s) in 57.69s
     Summary [  54.263s] 1483 tests run: 1483 passed, 0 skipped
```

### clippy:separate:seq:1-a / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s
```

### clippy:separate:seq:1-b / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
```

### clippy:separate:con:1 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s
```

### clippy:separate:con:1 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s
```

### clippy:shared:seq:1-a / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 16.10s
```

### clippy:shared:seq:1-b / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.18s
```

### clippy:shared:con:1 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.25s
```

### clippy:shared:con:1 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.33s
```

### test:shared:con:2 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Summary [  53.114s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:con:2 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `test` profile [unoptimized + debuginfo] target(s) in 56.95s
     Summary [  53.359s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:seq:2-a / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 28.33s
     Summary [  54.236s] 1483 tests run: 1483 passed, 0 skipped
```

### test:shared:seq:2-b / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 28.46s
     Summary [  53.212s] 1483 tests run: 1483 passed, 0 skipped
```

### test:separate:con:2 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.22s
     Summary [ 113.398s] 1483 tests run: 1483 passed (16 slow), 0 skipped
```

### test:separate:con:2 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Summary [ 113.534s] 1483 tests run: 1483 passed (13 slow), 0 skipped
```

### test:separate:seq:2-a / a

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Summary [  53.239s] 1483 tests run: 1483 passed, 0 skipped
```

### test:separate:seq:2-b / b

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Summary [  53.233s] 1483 tests run: 1483 passed, 0 skipped
```

### clippy:shared:con:2 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.62s
```

### clippy:shared:con:2 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on build directory
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 30.75s
```

### clippy:shared:seq:2-a / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.17s
```

### clippy:shared:seq:2-b / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.20s
```

### clippy:separate:con:2 / a

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
```

### clippy:separate:con:2 / b

```text
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Blocking waiting for file lock on package cache
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
```

### clippy:separate:seq:2-a / a

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```

### clippy:separate:seq:2-b / b

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s
```
