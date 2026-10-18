"""Renders a `Window` to the plain text an advisory model reads: the session state at the
decision point, then its bounded event window, oldest to newest, in the same `kind · summary`
shape `ariadne session logs` prints (spec 008 rule 22). Nothing after `session.now` is ever
in this text (task item 9: no future outcome reaches a model's input).
"""
from __future__ import annotations

from .schema import Window


def render_state(window: Window) -> str:
    s = window.session
    header = (
        f"seat={s.seat or 'none'} status={s.status} "
        f"task_status={s.task_status or 'none'} goal_status={s.goal_status or 'none'}\n"
        f"attention_reason={s.attention_reason or 'none'} work_is_active={s.work_is_active}\n"
        f"last_activity_at={s.last_activity_at or 'none'} launched_at={s.launched_at or 'none'} "
        f"relaunches={s.relaunches}\n"
        f"now={s.now}"
    )
    lines = [f"session: {header}", "", f"events ({len(window.events)}, oldest to newest, none after `now`):"]
    for e in window.events:
        bits = [e.ts, e.kind, e.summary]
        if e.tool_name:
            bits.append(f"tool={e.tool_name}")
        if e.ok is not None:
            bits.append(f"ok={e.ok}")
        lines.append(" ".join(bits))
    if not window.events:
        lines.append("(no events in this window)")
    return "\n".join(lines)
