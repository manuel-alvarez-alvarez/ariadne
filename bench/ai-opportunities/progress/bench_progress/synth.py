"""Builds the synthetic half of the dataset: scenario families, each producing several window
instances that vary only in timing or counts, so the family's split placement (dev or eval,
task item 7) carries every instance with it. Each generator documents, in its own
`rationale`, why its instances carry the label they do - the by-construction label a synthetic
case has, as opposed to a real one's observed or inferred label (task items 6 and 10).

Every window's `now` is the latest timestamp anywhere in it: no event and no activity stamp is
ever after the decision point (task item 9).
"""
from __future__ import annotations

import dataclasses
from datetime import datetime, timedelta, timezone

from .schema import Event, SessionState, Window

_EPOCH = datetime(2026, 1, 5, 12, 0, 0, tzinfo=timezone.utc)


def _ts(seconds_from_epoch: float) -> str:
    return (_EPOCH + timedelta(seconds=seconds_from_epoch)).isoformat().replace("+00:00", "Z")


def _mk(
    id_: str,
    family: str,
    split: str,
    label: str,
    rationale: str,
    session: SessionState,
    events: list[Event],
    ambiguous: bool = False,
    tags: tuple[str, ...] = (),
) -> Window:
    now_dt = datetime.fromisoformat(session.now.replace("Z", "+00:00"))
    elapsed = None
    if session.last_activity_at:
        last_dt = datetime.fromisoformat(session.last_activity_at.replace("Z", "+00:00"))
        elapsed = (now_dt - last_dt).total_seconds()
    return Window(
        id=id_,
        family=family,
        split=split,  # type: ignore[arg-type]
        provenance="synthetic",
        label=label,  # type: ignore[arg-type]
        label_source="by_construction",
        session=session,
        events=events,
        rationale=rationale,
        ambiguous=ambiguous,
        tags=tags,
        elapsed_since_activity_s=elapsed,
    )


# ---------------------------------------------------------------------------
# A. useful_progress
# ---------------------------------------------------------------------------


def progress_steady(split: str, n: int) -> list[Window]:
    """A1: an author editing and testing, activity recent throughout - the ordinary case
    neither the baseline nor an advisory should ever touch."""
    family = f"progress-steady-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t + 0), "user_prompt_submit", "implement the retry helper"),
            Event(_ts(t + 5), "pre_tool_use", "read src/retry.py", tool_name="read"),
            Event(_ts(t + 8), "post_tool_use", "read 40 lines", tool_name="read", ok=True),
            Event(_ts(t + 20), "pre_tool_use", "edit src/retry.py", tool_name="edit"),
            Event(_ts(t + 35), "post_tool_use", "applied one hunk", tool_name="edit", ok=True),
            Event(_ts(t + 50), "pre_tool_use", "pytest tests/test_retry.py", tool_name="execute"),
            Event(_ts(t + 90), "post_tool_use", "6 passed", tool_name="execute", ok=True),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason=None,
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 90 + 30 + i),  # a short, ordinary gap after the last event
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "useful_progress",
                "distinct steps (read, edit, test) each ending ok, activity throughout; "
                "the ordinary healthy case.",
                session,
                events,
            )
        )
    return out


def progress_long_tool(split: str, n: int) -> list[Window]:
    """A2: one tool call still running, no event since it started. Timestamps alone say the
    session has been silent past the flag or even the relaunch threshold, but the silence is the
    single long operation, not a stall - the false-escalation case the current watchdog is
    already exposed to, since its clock cannot see what a long call is doing."""
    from .thresholds import QUIET_FLAG_SECS, QUIET_NUDGE_SECS, QUIET_RELAUNCH_SECS

    family = f"progress-long-tool-{split}"
    out = []
    durations = [QUIET_NUDGE_SECS + 30, QUIET_FLAG_SECS - 30, QUIET_FLAG_SECS + 120, QUIET_RELAUNCH_SECS - 60,
                 QUIET_RELAUNCH_SECS + 300][:n] or [QUIET_FLAG_SECS + 60]
    while len(durations) < n:
        durations.append(durations[-1] + 600)
    for i, quiet_for in enumerate(durations):
        t = i * 10_000.0
        start = _ts(t)
        events = [
            Event(_ts(t - 30), "post_tool_use", "3 files changed in the last edit", tool_name="edit", ok=True),
            Event(start, "pre_tool_use", "cargo build --release (full workspace)", tool_name="execute"),
        ]
        session = SessionState(
            status="running",
            seat="author",
            attention_reason=None,
            last_activity_at=start,
            launched_at=_ts(t - 3600),
            now=_ts(t + quiet_for),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "useful_progress",
                f"a single build still running {quiet_for:.0f}s after it started; the agent is "
                "mid-turn on one real operation, not quiet.",
                session,
                events,
                tags=("long_tool",),
            )
        )
    return out


