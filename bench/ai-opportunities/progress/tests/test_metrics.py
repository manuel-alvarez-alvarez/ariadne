"""`compute` and `detection_delays` are recomputed from windows and predictions alone, so their
arithmetic needs its own small, hand-checked cases independent of the real dataset."""
from bench_progress import metrics
from bench_progress.schema import Prediction, SessionState, Window

SESSION = SessionState(
    status="idle", seat="author", attention_reason=None,
    last_activity_at="2026-01-01T00:00:00Z", launched_at="2026-01-01T00:00:00Z",
    now="2026-01-01T00:00:30Z", task_status="in_progress", goal_status="active",
)


def _w(id_, label, family=None):
    return Window(
        id=id_, family=family or id_, split="eval", provenance="synthetic", label=label,
        label_source="by_construction", session=SESSION, events=[], rationale="test",
    )


def test_a_false_escalation_is_counted_on_a_useful_progress_window():
    windows = [_w("a", "useful_progress")]
    preds = [Prediction("a", "p", "escalate_stalled", 1.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.false_escalations == 1
    assert m.false_escalation_rate == 1.0


def test_a_continue_on_useful_progress_is_not_a_false_escalation():
    windows = [_w("a", "useful_progress")]
    preds = [Prediction("a", "p", "continue", 1.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.false_escalations == 0


def test_a_missed_loop_is_counted_and_coverage_is_its_complement():
    windows = [_w("a", "repetitive_failure"), _w("b", "repetitive_failure")]
    preds = [Prediction("a", "p", "escalate_unproductive", 1.0), Prediction("b", "p", "continue", 1.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.missed_loops == 1
    assert m.missed_loop_rate == 0.5
    assert m.coverage == 0.5


def test_nudging_a_waiting_user_window_is_a_missed_question_but_continuing_is_not():
    windows = [_w("a", "waiting_user_input"), _w("b", "waiting_user_input")]
    preds = [Prediction("a", "p", "nudge", 1.0), Prediction("b", "p", "continue", 1.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.missed_user_questions == 1
    assert m.missed_user_question_rate == 0.5


def test_escalating_on_insufficient_evidence_is_counted():
    windows = [_w("a", "insufficient_evidence")]
    preds = [Prediction("a", "p", "escalate_stalled", 1.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.insufficient_evidence_escalations == 1


def test_latency_percentiles_come_from_the_committed_predictions():
    windows = [_w("a", "useful_progress"), _w("b", "useful_progress")]
    preds = [Prediction("a", "p", "continue", 10.0), Prediction("b", "p", "continue", 20.0)]
    m = metrics.compute(windows, preds, "p")
    assert m.mean_latency_ms == 15.0


def test_detection_delay_finds_the_first_snapshot_that_escalates():
    windows = [
        Window(
            id=f"fam-s{i}", family="fam", split="eval", provenance="synthetic", label="repetitive_failure",
            label_source="by_construction", session=SESSION, events=[], rationale="t",
            tags=("multi_snapshot",), elapsed_since_activity_s=float(i),
        )
        for i in range(1, 4)
    ]
    preds = [
        Prediction("fam-s1", "p", "continue", 1.0),
        Prediction("fam-s2", "p", "escalate_unproductive", 1.0),
        Prediction("fam-s3", "p", "escalate_unproductive", 1.0),
    ]
    delays = metrics.detection_delays(windows, preds, "p")
    assert len(delays) == 1
    assert delays[0].detected_at_elapsed_s == 2.0


def test_detection_delay_is_none_when_no_snapshot_ever_escalates():
    windows = [
        Window(
            id="fam-s1", family="fam", split="eval", provenance="synthetic", label="repetitive_failure",
            label_source="by_construction", session=SESSION, events=[], rationale="t",
            tags=("multi_snapshot",), elapsed_since_activity_s=1.0,
        )
    ]
    preds = [Prediction("fam-s1", "p", "continue", 1.0)]
    delays = metrics.detection_delays(windows, preds, "p")
    assert delays[0].detected_at_elapsed_s is None
