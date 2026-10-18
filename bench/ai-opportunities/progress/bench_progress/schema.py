"""The shapes every part of this benchmark agrees on: an event, a window, a label and a
decision. A window is what a policy sees; a label is what it is graded against. Nothing here
reaches into `crates/` — it is a plain description of the data the daemon already produces
(specs 008, 009, 018), read back as JSON.
"""
from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from typing import Any, Literal

#: The four outcomes a window can be labelled with (task item 2).
Label = Literal["useful_progress", "repetitive_failure", "waiting_user_input", "insufficient_evidence"]

LABELS: tuple[Label, ...] = (
    "useful_progress",
    "repetitive_failure",
    "waiting_user_input",
    "insufficient_evidence",
)

#: The decision a policy proposes for one window. `escalate_stalled` is the existing silence
#: claim (no activity for too long); `escalate_unproductive` is the new claim this bench
#: measures (activity that is not progress), kept apart per task item 13.
Decision = Literal[
    "continue", "nudge", "escalate_stalled", "escalate_unproductive", "waiting_user", "insufficient_evidence"
]

#: Where a window came from, and how sure its label is.
Provenance = Literal["synthetic", "real_sanitized"]
LabelSource = Literal["by_construction", "observed", "inferred"]

#: The `AttentionReason` wire values a session may already carry (`ariadne-core/src/lib.rs`).
AttentionReason = Literal[
    "waiting_permission", "waiting_input", "waiting_user", "agent_error", "disconnected", "stalled", "exhausted"
]
SessionStatus = Literal["starting", "running", "idle", "exited", "failed"]
Seat = Literal["orchestrator", "author", "reviewer", None]


@dataclass(frozen=True)
class Event:
    """One session event as the daemon's console would show it (spec 008 rule 13): a kind, a
    short summary and the timestamp it was recorded at. No raw tool payload is carried — a
    policy decides from the same shape `ariadne session logs` prints, not from secrets inside
    a call's arguments."""

    ts: str  # RFC3339, UTC
    kind: str  # "pre_tool_use", "post_tool_use", "user_prompt_submit", "stop", "agent_message", ...
    summary: str
    tool_name: str | None = None
    tool_call_id: str | None = None
    ok: bool | None = None  # for a post_tool_use / stop: did it end in an error

    def to_json(self) -> dict[str, Any]:
        return {k: v for k, v in asdict(self).items() if v is not None or k in ("ts", "kind", "summary")}


@dataclass(frozen=True)
class SessionState:
    """The session-row fields `check_session_quiet` reads (`scheduler/quiet.rs`,
    `ariadne-store`'s `AgentSession`), as they stand at the decision point `now`."""

    status: SessionStatus
    seat: Seat
    attention_reason: AttentionReason | None
    last_activity_at: str | None  # RFC3339
    launched_at: str | None  # RFC3339
    now: str  # RFC3339 — the clock the decision is made at; no event or field may be after it
    relaunches: int = 0
    task_status: str | None = None  # e.g. "ready", "in_progress", "under_review", "approved"
    goal_status: str | None = None  # e.g. "planning", "active", "completed"
    work_is_active: bool = True  # `attention::work_is_active` for this seat and status


@dataclass(frozen=True)
class Window:
    """One labelled case: a bounded window of events ending at a decision point, the session
    state visible there, and the label it is graded against. `family` groups every window that
    came from one session or one synthetic scenario, so a split never cuts a family in half
    (task item 7)."""

    id: str
    family: str
    split: Literal["dev", "eval"]
    provenance: Provenance
    label: Label
    label_source: LabelSource
    session: SessionState
    events: list[Event]
    rationale: str
    ambiguous: bool = False
    tags: tuple[str, ...] = ()
    elapsed_since_activity_s: float | None = None  # convenience, derived, for detection-delay grading

    def to_json(self) -> dict[str, Any]:
        d = asdict(self)
        d["session"] = asdict(self.session)
        d["events"] = [e.to_json() if isinstance(e, Event) else e for e in self.events]
        return d

    @staticmethod
    def from_json(d: dict[str, Any]) -> "Window":
        session = SessionState(**d["session"])
        events = [Event(**e) for e in d["events"]]
        return Window(
            id=d["id"],
            family=d["family"],
            split=d["split"],
            provenance=d["provenance"],
            label=d["label"],
            label_source=d["label_source"],
            session=session,
            events=events,
            rationale=d["rationale"],
            ambiguous=d.get("ambiguous", False),
            tags=tuple(d.get("tags", ())),
            elapsed_since_activity_s=d.get("elapsed_since_activity_s"),
        )


def load_windows(path: str) -> list[Window]:
    windows = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            windows.append(Window.from_json(json.loads(line)))
    return windows


def dump_windows(windows: list[Window], path: str) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        for w in windows:
            fh.write(json.dumps(w.to_json(), sort_keys=True))
            fh.write("\n")


@dataclass(frozen=True)
class Prediction:
    """One policy's answer for one window, timed, committed so metrics can be recomputed
    without a rerun (common acceptance criteria)."""

    window_id: str
    policy: str
    decision: Decision
    latency_ms: float
    detail: str = ""

    def to_json(self) -> dict[str, Any]:
        return asdict(self)

    @staticmethod
    def from_json(d: dict[str, Any]) -> "Prediction":
        return Prediction(
            window_id=d["window_id"],
            policy=d["policy"],
            decision=d["decision"],
            latency_ms=d["latency_ms"],
            detail=d.get("detail", ""),
        )


def load_predictions(path: str) -> list[Prediction]:
    preds = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            preds.append(Prediction.from_json(json.loads(line)))
    return preds


def dump_predictions(preds: list[Prediction], path: str) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        for p in preds:
            fh.write(json.dumps(p.to_json(), sort_keys=True))
            fh.write("\n")
