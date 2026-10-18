"""The integration point task item 15 asks for: how an advisory model's opinion (Kev, or the
repetition rule) would sit beside the watchdog that already runs, for a later rollout that only
adds a signal rather than replaces one.

Rule, in order, the same for every advisory source:

1. The current policy's two explicit waits always win. Neither Kev nor the repetition rule is
   ever allowed to override a session already correctly left alone (task item 4) — this is
   checked against the baseline's own verdict, not against the advisory's, so the preservation
   holds whatever the advisory says.
2. If the advisory could not answer (task item 9's missing-evidence case, or an inference
   failure, or a latency timeout `run_kev.py` could not keep) the combined policy is the
   baseline alone: an advisory signal is additive, never a single point of failure for a
   decision the daemon already makes safely.
3. If the advisory's own answer is `insufficient_evidence`, the same fallback applies: an
   under-evidenced guess is not escalated on.
4. Otherwise, the advisory may add `escalate_unproductive` where the baseline would otherwise
   say `continue` or `nudge` — the one decision the baseline structurally cannot make, since it
   has no notion of repeated activity without progress (task item 13). It is never allowed to
   soften a baseline escalation, so a session the current policy already flags stays flagged.
"""
from __future__ import annotations

from .policy_baseline import decide as baseline_decide
from .schema import Decision, SessionState

#: What the baseline treats as final no matter what an advisory says.
_PRESERVED = {"waiting_user"}


def combine(session: SessionState, advisory: Decision | None, advisory_detail: str = "") -> tuple[Decision, str]:
    base_decision, base_detail = baseline_decide(session)

    if base_decision in _PRESERVED:
        return base_decision, f"preserved explicit wait over advisory: {base_detail}"

    if advisory is None or advisory == "insufficient_evidence":
        return base_decision, f"fallback to baseline (no usable advisory signal): {base_detail}"

    if advisory == "escalate_unproductive" and base_decision in ("continue", "nudge"):
        return "escalate_unproductive", f"advisory added what the baseline cannot see: {advisory_detail}"

    return base_decision, f"baseline stands: {base_detail}"
