"""The simple repetition rule Kev is compared against: count how many of the most recent tool
calls share a tool name, and escalate past a fixed count. It reads only `tool_name`, never the
call's arguments or output, which is the point of including it — a repeated `cargo test` after
each real edit looks identical to a repeated `cargo test` that never got a fix in between, so
this rule cannot tell a healthy test loop from a stuck one (task item 3, and the acceptance
criterion that the comparison include healthy sessions a simplistic detector would flag).

It preserves the two explicit waits (task item 4) because it is meant to replace only the
stalled-vs-looping judgment, not the parts of the current policy nothing here claims to improve.
"""
from __future__ import annotations

from .schema import Decision, SessionState, Event
from .policy_baseline import _EXPLICIT_WAIT_REASONS

#: How many of the last tool calls must share a name before this rule calls it a loop.
REPETITION_THRESHOLD = 3
#: How many of the most recent events it looks at at all.
WINDOW = 6


def decide(session: SessionState, events: list[Event]) -> tuple[Decision, str]:
    if session.attention_reason in _EXPLICIT_WAIT_REASONS or session.attention_reason == "waiting_user":
        return "waiting_user", f"explicit wait guard: {session.attention_reason}"

    recent_tool_events = [e for e in events[-WINDOW:] if e.kind == "post_tool_use" and e.tool_name]
    if len(recent_tool_events) < REPETITION_THRESHOLD:
        return "continue", f"fewer than {REPETITION_THRESHOLD} recent tool calls to count"

    counts: dict[str, int] = {}
    for e in recent_tool_events:
        counts[e.tool_name] = counts.get(e.tool_name, 0) + 1
    name, count = max(counts.items(), key=lambda kv: kv[1])
    if count >= REPETITION_THRESHOLD:
        return "escalate_unproductive", f"'{name}' called {count} times in the last {len(recent_tool_events)} tool calls"

    return "continue", "no tool name repeats often enough in the window"
