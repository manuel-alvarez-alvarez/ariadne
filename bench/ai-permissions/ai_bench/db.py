"""Load real, approved permission requests out of the local Ariadne DB (`--real`).

Opened read-only, never written to. See AGENTS.md and the task brief: this machine may run a
production daemon with its own DB, so a probe here must never write to it or start a server.
"""
from __future__ import annotations

import json
import sqlite3
import zlib
from pathlib import Path
from typing import Any

HOME_PLACEHOLDER = "/home/user"
DEFAULT_REPOSITORY_PLACEHOLDER = "/repo/ariadne"
STRATIFIED_MINIMUM = 300


def default_db_path() -> Path:
    return Path.home() / ".ariadne" / "ariadne.db"


def _decode_payload(payload: bytes, codec: str) -> dict[str, Any]:
    data = zlib.decompress(payload, -15) if codec == "deflate" else payload
    return json.loads(data)


def _rewrite(value: Any, home: str, worktree: str | None, repository: str) -> Any:
    if isinstance(value, str):
        if worktree:
            value = value.replace(worktree, repository)
        return value.replace(home, HOME_PLACEHOLDER)
    if isinstance(value, dict):
        return {k: _rewrite(v, home, worktree, repository) for k, v in value.items()}
    if isinstance(value, list):
        return [_rewrite(v, home, worktree, repository) for v in value]
    return value


def _to_case(event_id: str, acp: dict, options: list, worktree: str | None, repository: str, home: str) -> dict[str, Any]:
    tool_call = {
        "toolCallId": acp.get("toolCallId", "c1"),
        "name": acp.get("name", "unknown"),
        "title": acp.get("title", ""),
        "kind": acp.get("kind", "other"),
        "rawInput": acp.get("rawInput", {}) or {},
        "locations": acp.get("locations", []) or [],
    }
    tool_call = _rewrite(tool_call, home, worktree, repository)
    clean_options = [
        {"optionId": o.get("optionId"), "name": o.get("name"), "kind": o.get("kind")}
        for o in options
        if isinstance(o, dict)
    ]
    return {
        "id": "real-%s" % event_id,
        "set": "real",
        "expected": "allow",
        "category": "real-%s" % tool_call["kind"],
        "note": "An approved real permission request from agent_events, replayed for coverage.",
        "repository": repository,
        "request": {"toolCall": tool_call, "options": clean_options},
    }


def load_real_cases(
    db_path: Path | None = None,
    minimum: int = STRATIFIED_MINIMUM,
    repository_placeholder: str = DEFAULT_REPOSITORY_PLACEHOLDER,
) -> list[dict[str, Any]]:
    """Real approved requests as `set: real` cases, one per distinct (kind, title), stratified
    by kind. Returns an empty list, cleanly, when the DB is absent."""
    db_path = db_path or default_db_path()
    if not db_path.exists():
        return []
    home = str(Path.home())
    uri = "file:%s?mode=ro" % db_path
    con = sqlite3.connect(uri, uri=True)
    try:
        rows = con.execute(
            """
            select e.id, e.payload, e.payload_codec, s.worktree_path
            from agent_events e
            left join agent_sessions s on s.id = e.session_id
            where e.kind = 'permission_request'
            order by e.id
            """
        ).fetchall()
    finally:
        con.close()

    by_kind: dict[str, list[dict[str, Any]]] = {}
    seen_pairs: set[tuple[str, str]] = set()
    for event_id, payload, codec, worktree in rows:
        try:
            body = _decode_payload(payload, codec)
        except (zlib.error, json.JSONDecodeError):
            continue
        acp = body.get("acp")
        options = body.get("options")
        if not isinstance(acp, dict) or not isinstance(options, list):
            continue
        kind = acp.get("kind", "other")
        title = acp.get("title", "")
        pair = (kind, title)
        if pair in seen_pairs:
            continue
        seen_pairs.add(pair)
        case = _to_case(event_id, acp, options, worktree, repository_placeholder, home)
        by_kind.setdefault(kind, []).append(case)

    total = sum(len(v) for v in by_kind.values())
    if total <= minimum:
        return [case for cases in by_kind.values() for case in cases]

    # Stratified sample: take a share of `minimum` from each kind proportional to its size,
    # in the stable order the DB returned them (oldest first), so a rerun is reproducible.
    selected: list[dict[str, Any]] = []
    for kind, cases in by_kind.items():
        share = max(1, round(minimum * len(cases) / total))
        selected.extend(cases[:share])
    return selected
