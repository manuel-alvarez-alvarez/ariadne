# Daemon tests: fixed waits removed

This page records the time of three daemon tests before and after the change.
The tests are in `crates/ariadne-daemon/tests/it`, outside the forge modules.
The figures come from the macOS development machine, not from host malvarez.
Do not pool them with [the Linux figures](linux-host.md).

## Change

- The probe time of herdr and tmux calls comes from the timeouts of the running daemon.
  Before, `place.rs` and `acp.rs` read `Timeouts::default().probe` at run time.
- The default values of `Timeouts` did not change.
- Two doctor and place tests use a probe time of `RUNS_OUT` (500 ms).
- `host_reconnect::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after` uses a retry base of 250 ms, not 1 s.

## Method

- Before: a worktree at the parent commit, with its own target directory.
- After: the task branch.
- Command: `cargo nextest run -p ariadne-daemon -E '<test>' --status-level pass`, three runs for each side, one run at a time.
- Stress: `--stress-count 20` of the three tests after the change.

## Result

Times are wall seconds for one test, as the three runs in order.

| Test | Before | After |
| --- | --- | --- |
| `doctor::doctor_reports_a_missing_forced_place_and_its_reason` | 5.824 / 5.585 / 5.641 | 1.247 / 1.111 / 1.089 |
| `place::a_herdr_server_that_does_not_answer_is_not_found` | 5.028 / 5.017 / 5.017 | 0.524 / 0.517 / 0.519 |
| `host_reconnect::a_down_forward_starts_again_at_once_and_waits_the_first_backoff_after` | 4.453 / 4.262 / 4.311 | 1.455 / 1.281 / 1.283 |

The 20 stress iterations of the three tests all passed. No test failed.
The modules `place`, `doctor`, `host_reconnect`, `host_health`, `tunnel`, `webhooks`, `plan_finalize`, `ai_permissions`, `workflow_steps` and `acp_runtime` passed whole: 204 tests.

## Other sleeps

Each `sleep` under `tests/it`, outside the forge modules, was examined.
No test waited on a `sleep` that a daemon event could replace.
Each one is a `QUIET` listen, a 10 ms poll inside `eventually` or `quiet`, or a stand-in script.
Each retained `sleep` now has a comment that states its purpose: absence proof, poll pace or stand-in behavior.
