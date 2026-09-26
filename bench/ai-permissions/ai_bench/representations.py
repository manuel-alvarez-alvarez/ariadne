"""Build the `state` and `questions` values a configuration sends to the model.

A representation is a pure function of the case's request and repository. `json` with
`fields = title, kind, input, repository, options` reproduces the production state built by
`crates/ariadne-daemon/src/ai_permissions/decide.rs` exactly -- see `states` in `harness.py`.
"""
from __future__ import annotations

import json as jsonlib
from typing import Any

from .features import derive_features

INPUT_CUT = 2_000

# field name (as it appears in a config's `fields` list) -> key used in the `json`
# representation. These match the literal keys `decide.rs` sends today.
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


def _field_value(field: str, request: dict[str, Any], repository: str) -> Any:
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
        return compact[:INPUT_CUT]
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


def build_raw(request: dict[str, Any], repository: str, fields: list[str]) -> str:
    """The compact JSON of `rawInput` alone. `fields` and `repository` are ignored."""
    raw_input = request.get("toolCall", {}).get("rawInput", {}) or {}
    return jsonlib.dumps(raw_input, ensure_ascii=False, separators=(",", ":"))


def build_structured(request: dict[str, Any], repository: str, fields: list[str]) -> str:
    lines = []
    for field in fields:
        value = _field_value(field, request, repository)
        if value in (None, "", []):
            continue
        key = STRUCTURED_KEYS[field]
        if isinstance(value, list):
            value = ", ".join(value)
        lines.append("%s: %s" % (key, value))
    return "\n".join(lines)


def build_json(request: dict[str, Any], repository: str, fields: list[str]) -> dict[str, Any]:
    obj: dict[str, Any] = {}
    for field in fields:
        value = _field_value(field, request, repository)
        if value in (None, "", []):
            continue
        obj[JSON_KEYS[field]] = value
    return obj


def build_normalized(base: str, request: dict[str, Any], repository: str, fields: list[str]):
    derived = derive_features(request, repository)
    if base == "json":
        obj = build_json(request, repository, fields)
        obj.update(derived)
        return obj
    if base == "structured":
        text = build_structured(request, repository, fields)
        extra = "\n".join("%s: %s" % (k, v) for k, v in derived.items())
        return text + ("\n" + extra if text else extra)
    raise ValueError("normalized must be based on 'structured' or 'json', got %r" % base)


REPRESENTATIONS = {
    "raw": build_raw,
    "structured": build_structured,
    "json": build_json,
}


def build_state(config: dict[str, Any], request: dict[str, Any], repository: str):
    """The `state` value a configuration sends, exactly as it would be sent."""
    representation = config["representation"]
    fields = config.get("fields") or []
    if representation == "normalized":
        base = config.get("normalized_base", "json")
        return build_normalized(base, request, repository, fields)
    builder = REPRESENTATIONS.get(representation)
    if builder is None:
        raise ValueError("unknown representation %r" % representation)
    return builder(request, repository, fields)


def build_question(question_cfg: dict[str, Any]) -> dict[str, Any]:
    """The single `decision` question laya's `Router.predict` expects."""
    qtype = question_cfg["type"]
    q: dict[str, Any] = {"type": qtype, "instructions": question_cfg["instructions"]}
    if qtype == "choice":
        q["criteria"] = question_cfg["criteria"]
    elif qtype == "noul":
        if "criteria" in question_cfg:
            q["criteria"] = question_cfg["criteria"]
        if "display_labels" in question_cfg:
            q["labels"] = question_cfg["display_labels"]
    else:
        raise ValueError("unsupported question type %r" % qtype)
    return {"decision": q}