def progress_retry(split: str, n: int) -> list[Window]:
    """A3: a transient failure retried, each retry a real attempt at the same fix, ending in
    success - legitimate retries, not a loop."""
    family = f"progress-retry-{split}"
    out = []
    for i in range(n):
        t = i * 400.0
        events = [
            Event(_ts(t + 0), "pre_tool_use", "pip install -r requirements.txt", tool_name="execute"),
            Event(_ts(t + 20), "post_tool_use", "connection reset by peer", tool_name="execute", ok=False),
            Event(_ts(t + 25), "pre_tool_use", "pip install -r requirements.txt", tool_name="execute"),
            Event(_ts(t + 45), "post_tool_use", "connection reset by peer", tool_name="execute", ok=False),
            Event(_ts(t + 50), "pre_tool_use", "pip install -r requirements.txt --retries 5", tool_name="execute"),
            Event(_ts(t + 95), "post_tool_use", "installed 42 packages", tool_name="execute", ok=True),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason=None,
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 120),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "useful_progress",
                "a flaky network error retried with a widened flag, ending in success: a real "
                "attempt each time, not a repeat of the same dead end.",
                session,
                events,
            )
        )
    return out


def progress_testloop_frequency(split: str, n: int) -> list[Window]:
    """A4: a test rerun after every real edit, several times - the healthy session a frequency-
    only repetition rule flags, because the same tool name appears often in the recent window
    even though each call's outcome changed (task item 3; acceptance criterion on a simplistic
    detector's false positives)."""
    family = f"progress-testloop-freq-{split}"
    out = []
    for i in range(n):
        t = i * 500.0
        events = []
        failing = 5
        for round_ in range(4):
            off = round_ * 60
            events.append(Event(_ts(t + off), "pre_tool_use", "edit src/parser.py", tool_name="edit"))
            events.append(Event(_ts(t + off + 10), "post_tool_use", "applied one hunk", tool_name="edit", ok=True))
            events.append(
                Event(_ts(t + off + 20), "pre_tool_use", "pytest tests/test_parser.py", tool_name="execute")
            )
            failing = max(0, failing - 2)
            events.append(
                Event(
                    _ts(t + off + 40),
                    "post_tool_use",
                    f"{failing} failed, {10 - failing} passed" if failing else "10 passed",
                    tool_name="execute",
                    ok=(failing == 0),
                )
            )
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason=None,
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 4 * 60 + 60),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "useful_progress",
                "pytest reruns after real edits with the failing count falling each time: "
                "the same tool name repeats, but each call's outcome is new.",
                session,
                events,
                tags=("testloop_foil",),
            )
        )
    return out


def progress_completed(split: str, n: int) -> list[Window]:
    """A5: the task already landed; the session ended cleanly. Nobody need act."""
    family = f"progress-completed-{split}"
    out = []
    for i in range(n):
        t = i * 200.0
        events = [
            Event(_ts(t + 0), "post_tool_use", "approve", tool_name="review", ok=True),
            Event(_ts(t + 5), "stop", "end_turn"),
            Event(_ts(t + 10), "session_end", "task landed"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="exited",
            seat="author",
            attention_reason=None,
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 7200),  # long after - an exited session is never on the quiet clock
            relaunches=0,
            task_status="done",
            goal_status="active",
            work_is_active=False,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "useful_progress",
                "the task landed and the session exited; there is nothing left to wait on.",
                session,
                events,
                tags=("completed",),
            )
        )
    return out


# ---------------------------------------------------------------------------
# B. repetitive_failure
# ---------------------------------------------------------------------------


