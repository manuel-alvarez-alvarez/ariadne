"""Pins the baseline port against the thresholds and guard rules `scheduler/quiet.rs` and spec
009 describe (rules 9, 11, 16-19, 31), so a drift in either the Rust constants or this port's
reading of them fails a test rather than surfacing only in the comparison report."""
import dataclasses

from bench_progress import policy_baseline
from bench_progress.schema import SessionState
from bench_progress.thresholds import QUIET_FLAG_SECS, QUIET_NUDGE_SECS, QUIET_RELAUNCH_SECS

# Spec 009 rule 9 quotes these three numbers by name; a change to either side must fail here.
assert QUIET_NUDGE_SECS == 180
assert QUIET_FLAG_SECS == 600
assert QUIET_RELAUNCH_SECS == 1800

BASE = SessionState(
    status="idle",
    seat="author",
    attention_reason=None,
    last_activity_at="2026-01-01T00:00:00Z",
    launched_at="2026-01-01T00:00:00Z",
    now="2026-01-01T00:00:00Z",
    task_status="in_progress",
    goal_status="active",
)


def _at(seconds_quiet, **overrides):
    now = f"2026-01-01T00:{seconds_quiet // 60:02d}:{seconds_quiet % 60:02d}Z"
    return dataclasses.replace(BASE, now=now, **overrides)


def test_under_the_nudge_threshold_is_left_alone():
    decision, _ = policy_baseline.decide(_at(QUIET_NUDGE_SECS - 1))
    assert decision == "continue"


def test_an_idle_session_past_the_nudge_threshold_is_nudged():
    decision, _ = policy_baseline.decide(_at(QUIET_NUDGE_SECS + 1, status="idle"))
    assert decision == "nudge"


def test_a_running_session_past_the_nudge_threshold_is_not_nudged():
    # Rule 11: only an idle agent is nudged - one mid-turn waits out the thresholds behind it.
    decision, _ = policy_baseline.decide(_at(QUIET_NUDGE_SECS + 1, status="running"))
    assert decision == "continue"


def test_past_the_flag_threshold_is_escalated_stalled():
    decision, _ = policy_baseline.decide(_at(QUIET_FLAG_SECS + 1))
    assert decision == "escalate_stalled"


def test_past_the_relaunch_threshold_is_also_escalated():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS + 1))
    assert decision == "escalate_stalled"


def test_waiting_permission_is_preserved_however_quiet():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS * 10, attention_reason="waiting_permission"))
    assert decision == "waiting_user"


def test_waiting_input_is_preserved_however_quiet():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS * 10, attention_reason="waiting_input"))
    assert decision == "waiting_user"


def test_an_agent_error_is_left_alone_however_quiet():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS * 10, attention_reason="agent_error"))
    assert decision == "waiting_user"


def test_a_waiting_user_flag_is_not_re_escalated_at_the_flag_threshold():
    # Rule 18: a flag already raised for the user is left alone, not overwritten with `stalled`.
    decision, _ = policy_baseline.decide(_at(QUIET_FLAG_SECS + 1, attention_reason="waiting_user"))
    assert decision == "waiting_user"


def test_a_waiting_user_flag_is_still_relaunched_past_the_relaunch_threshold():
    # Rule 18's other half: `waiting_user` keeps the flag but the agent is still relaunched.
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS + 1, attention_reason="waiting_user"))
    assert decision == "escalate_stalled"


def test_an_idle_planning_orchestrator_is_never_nudged_or_flagged():
    decision, _ = policy_baseline.decide(
        _at(QUIET_RELAUNCH_SECS * 100, seat="orchestrator", goal_status="planning", status="idle")
    )
    assert decision == "waiting_user"


def test_a_session_nobody_is_waiting_on_raises_nothing():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS + 1, work_is_active=False))
    assert decision == "continue"


def test_a_non_live_status_is_outside_the_quiet_clock():
    decision, _ = policy_baseline.decide(_at(QUIET_RELAUNCH_SECS + 1, status="exited"))
    assert decision == "continue"


def test_a_missing_activity_timestamp_cannot_be_measured():
    session = dataclasses.replace(BASE, last_activity_at=None, launched_at=None)
    decision, _ = policy_baseline.decide(session)
    assert decision == "continue"
