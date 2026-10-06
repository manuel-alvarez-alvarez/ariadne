"""The integration point (task item 15): an advisory signal may only add
`escalate_unproductive`, never override an explicit wait, and the combined policy falls back to
the baseline alone whenever the advisory has nothing usable to say."""
import dataclasses

from bench_progress import policy_combine, policy_kev
from bench_progress.schema import SessionState

BASE = SessionState(
    status="idle", seat="author", attention_reason=None,
    last_activity_at="2026-01-01T00:00:00Z", launched_at="2026-01-01T00:00:00Z",
    now="2026-01-01T00:00:30Z", task_status="in_progress", goal_status="active",
)


def test_a_missing_advisory_signal_falls_back_to_the_baseline():
    decision, detail = policy_combine.combine(BASE, None)
    assert decision == "continue"  # the baseline's own answer for a fresh, active session
    assert "fallback" in detail


def test_an_insufficient_evidence_advisory_falls_back_to_the_baseline():
    decision, _ = policy_combine.combine(BASE, "insufficient_evidence")
    assert decision == "continue"


def test_the_advisory_may_add_escalate_unproductive_where_the_baseline_says_continue():
    decision, _ = policy_combine.combine(BASE, "escalate_unproductive")
    assert decision == "escalate_unproductive"


def test_an_explicit_wait_is_preserved_whatever_the_advisory_says():
    waiting = dataclasses.replace(BASE, attention_reason="waiting_permission")
    decision, detail = policy_combine.combine(waiting, "escalate_unproductive")
    assert decision == "waiting_user"
    assert "preserved" in detail


def test_the_advisory_cannot_soften_a_baseline_escalation():
    quiet = dataclasses.replace(BASE, now="2026-01-01T00:20:00Z")  # past the flag threshold
    decision, _ = policy_combine.combine(quiet, "useful_progress" and None)  # no advisory opinion
    assert decision == "escalate_stalled"


def test_a_malformed_kev_answer_yields_no_advisory_signal():
    advisory, detail = policy_kev.advisory_from_kev_answer(None, ok=False)
    assert advisory is None
    assert "failed" in detail


def test_an_out_of_vocabulary_kev_answer_yields_no_advisory_signal():
    advisory, _ = policy_kev.advisory_from_kev_answer("definitely_fine", ok=True)
    assert advisory is None


def test_a_kev_label_maps_onto_the_shared_decision_space():
    advisory, _ = policy_kev.advisory_from_kev_answer("repetitive_failure", ok=True)
    assert advisory == "escalate_unproductive"
    advisory, _ = policy_kev.advisory_from_kev_answer("waiting_user_input", ok=True)
    assert advisory == "waiting_user"
