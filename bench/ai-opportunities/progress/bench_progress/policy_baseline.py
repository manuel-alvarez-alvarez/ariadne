"""The current-policy baseline: `check_session_quiet` (`crates/ariadne-daemon/src/scheduler/
quiet.rs`) ported to a pure function of one snapshot, so it can be replayed offline against the
same windows as every other policy. It reproduces, in order:

- the explicit-wait guard (spec 009 rules 17-19): a session `waiting_permission`,
  `waiting_input` or carrying an `agent_error` is blocked, not quiet, and is left alone;
- the idle-planning-orchestrator exemption (rule 31): never nudged or flagged however long it
  sits idle between turns;
- the active-work gate (rule 16, `attention.rs::work_is_active`): a session nobody is waiting
  on (a reviewer that voted, an orchestrator of a finished goal) raises nothing;
- the three-threshold clock (rule 9): nudge at 180s, flag `stalled` at 600s (unless the session
  already carries `waiting_user`, rule 18), kill-and-relaunch at 1800s;
- only an idle session is nudged (rule 11); a session mid-turn waits out the thresholds behind
  it.

What this port does not model: the one-nudge-per-situation and one-flag-per-situation dedup
(`Quiet.nudged`/`flagged` in quiet.rs), since that is about not repeating a delivery across
scheduler passes, not about what the right decision is at one snapshot. Graded here is the
decision a fresh look at the session would make, which is what every other policy is graded on
too.
"""
from __future__ import annotations

from datetime import datetime

from .schema import Decision, SessionState
from .thresholds import QUIET_FLAG_SECS, QUIET_NUDGE_SECS, QUIET_RELAUNCH_SECS

_EXPLICIT_WAIT_REASONS = {"waiting_permission", "waiting_input", "agent_error"}


def _parse(ts: str) -> datetime:
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))


def quiet_seconds(session: SessionState) -> float | None:
    """`last_heard_from`: the newer of `last_activity_at` and `launched_at`, against `now`."""
    stamps = [s for s in (session.last_activity_at, session.launched_at) if s]
    if not stamps:
        return None
    since = max(_parse(s) for s in stamps)
    now = _parse(session.now)
    return (now - since).total_seconds()


def decide(session: SessionState) -> tuple[Decision, str]:
    if session.attention_reason in _EXPLICIT_WAIT_REASONS:
        return "waiting_user", f"explicit wait guard: {session.attention_reason}"

    if session.seat == "orchestrator" and session.goal_status == "planning" and session.status == "idle":
        return "waiting_user", "idle planning orchestrator is waiting on the user, not silent (rule 31)"

    if not session.work_is_active:
        return "continue", "nobody is waiting on this session's work (rule 16)"

    if session.status not in ("running", "idle"):
        return "continue", f"not live for the quiet clock: {session.status}"

    secs = quiet_seconds(session)
    if secs is None:
        return "continue", "no activity timestamp to measure quiet from"

    if secs < QUIET_NUDGE_SECS:
        return "continue", f"quiet {secs:.0f}s < nudge threshold {QUIET_NUDGE_SECS}s"

    if secs >= QUIET_RELAUNCH_SECS:
        return "escalate_stalled", f"quiet {secs:.0f}s >= relaunch threshold {QUIET_RELAUNCH_SECS}s"

    if secs >= QUIET_FLAG_SECS:
        if session.attention_reason == "waiting_user":
            return "waiting_user", "a flag already raised for the user is left alone (rule 18)"
        return "escalate_stalled", f"quiet {secs:.0f}s >= flag threshold {QUIET_FLAG_SECS}s"

    if session.status == "running":
        return "continue", "mid-turn: left to the thresholds behind the nudge (rule 11)"

    return "nudge", f"quiet {secs:.0f}s, idle: nudge threshold crossed"