def loop_identical_command(split: str, n_families: int, snapshots_per_family: int) -> list[Window]:
    """B1: the same command fails the same way, repeated with no fix between repeats, and each
    repeat keeps the activity clock fresh - the case the current watchdog cannot see (it is
    never quiet long enough to flag) and a frequency-only repetition rule can (task item 13:
    separating "no activity" from "activity without progress"). Several snapshots at growing
    repeat counts let a detection-delay comparison (task item 12, "where timestamps permit")."""
    out = []
    for f in range(n_families):
        family = f"loop-identical-{split}-{f:02d}"
        t0 = f * 20_000.0
        for s in range(1, snapshots_per_family + 1):
            events = []
            for r in range(s):
                off = r * 45.0
                events.append(
                    Event(_ts(t0 + off), "pre_tool_use", "cargo test -p ariadne-store", tool_name="execute")
                )
                events.append(
                    Event(
                        _ts(t0 + off + 20),
                        "post_tool_use",
                        "error[E0308]: mismatched types at store.rs:412",
                        tool_name="execute",
                        ok=False,
                    )
                )
            last = events[-1].ts
            session = SessionState(
                status="idle",
                seat="author",
                attention_reason=None,
                last_activity_at=last,
                launched_at=_ts(t0 - 600),
                now=_ts(t0 + s * 45.0 + 30),  # activity stays fresh: quiet_secs is always small
                relaunches=0,
                task_status="in_progress",
                goal_status="active",
                work_is_active=True,
            )
            window = _mk(
                f"{family}-s{s:02d}",
                family,
                split,
                "repetitive_failure",
                f"the identical command fails with the identical error {s} times running, "
                "nothing changes between repeats, yet the activity clock never crosses the "
                "current watchdog's thresholds.",
                session,
                events,
                tags=("multi_snapshot",),
            )
            # `elapsed_since_activity_s` is near-constant across snapshots here (every repeat
            # refreshes `last_activity_at`), which is the point - but useless to rank "how fast
            # did detection happen" by. For this family alone it is overridden to mean elapsed
            # time since the loop began, which does grow with the repeat count.
            window = dataclasses.replace(window, elapsed_since_activity_s=s * 45.0 + 30.0)
            out.append(window)
    return out


# ---------------------------------------------------------------------------
# C. waiting_user_input
# ---------------------------------------------------------------------------


def waiting_permission(split: str, n: int) -> list[Window]:
    family = f"waiting-permission-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "pre_tool_use", "git push origin feature-branch", tool_name="execute"),
            Event(_ts(t + 2), "permission_request", "push to a remote"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="running",
            seat="author",
            attention_reason="waiting_permission",
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 2 * 3600),  # hours idle on a person - still not "stalled"
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "waiting_user_input",
                "a permission question is open; the session is blocked on a person, not quiet.",
                session,
                events,
            )
        )
    return out


def waiting_input(split: str, n: int) -> list[Window]:
    family = f"waiting-input-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "agent_message", "which database migration tool should this use?"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason="waiting_input",
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 3 * 3600),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "waiting_user_input",
                "the agent asked the user a direct question and is idle until it is answered.",
                session,
                events,
            )
        )
    return out


def waiting_planning_orchestrator(split: str, n: int) -> list[Window]:
    family = f"waiting-planning-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "agent_message", "I'd split this into three tasks - does that match what you want?"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="orchestrator",
            attention_reason=None,
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 5 * 3600),  # idle for hours - rule 31 exempts it entirely
            relaunches=0,
            task_status=None,
            goal_status="planning",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "waiting_user_input",
                "an idle planning orchestrator is waiting on the user's answer, not silent "
                "(spec 009 rule 31), however long it sits idle.",
                session,
                events,
            )
        )
    return out


def waiting_agent_error(split: str, n: int) -> list[Window]:
    family = f"waiting-agent-error-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "session.error", "the model API returned a 529 overloaded error"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason="agent_error",
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 3600),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "waiting_user_input",
                "the agent reported an error and is left alone rather than nudged over it "
                "(spec 009 rule 19); a person is already the one who must act.",
                session,
                events,
            )
        )
    return out


def waiting_user_flag(split: str, n: int) -> list[Window]:
    """C5: already flagged `waiting_user` (e.g. a PR ready to merge) and quiet past the flag
    threshold - must not be re-escalated as `stalled` (spec 009 rule 18)."""
    family = f"waiting-user-flag-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "post_tool_use", "approve", tool_name="review", ok=True),
            Event(_ts(t + 5), "message", "the change is ready to land; merge when you are ready"),
        ]
        last = events[-1].ts
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason="waiting_user",
            last_activity_at=last,
            launched_at=_ts(t - 600),
            now=_ts(t + 1200),  # past the flag threshold, short of the relaunch one
            relaunches=0,
            task_status="approved",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "waiting_user_input",
                "already flagged for the user with something to merge; quiet past the flag "
                "threshold must not become `stalled` on top of it.",
                session,
                events,
            )
        )
    return out


