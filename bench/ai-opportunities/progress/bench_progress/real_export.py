"""Exports a small, sanitized sample of real session windows from the daemon's own
`~/.ariadne/ariadne.db`, read-only, the same way `bench/ai-permissions/ai_bench/db.py` reads
`agent_events` without writing to it or taking any lock the daemon needs (a plain `mode=ro`
SQLite URI connection, never a write, never a server). This file never changes a live session,
a specification or any daemon setting - it only reads a history the daemon already wrote.

Sanitizing: every path under the user's home is rewritten to a placeholder, and a tool call's
actual command or content is never carried into a window - only its tool category and a stable
hash of its input, so two calls with the same input (a real repeat) still read as the same call
without leaking what either call actually did.

A real window's label is an after-the-fact judgement from the same evidence a policy would see
(`label_source="inferred"`), never a certainty: ambiguous ones are marked, with the call this
script made written into `rationale` (task item 10, and the acceptance criterion distinguishing
supported from inferred labels).

No future outcome reaches a real window either (task item 9): `now` is the window's own last
event, never a live `agent_sessions` column, and `status`/`attention_reason` are derived from
that same last event alone (`_derive_state`) rather than read from the live row, which can
already describe the session after events this window does not show. `task_status` and
`goal_status` are always `None` here for the same reason - the daemon keeps no point-in-time
history of either, so there is no safe way to say what they were at the window's own decision
point; synthetic windows (`synth.py`) are the ones that exercise those two fields.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import sqlite3
import zlib
from dataclasses import dataclass

from .schema import Event, SessionState, Window

DB_PATH = os.path.expanduser("~/.ariadne/ariadne.db")
_HOME_RE = re.compile(re.escape(os.path.expanduser("~")))


def _sanitize_text(text: str) -> str:
    return _HOME_RE.sub("/home/user", text)


def _decode_payload(payload: bytes, codec: str) -> dict:
    data = zlib.decompress(payload, -15) if codec == "deflate" else payload
    return json.loads(data)


def _input_hash(payload: dict) -> str:
    acp = payload.get("acp") or {}
    material = json.dumps(
        {"name": acp.get("name"), "raw_input": acp.get("rawInput") or payload.get("tool_input")}, sort_keys=True
    )
    return hashlib.sha256(_sanitize_text(material).encode("utf-8")).hexdigest()[:8]


def _summarize(kind: str, payload: dict) -> tuple[str, str | None, bool | None]:
    acp = payload.get("acp") or {}
    tool_category = acp.get("kind") or acp.get("name")
    if kind in ("pre_tool_use", "post_tool_use") and tool_category:
        h = _input_hash(payload)
        status = acp.get("status")
        ok = None
        if kind == "post_tool_use":
            ok = status not in ("failed", "error") if status else None
        summary = f"{tool_category} call, input-hash {h}" + (f", status={status}" if status else "")
        return summary, tool_category, ok
    if kind == "permission_request":
        return "a permission question is open", None, None
    if kind == "permission.replied":
        return "the permission question was answered", None, None
    if kind == "user_prompt_submit":
        return "the user posted a prompt", None, None
    if kind == "agent_message":
        return "the agent sent a message", None, None
    if kind == "stop":
        return "the turn ended", None, None
    if kind == "session_start":
        return "the session started", None, None
    if kind == "session_end":
        return "the session ended", None, None
    if kind == "session.error":
        return "the agent reported an error", None, False
    return kind.replace("_", " "), None, None


@dataclass
class _SessionRow:
    id: str
    seat: str | None
    status: str  # derived from the window's own last event, never the live row (see `_derive_state`)
    attention_reason: str | None  # derived the same way
    last_activity_at: str | None
    launched_at: str | None
    task_status: str | None  # always None: no point-in-time history exists for this field
    goal_status: str | None  # always None, for the same reason


def _connect() -> sqlite3.Connection:
    return sqlite3.connect(f"file:{DB_PATH}?mode=ro", uri=True)


def _derive_state(events: list[Event]) -> tuple[str, str | None]:
    """Status and attention_reason, read off the window's own last event alone - never the
    live `agent_sessions` row, which can describe the session as it is now, after events this
    window does not show (the temporal leak task item 9 forbids: a later tool call, a cleared
    flag, a task that has since finished). A session's `status` and `attention_reason` change
    without a matching `agent_events` row in the general case (the scheduler writes them
    directly), so there is no way to reconstruct their exact history from events bounded by
    `max_events` below - this is an approximation, grounded only in what the window shows,
    rather than a point-in-time snapshot the daemon does not keep."""
    last = events[-1]
    if last.kind == "session_end":
        return "exited", None
    if last.kind == "permission_request":
        return "running", "waiting_permission"
    if last.kind == "session.error":
        return "idle", "agent_error"
    if last.kind == "stop":
        return "idle", None
    return "running", None


def _infer_label(session_row: _SessionRow, events: list[Event]) -> tuple[str, str, bool, str]:
    """Returns (label, label_source, ambiguous, rationale). Mirrors the same evidence a policy
    sees - nothing here reads ahead of the window's own last event."""
    if session_row.attention_reason in ("waiting_permission", "waiting_input", "agent_error", "waiting_user"):
        return (
            "waiting_user_input",
            "observed",
            False,
            f"the window's last event derives attention_reason={session_row.attention_reason}, "
            "an explicit wait.",
        )
    if len(events) < 3:
        return (
            "insufficient_evidence",
            "observed",
            True,
            f"only {len(events)} events survive sanitizing/selection for this window.",
        )
    if session_row.status in ("exited",) and not session_row.attention_reason:
        return (
            "useful_progress",
            "inferred",
            False,
            "session ended with no standing attention flag; read as finished cleanly.",
        )
    tail = [e for e in events[-6:] if e.kind == "post_tool_use" and e.tool_name]
    if len(tail) >= 3:
        counts: dict[str, int] = {}
        for e in tail:
            counts[e.summary] = counts.get(e.summary, 0) + 1
        if max(counts.values()) >= 3:
            return (
                "repetitive_failure",
                "inferred",
                True,
                "the last several tool calls repeat the identical input-hash and tool: read as "
                "a loop, though a human reviewer did not confirm it against the real transcript.",
            )
    return (
        "useful_progress",
        "inferred",
        True,
        "no repeat and no standing wait found in the tail; read as ordinary progress by default, "
        "an inference rather than a confirmed outcome.",
    )


