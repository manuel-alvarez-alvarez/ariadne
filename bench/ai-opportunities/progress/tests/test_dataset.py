"""Dataset separation: no family crosses dev/eval, and no window leaks a future timestamp into
what a policy is given (task items 7 and 9)."""
import dataclasses

import pytest
from bench_progress.dataset import DatasetError, check_family_split_integrity, check_no_future_leakage, load_dataset
from bench_progress.schema import Event, SessionState, Window

SESSION = SessionState(
    status="idle",
    seat="author",
    attention_reason=None,
    last_activity_at="2026-01-01T00:00:00Z",
    launched_at="2026-01-01T00:00:00Z",
    now="2026-01-01T00:10:00Z",
    task_status="in_progress",
    goal_status="active",
)


def _w(id_, family, split, label="useful_progress", events=(), session=SESSION):
    return Window(
        id=id_, family=family, split=split, provenance="synthetic", label=label,
        label_source="by_construction", session=session, events=list(events), rationale="test",
    )


def test_committed_dataset_loads_and_is_internally_consistent():
    dev, ev = load_dataset()
    assert len(dev) + len(ev) >= 120
    assert len(ev) >= 40
    dev_families = {w.family for w in dev}
    eval_families = {w.family for w in ev}
    assert not (dev_families & eval_families)


def test_a_family_split_across_dev_and_eval_is_rejected():
    windows = [_w("a", "fam-1", "dev"), _w("b", "fam-1", "eval")]
    with pytest.raises(DatasetError):
        check_family_split_integrity(windows)


def test_an_event_after_the_decision_point_is_rejected():
    future_event = Event(ts="2026-01-01T00:20:00Z", kind="post_tool_use", summary="after now")
    w = _w("a", "fam-1", "dev", events=[future_event])
    with pytest.raises(DatasetError):
        check_no_future_leakage(w)


def test_a_session_stamp_after_the_decision_point_is_rejected():
    bad_session = dataclasses.replace(SESSION, last_activity_at="2026-01-01T00:30:00Z")
    w = _w("a", "fam-1", "dev", session=bad_session)
    with pytest.raises(DatasetError):
        check_no_future_leakage(w)


def test_an_event_at_or_before_the_decision_point_is_accepted():
    w = _w("a", "fam-1", "dev", events=[Event(ts="2026-01-01T00:10:00Z", kind="stop", summary="end_turn")])
    check_no_future_leakage(w)  # does not raise
