"""The quiet watchdog's own thresholds (`crates/ariadne-daemon/src/scheduler/mod.rs`), copied
as plain constants so the baseline policy can be graded without linking the daemon. A change to
the Rust constants is a change here too — `tests/test_baseline_policy.py` pins the values this
file holds against the ones quoted in `specs/009-scheduler-attention-and-watchdogs.md` rule 9,
so a drift between the two is caught as a failing test, not a silent mismatch.
"""
from __future__ import annotations

SPAWN_RETRY_BUDGET = 3
QUIET_NUDGE_SECS = 180
QUIET_FLAG_SECS = 600
QUIET_RELAUNCH_SECS = 1_800

assert QUIET_NUDGE_SECS < QUIET_FLAG_SECS < QUIET_RELAUNCH_SECS
assert QUIET_FLAG_SECS >= 600