# ---------------------------------------------------------------------------
# D. insufficient_evidence
# ---------------------------------------------------------------------------


def insufficient_missing_timestamps(split: str, n: int) -> list[Window]:
    family = f"insufficient-missing-ts-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [Event(_ts(t), "post_tool_use", "edit applied", tool_name="edit", ok=True)]
        session = SessionState(
            status="running",
            seat="author",
            attention_reason=None,
            last_activity_at=None,  # the one stamp the clock reads is missing
            launched_at=None,
            now=_ts(t + 60),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "insufficient_evidence",
                "neither activity stamp the watchdog's clock reads is present, so no decision "
                "about quiet time can be grounded.",
                session,
                events,
                ambiguous=True,
            )
        )
    return out


def insufficient_truncated(split: str, n: int) -> list[Window]:
    family = f"insufficient-truncated-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        # One tool call's end only - its start fell outside this window (spec 008 rule 13: a
        # page that cuts a call's start off drops the end too; here we deliberately keep only
        # the end, to test a policy offered a transcript cut the same way).
        events = [Event(_ts(t), "post_tool_use", "3 passed", tool_name="execute", ok=True)]
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason=None,
            last_activity_at=_ts(t),
            launched_at=_ts(t - 7200),
            now=_ts(t + 60),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "insufficient_evidence",
                "the window holds one tool call's end with no start and nothing before it: "
                "too little history to tell progress from a lucky single call.",
                session,
                events,
                ambiguous=True,
            )
        )
    return out


def insufficient_gap(split: str, n: int) -> list[Window]:
    family = f"insufficient-gap-{split}"
    out = []
    for i in range(n):
        t = i * 300.0
        events = [
            Event(_ts(t), "pre_tool_use", "pytest", tool_name="execute"),
            # a gap far larger than the surrounding deltas: a dropped console event (spec 008
            # rule 15), not a real silence - there is no way to tell what happened in it.
            Event(_ts(t + 9000), "post_tool_use", "2 passed", tool_name="execute", ok=True),
        ]
        session = SessionState(
            status="idle",
            seat="author",
            attention_reason=None,
            last_activity_at=_ts(t + 9000),
            launched_at=_ts(t - 600),
            now=_ts(t + 9060),
            relaunches=0,
            task_status="in_progress",
            goal_status="active",
            work_is_active=True,
        )
        out.append(
            _mk(
                f"{family}-{i:02d}",
                family,
                split,
                "insufficient_evidence",
                "a 2.5-hour gap between a call's start and end with nothing recorded in "
                "between is a hole in the transcript, not two hours of visible silence.",
                session,
                events,
                ambiguous=True,
            )
        )
    return out


def build_all() -> list[Window]:
    windows: list[Window] = []
    windows += progress_steady("dev", 9)
    windows += progress_steady("eval", 5)
    windows += progress_long_tool("dev", 5)
    windows += progress_long_tool("eval", 4)
    windows += progress_retry("dev", 7)
    windows += progress_retry("eval", 4)
    windows += progress_testloop_frequency("dev", 8)
    windows += progress_testloop_frequency("eval", 4)
    windows += progress_completed("dev", 4)
    windows += progress_completed("eval", 3)

    windows += loop_identical_command("dev", 3, 5)
    windows += loop_identical_command("eval", 2, 5)

    windows += waiting_permission("dev", 4)
    windows += waiting_permission("eval", 3)
    windows += waiting_input("dev", 4)
    windows += waiting_input("eval", 3)
    windows += waiting_planning_orchestrator("dev", 3)
    windows += waiting_planning_orchestrator("eval", 2)
    windows += waiting_agent_error("dev", 3)
    windows += waiting_agent_error("eval", 2)
    windows += waiting_user_flag("dev", 3)
    windows += waiting_user_flag("eval", 2)

    windows += insufficient_missing_timestamps("dev", 3)
    windows += insufficient_missing_timestamps("eval", 2)
    windows += insufficient_truncated("dev", 3)
    windows += insufficient_truncated("eval", 2)
    windows += insufficient_gap("dev", 3)
    windows += insufficient_gap("eval", 2)

    return windows
