"""Build the `state` and `questions` values an evaluator sends to its model.

Each builder is a pure function of a case's request and repository. `fields` picks which
parts of the call the state carries (`JSON_KEYS` / `STRUCTURED_KEYS` name them), in order;
an empty value is left out.
"""
from __future__ import annotations

import json as jsonlib
from typing import Any

INPUT_CUT = 2_000

# field name (as a builder's `fields` names it) -> key used in the `json` representation.
JSON_KEYS = {
    "name": "name",
    "title": "tool",
    "kind": "kind",
    "input": "input",
    "command": "command",
    "description": "description",
    "paths": "paths",
    "repository": "repository",
    "options": "options",
}

# field name -> key used in the `structured` representation's "key: value" lines.
STRUCTURED_KEYS = {
    "name": "name",
    "title": "tool",
    "kind": "kind",
    "input": "input",
    "command": "command",
    "description": "reason",
    "paths": "path",
    "repository": "cwd",
    "options": "options",
}


def _option_names(request: dict[str, Any]) -> str:
    return ", ".join(o.get("name", "") for o in request.get("options", []) or [])


def _paths(request: dict[str, Any]) -> list[str]:
    tool_call = request.get("toolCall", {})
    raw_input = tool_call.get("rawInput", {}) or {}
    found = []
    for key in ("file_path", "path", "url"):
        value = raw_input.get(key)
        if isinstance(value, str):
            found.append(value)
    for location in tool_call.get("locations", []) or []:
        path = location.get("path") if isinstance(location, dict) else None
        if isinstance(path, str):
            found.append(path)
    return found


def _field_value(field: str, request: dict[str, Any], repository: str, input_cut: int = INPUT_CUT) -> Any:
    tool_call = request.get("toolCall", {})
    raw_input = tool_call.get("rawInput", {}) or {}
    if field == "name":
        return tool_call.get("name")
    if field == "title":
        return tool_call.get("title")
    if field == "kind":
        return tool_call.get("kind")
    if field == "input":
        compact = jsonlib.dumps(raw_input, ensure_ascii=False, separators=(",", ":"))
        return compact[:input_cut]
    if field == "command":
        return raw_input.get("command")
    if field == "description":
        return raw_input.get("description")
    if field == "paths":
        return _paths(request)
    if field == "repository":
        return repository
    if field == "options":
        return _option_names(request)
    raise ValueError("unknown field %r" % field)


def build_raw(request: dict[str, Any]) -> str:
    """The compact JSON of `rawInput` alone."""
    raw_input = request.get("toolCall", {}).get("rawInput", {}) or {}
    return jsonlib.dumps(raw_input, ensure_ascii=False, separators=(",", ":"))


def build_structured(request: dict[str, Any], repository: str, fields: list[str], input_cut: int = INPUT_CUT) -> str:
    lines = []
    for field in fields:
        value = _field_value(field, request, repository, input_cut)
        if value in (None, "", []):
            continue
        key = STRUCTURED_KEYS[field]
        if isinstance(value, list):
            value = ", ".join(value)
        lines.append("%s: %s" % (key, value))
    return "\n".join(lines)


def build_json(request: dict[str, Any], repository: str, fields: list[str], input_cut: int = INPUT_CUT) -> dict[str, Any]:
    obj: dict[str, Any] = {}
    for field in fields:
        value = _field_value(field, request, repository, input_cut)
        if value in (None, "", []):
            continue
        obj[JSON_KEYS[field]] = value
    return obj


def noul_question(instructions: str, criteria: dict[str, str] | None = None) -> dict[str, Any]:
    """The single `decision` question as a `noul`: the probability that its statement holds.
    `criteria`, when given, is keyed `true` and `false`."""
    question: dict[str, Any] = {"type": "noul", "instructions": instructions}
    if criteria is not None:
        question["criteria"] = criteria
    return {"decision": question}


def choice_question(instructions: str, criteria: dict[str, str]) -> dict[str, Any]:
    """The single `decision` question as a `choice` between `criteria`'s keys."""
    return {"decision": {"type": "choice", "instructions": instructions, "criteria": criteria}}