def export(limit_sessions: int = 20, min_events: int = 4, max_events: int = 40) -> list[Window]:
    if not os.path.exists(DB_PATH):
        return []
    con = _connect()
    try:
        cur = con.cursor()
        cur.execute(
            """
            select s.id, s.seat, s.launched_at
            from agent_sessions s
            where s.id in (select session_id from agent_events group by session_id having count(*) >= ?)
            order by s.id
            limit ?
            """,
            (min_events, limit_sessions),
        )
        rows = cur.fetchall()
        windows: list[Window] = []
        for (sid, seat, launched_at) in rows:
            cur.execute(
                "select kind, payload, payload_codec, created_at from agent_events "
                "where session_id = ? order by id desc limit ?",
                (sid, max_events),
            )
            raw = list(reversed(cur.fetchall()))
            events: list[Event] = []
            for kind, payload, codec, created_at in raw:
                try:
                    decoded = _decode_payload(payload, codec)
                except (zlib.error, json.JSONDecodeError):
                    continue
                summary, tool_name, ok = _summarize(kind, decoded)
                events.append(Event(ts=created_at, kind=kind, summary=summary, tool_name=tool_name, ok=ok))
            if not events:
                continue
            # `now` is the window's own last event alone - never extended by a live column,
            # which can hold a later timestamp this window's events do not show.
            now = events[-1].ts
            status, attention_reason = _derive_state(events)
            session_row = _SessionRow(
                id=sid,
                seat=seat,
                status=status,
                attention_reason=attention_reason,
                last_activity_at=now,
                launched_at=launched_at if launched_at and launched_at <= now else now,
                task_status=None,
                goal_status=None,
            )
            label, label_source, ambiguous, rationale = _infer_label(session_row, events)
            session = SessionState(
                status=status,
                seat=seat,
                attention_reason=attention_reason,
                last_activity_at=now,
                launched_at=session_row.launched_at,
                now=now,
                relaunches=0,
                task_status=None,
                goal_status=None,
                work_is_active=status != "exited",
            )
            family = f"real-{sid[:8]}"
            windows.append(
                Window(
                    id=f"{family}-tail",
                    family=family,
                    split="dev",  # assigned by the caller/generator; see build_real_dataset
                    provenance="real_sanitized",
                    label=label,  # type: ignore[arg-type]
                    label_source=label_source,  # type: ignore[arg-type]
                    session=session,
                    events=events,
                    rationale=rationale,
                    ambiguous=ambiguous,
                    tags=("real",),
                )
            )
        return windows
    finally:
        con.close()
