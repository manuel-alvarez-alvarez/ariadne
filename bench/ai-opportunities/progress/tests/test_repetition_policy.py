"""The naive rule's defining property: it counts tool names alone, so it cannot tell a healthy,
edited-between-runs test loop from a stuck one (task item 3)."""
import dataclasses

from bench_progress import policy_repetition
from bench_progress.schema import Event, SessionState

SESSION = SessionState(
    status="idle", seat="author", attention_reason=None,
    last_activity_at="2026-01-01T00:00:00Z", launched_at="2026-01-01T00:00:00Z",
    now="2026-01-01T00:00:30Z", task_status="in_progress", goal_status="active",
)


def _events(names, ok_flags=None):
    ok_flags = ok_flags or [True] * len(names)
    return [
        Event(ts=f"2026-01-01T00:00:{i:02d}Z", kind="post_tool_use", summary=f"call {i}", tool_name=n, ok=ok)
        for i, (n, ok) in enumerate(zip(names, ok_flags))
    ]


def test_three_identical_tool_names_in_the_window_escalate():
    events = _events(["execute", "edit", "execute", "read", "execute"])
    decision, _ = policy_repetition.decide(SESSION, events)
    assert decision == "escalate_unproductive"


def test_a_mix_of_tool_names_does_not_escalate():
    events = _events(["execute", "edit", "read", "search", "execute"])
    decision, _ = policy_repetition.decide(SESSION, events)
    assert decision == "continue"


def test_fewer_than_three_tool_calls_does_not_escalate():
    events = _events(["execute", "execute"])
    decision, _ = policy_repetition.decide(SESSION, events)
    assert decision == "continue"


def test_the_rule_does_not_look_at_whether_the_outcome_changed():
    # Three `execute` calls, each one `ok` - a real healthy test loop by every measure except
    # the tool name repeating, which is all this rule reads.
    events = _events(["execute", "edit", "execute", "edit", "execute"], ok_flags=[False, True, False, True, True])
    decision, _ = policy_repetition.decide(SESSION, events)
    assert decision == "escalate_unproductive"


def test_an_explicit_wait_is_preserved_over_the_repetition_count():
    session = dataclasses.replace(SESSION, attention_reason="waiting_permission")
    events = _events(["execute", "execute", "execute"])
    decision, _ = policy_repetition.decide(session, events)
    assert decision == "waiting_user"
