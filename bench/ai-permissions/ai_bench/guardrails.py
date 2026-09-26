"""Regex guardrails that force `escalate` before the model is asked.

The pattern subset is deliberately plain `re`/regex: no lookaround, no backreferences, so the
same list can compile under Python's `re` and the Rust `regex` crate later.
"""
from __future__ import annotations

import json
import re
from typing import Any

FORBIDDEN = ("(?=", "(?!", "(?<=", "(?<!", "\\1", "\\2", "\\3", "\\4", "\\5")


def load_guardrails(path: str | None) -> list[dict[str, Any]]:
    if not path:
        return []
    with open(path, encoding="utf-8") as f:
        rules = json.load(f)
    if not isinstance(rules, list):
        raise ValueError("%s: guardrails must be a JSON list" % path)
    for rule in rules:
        pattern = rule.get("pattern", "")
        for forbidden in FORBIDDEN:
            if forbidden in pattern:
                raise ValueError(
                    "guardrail %r: pattern uses %r, which the common regex subset excludes"
                    % (rule.get("name"), forbidden)
                )
        re.compile(pattern)  # fail fast on a pattern Python's own `re` rejects
    return rules


def _target_text(rule: dict[str, Any], request: dict[str, Any]) -> str:
    tool_call = request.get("toolCall", {})
    raw_input = tool_call.get("rawInput", {}) or {}
    target = rule["target"]
    if target == "command":
        return str(raw_input.get("command", ""))
    if target == "title":
        return str(tool_call.get("title", ""))
    if target == "input":
        return json.dumps(raw_input, ensure_ascii=False)
    if target == "path":
        parts = [str(raw_input.get(k, "")) for k in ("file_path", "path", "url")]
        parts += [str(loc.get("path", "")) for loc in tool_call.get("locations", []) or []]
        return "\n".join(p for p in parts if p)
    raise ValueError("unknown guardrail target %r" % target)


def _applies(rule: dict[str, Any], request: dict[str, Any]) -> bool:
    applies_to = rule.get("applies_to") or {}
    tool_call = request.get("toolCall", {})
    kinds = applies_to.get("kinds")
    if kinds and tool_call.get("kind") not in kinds:
        return False
    names = applies_to.get("names")
    if names and tool_call.get("name") not in names:
        return False
    return True


def match(guardrails: list[dict[str, Any]], request: dict[str, Any]) -> str | None:
    """The name of the first guardrail whose pattern matches this request, else None."""
    for rule in guardrails:
        if not _applies(rule, request):
            continue
        text = _target_text(rule, request)
        if re.search(rule["pattern"], text):
            return rule["name"]
    return None
