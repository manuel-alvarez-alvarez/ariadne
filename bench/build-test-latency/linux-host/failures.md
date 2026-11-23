# Host malvarez: failed tests

Source: `226a47eb63c8ab763167b3fbedc89e52a0e66ab3`, host malvarez.
Each run used `cargo nextest run --workspace --profile ci --status-level fail --final-status-level fail`.
All other runs of the same revision passed these tests, so each failure is intermittent.
No flaky marker, retry or skip was added.

| Test | Failed runs | Passed runs | Assertion |
| --- | --- | --- | --- |
| `ariadne-daemon::it relay::stop_kills_the_full_process_group` | series a cold 2, series a warm 1, series b cold 1 | 9 of 12 | `relay.rs:248` expects EOF (`Ok(0)`) and gets `ECONNRESET` (os error 104) |
| `ariadne-daemon::it ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console` | series b warm 1 | 11 of 12 | `ai_permissions_decisions.rs:1160`: the 31 KB log snapshot holds no `WARN` line |
| `ariadne-daemon::it ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval` | series b warm 1 | 11 of 12 | `ai_permissions_decisions.rs:867`: the model stub got 3 requests, not 2 |

Series a lost its JUnit files (see the report), but its logs keep the failure output below.
Lines longer than 400 characters are cut, and the cut is marked.

### Series a, nextest-cold-2.log

```text
        FAIL [   0.228s] (2153/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
  stdout ───
    running 1 test
    test relay::stop_kills_the_full_process_group ... FAILED
    failures:
    failures:
        relay::stop_kills_the_full_process_group
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1153 filtered out; finished in 0.20s
  stderr ───
    thread 'relay::stop_kills_the_full_process_group' (3064373) panicked at crates/ariadne-daemon/tests/it/relay.rs:248:61:
    called `Result::unwrap()` on an `Err` value: Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        FAIL [   0.228s] (2153/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
error: test run failed
```

### Series a, nextest-warm-1.log

```text
        FAIL [   0.195s] (2154/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
  stdout ───
    running 1 test
    test relay::stop_kills_the_full_process_group ... FAILED
    failures:
    failures:
        relay::stop_kills_the_full_process_group
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1153 filtered out; finished in 0.15s
  stderr ───
    thread 'relay::stop_kills_the_full_process_group' (2998304) panicked at crates/ariadne-daemon/tests/it/relay.rs:248:61:
    called `Result::unwrap()` on an `Err` value: Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        FAIL [   0.195s] (2154/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
error: test run failed
```

### Series b, nextest-cold-1.log

```text
        FAIL [   0.202s] (2152/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
  stdout ───
    running 1 test
    test relay::stop_kills_the_full_process_group ... FAILED
    failures:
    failures:
        relay::stop_kills_the_full_process_group
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1153 filtered out; finished in 0.17s
  stderr ───
    thread 'relay::stop_kills_the_full_process_group' (3328090) panicked at crates/ariadne-daemon/tests/it/relay.rs:248:61:
    called `Result::unwrap()` on an `Err` value: Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        FAIL [   0.202s] (2152/2651) ariadne-daemon::it relay::stop_kills_the_full_process_group
error: test run failed
```

### Series b, nextest-warm-1.log

```text
        FAIL [   1.120s] (1487/2651) ariadne-daemon::it ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console
  stdout ───
    running 1 test
    test ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console ... FAILED
    failures:
    failures:
        ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1153 filtered out; finished in 1.09s
  stderr ───
    thread 'ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console' (3348991) panicked at crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs:1160:5:
    LogSnapshotResponse { lines: [LogLineDto { ts: "2026-11-23T22:00:06.588Z", level: "DEBUG", target: "opentelemetry_sdk", message: " name=MeterProvider.ExistingMeterReturned meter_name=ariadned", service: "ariadned" }, LogLineDto { ts: "2026-11-23T22:00:06.588Z", level: "DEBUG", target: "opentelemetry_sdk", message: " name=Metrics.InstrumentCreated instrument_name=ariadne.task.status_time cardin …[truncated]
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        FAIL [   1.476s] (1512/2651) ariadne-daemon::it ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval
  stdout ───
    running 1 test
    test ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval ... FAILED
    failures:
    failures:
        ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1153 filtered out; finished in 1.44s
  stderr ───
    thread 'ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval' (3349388) panicked at crates/ariadne-daemon/tests/it/ai_permissions_decisions.rs:867:5:
    assertion `left == right` failed: the model decides first each time
      left: 3
     right: 2
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        FAIL [   1.120s] (1487/2651) ariadne-daemon::it ai_permissions_decisions::a_stopped_model_warns_and_waits_for_the_console
        FAIL [   1.476s] (1512/2651) ariadne-daemon::it ai_permissions_decisions::an_uncertain_allow_falls_to_console_and_then_to_the_learned_approval
error: test run failed
```
