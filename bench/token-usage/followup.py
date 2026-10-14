#!/usr/bin/env python3
"""Reconstruct privacy-preserving request histories from saved transcripts.

This is a follow-up to ``analyze.py``.  It does not change the pinned baseline
artifacts or canonical Ariadne accounting.  It writes identifiers, counts and
lengths only; raw transcript text and source databases stay outside Git.
"""

from __future__ import annotations

import argparse
import collections
import csv
import datetime as dt
import hashlib
import json
import sqlite3
import tempfile
from pathlib import Path
from typing import Any


VERSION = "2"
TOKENS = ("input_tokens", "cached_input_tokens", "output_tokens")


def rows(conn: sqlite3.Connection, query: str) -> list[dict[str, Any]]:
    return [dict(row) for row in conn.execute(query)]


def stamp(value: Any) -> dt.datetime | None:
    if not isinstance(value, str):
        return None
    try:
        return dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def usage() -> dict[str, int]:
    return {field: 0 for field in TOKENS}


def add(target: dict[str, int], source: dict[str, int]) -> None:
    for field in TOKENS:
        target[field] += max(0, int(source.get(field, 0)))


def uncached(item: dict[str, int]) -> int:
    return max(0, item["input_tokens"] - item["cached_input_tokens"])


def write_csv(path: Path, values: list[dict[str, Any]], fields: list[str]) -> None:
    with path.open("w", newline="", encoding="utf-8") as file:
        writer = csv.DictWriter(file, fieldnames=fields, extrasaction="ignore", lineterminator="\n")
        writer.writeheader()
        writer.writerows(values)


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def retained_digest(path: Path, cutoff: str) -> str:
    """Hash only JSONL lines retained by the documented cutoff."""
    value = hashlib.sha256()
    with path.open(encoding="utf-8", errors="replace") as file:
        for line in file:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record.get("timestamp"), str) and record["timestamp"] > cutoff:
                continue
            value.update(line.encode("utf-8", "replace"))
    return value.hexdigest()


def string_size(value: Any) -> tuple[int, str]:
    """Measure retained text blocks without claiming they are tokens."""
    if isinstance(value, str):
        return len(value), "text"
    if isinstance(value, list):
        total = 0
        supported = True
        for part in value:
            if isinstance(part, dict) and isinstance(part.get("text"), str):
                total += len(part["text"])
            elif isinstance(part, str):
                total += len(part)
            else:
                supported = False
        return total, "blocks" if supported else "partially_unsupported_blocks"
    return 0, "unsupported_content_type"


def claude_requests(path: Path, internal: dict[str, dict[str, Any]], cutoff: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, int]]:
    """Merge repeated Claude blocks by message ID and join tools by tool_use ID."""
    requests: dict[str, dict[str, Any]] = {}
    aliases: dict[str, str] = {}
    tool_calls: dict[str, dict[str, Any]] = {}
    tool_results: list[dict[str, Any]] = []
    session_id: str | None = None
    counts = collections.Counter()
    try:
        file = path.open(encoding="utf-8", errors="replace")
    except OSError:
        return [], [], counts
    with file:
        for line in file:
            try:
                record = json.loads(line)
            except ValueError:
                counts["invalid_json"] += 1
                continue
            if isinstance(record.get("timestamp"), str) and record["timestamp"] > cutoff:
                counts["after_cutoff"] += 1
                continue
            key = record.get("sessionId")
            if isinstance(key, str) and key in internal:
                session_id = internal[key]["id"]
            if record.get("type") != "assistant" or not isinstance(record.get("message"), dict):
                if record.get("type") == "user":
                    for part in (record.get("message") or {}).get("content") or []:
                        if isinstance(part, dict) and part.get("type") == "tool_result":
                            length, representation = string_size(part.get("content"))
                            tool_results.append({"session_id": session_id or "unmapped", "tool_call_id": str(part.get("tool_use_id") or "missing"), "result_chars": length, "result_representation": representation, "timestamp": record.get("timestamp") or "", "source_assistant_uuid": record.get("sourceToolAssistantUUID") or ""})
                continue
            message = record["message"]
            message_id = message.get("id")
            request_id = record.get("requestId")
            supplied_aliases = [value for value in (request_id, message_id) if isinstance(value, str)]
            known_ids = {aliases[value] for value in supplied_aliases if value in aliases}
            vendor_id = min(known_ids) if known_ids else (message_id if isinstance(message_id, str) else request_id)
            if not isinstance(vendor_id, str):
                counts["assistant_without_request_id"] += 1
                continue
            # A later block can bridge requestId-only and message.id-only
            # records.  Merge the connected component before usage selection.
            for other_id in sorted(known_ids - {vendor_id}):
                other = requests.pop(other_id)
                prior = requests.get(vendor_id)
                if prior is None:
                    requests[vendor_id] = other
                    prior = other
                elif other["usage_record"] and not prior["usage_record"]:
                    prior.update({field: other[field] for field in TOKENS})
                    prior["usage_record"] = 1
                    prior["usage_status"] = other["usage_status"]
                elif other["usage_record"] and prior["usage_record"] and any(prior[field] != other[field] for field in TOKENS):
                    prior["usage_status"] = "ambiguous_conflict"
                    counts["conflicting_usage_records"] += 1
                if other.get("usage_status") == "ambiguous_conflict":
                    prior["usage_status"] = "ambiguous_conflict"
                for call in tool_calls.values():
                    if call["request_id"] == other_id:
                        call["request_id"] = vendor_id
                for alias, target in list(aliases.items()):
                    if target == other_id:
                        aliases[alias] = vendor_id
            raw = message.get("usage")
            if raw is not None and not isinstance(raw, dict):
                counts["invalid_usage"] += 1
                raw = None
            if raw is None:
                counts["assistant_without_usage"] += 1
                item = None
            else:
                direct_input = int(raw.get("input_tokens") or 0)
                cache_create = int(raw.get("cache_creation_input_tokens") or 0)
                cached = int(raw.get("cache_read_input_tokens") or 0)
                item = {"input_tokens": direct_input + cache_create + cached, "cached_input_tokens": cached, "output_tokens": int(raw.get("output_tokens") or 0)}
            for alias in supplied_aliases:
                aliases[alias] = vendor_id
            if vendor_id in requests:
                # Keep every observed native spelling: this is also the
                # identity evidence used when copies from different files are
                # reconciled below.
                requests[vendor_id]["alias_ids"] = "|".join(sorted({*filter(None, requests[vendor_id].get("alias_ids", "").split("|")), *supplied_aliases}))
            if vendor_id in requests:
                counts["duplicate_request_records"] += 1
                prior = requests[vendor_id]
                if item is not None:
                    if not prior["usage_record"]:
                        prior.update(item)
                        prior["usage_record"] = 1
                        prior["usage_status"] = "present"
                    elif any(prior[field] != item[field] for field in TOKENS):
                        prior["usage_status"] = "ambiguous_conflict"
                        counts["conflicting_usage_records"] += 1
            else:
                initial = item or usage()
                requests[vendor_id] = {"family": "claude", "session_id": session_id or "unmapped", "vendor_session_id": record.get("sessionId") or "missing", "request_id": vendor_id, "alias_ids": "|".join(sorted(supplied_aliases)), "turn_id": "unknown_not_assumed_agent_turn", "response_id": message_id or "missing", "timestamp": record.get("timestamp") or "", "model": message.get("model") or "unknown", "effort": record.get("effort") or record.get("perTurnEffort") or "unknown", "tool_calls": 0, "usage_record": int(item is not None), "usage_status": "present" if item is not None else "missing", "deduplication": "alias_union_message.id_requestId", **initial}
            for part in message.get("content") or []:
                if isinstance(part, dict) and part.get("type") == "tool_use":
                    call_id = str(part.get("id") or "missing")
                    if call_id in tool_calls:
                        counts["duplicate_tool_call_blocks"] += 1
                    tool_calls[call_id] = {"family": "claude", "session_id": session_id or "unmapped", "request_id": vendor_id, "tool_call_id": call_id, "tool_name": part.get("name") or "unknown", "result_chars": 0, "result_representation": "missing_result", "timestamp": record.get("timestamp") or "", "association": "native_tool_use_id", "request_association": "native_assistant_content"}
    tools = list(tool_calls.values())
    by_call = {(row["session_id"], row["tool_call_id"]): row for row in tools}
    for result in tool_results:
        target = by_call.get((result["session_id"], result["tool_call_id"]))
        if target is None:
            tools.append({"family": "claude", "session_id": result["session_id"], "request_id": "unmapped", "tool_call_id": result["tool_call_id"], "tool_name": "unknown", "result_chars": result["result_chars"], "result_representation": result["result_representation"], "timestamp": result["timestamp"], "association": "unmatched_tool_result", "request_association": "unavailable"})
            counts["unmatched_tool_results"] += 1
        else:
            target["result_chars"] = max(target["result_chars"], result["result_chars"])
            target["result_representation"] = result["result_representation"]
    for row in requests.values():
        row["tool_calls"] = sum(call["request_id"] == row["request_id"] for call in tools)
    return list(requests.values()), tools, counts


def codex_requests(path: Path, internal: dict[str, dict[str, Any]], cutoff: str) -> tuple[list[dict[str, Any]], dict[str, int]]:
    """Read the non-overlapping response usage records in current rollouts."""
    requests: list[dict[str, Any]] = []
    counts = collections.Counter()
    try:
        file = path.open(encoding="utf-8", errors="replace")
    except OSError:
        return requests, counts
    with file:
        for line in file:
            try:
                record = json.loads(line)
            except ValueError:
                counts["invalid_json"] += 1
                continue
            if isinstance(record.get("timestamp"), str) and record["timestamp"] > cutoff:
                counts["after_cutoff"] += 1
                continue
            payload = record.get("payload") if isinstance(record.get("payload"), dict) else {}
            for value in (record.get("sessionId"), record.get("id"), payload.get("sessionId"), payload.get("session_id"), payload.get("id")):
                if isinstance(value, str) and value in internal:
                    sid = internal[value]["id"]
                    break
            else:
                sid = "unmapped"
            if record.get("type") == "token_usage_record" and isinstance(payload.get("response_id"), str) and isinstance(payload.get("usage"), dict):
                raw = payload["usage"]
                requests.append({"family": "codex", "session_id": sid, "vendor_session_id": payload.get("session_id") or "missing", "request_id": payload["response_id"], "turn_id": payload.get("turn_id") or "missing", "response_id": payload["response_id"], "timestamp": record.get("timestamp") or "", "model": "unknown", "effort": "unknown", "tool_calls": 0, "usage_record": 1, "deduplication": "response_id", "input_tokens": int(raw.get("input_tokens") or 0), "cached_input_tokens": int(raw.get("cached_input_tokens") or 0), "output_tokens": int(raw.get("output_tokens") or 0)})
                counts["request_usage_records"] += 1
            if record.get("type") == "event_msg" and payload.get("type") == "token_count":
                counts["cumulative_counter_records"] += 1
            elif record.get("record_type") == "state":
                counts["state_records_without_usage"] += 1
    return requests, counts


def codex_tools(path: Path, internal: dict[str, dict[str, Any]], cutoff: str) -> tuple[list[dict[str, Any]], dict[str, int]]:
    """Join Codex function calls/results by native call ID, not request order."""
    calls: dict[tuple[str, str], dict[str, Any]] = {}
    results: list[dict[str, Any]] = []
    vendor_session: str | None = None
    counts = collections.Counter()
    try:
        file = path.open(encoding="utf-8", errors="replace")
    except OSError:
        return [], counts
    with file:
        for line in file:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record.get("timestamp"), str) and record["timestamp"] > cutoff:
                continue
            payload = record.get("payload") if isinstance(record.get("payload"), dict) else {}
            meta = payload.get("session_meta") if isinstance(payload.get("session_meta"), dict) else {}
            discovered_session = payload.get("session_id") or meta.get("id") or (payload.get("id") if payload.get("type") == "session_meta" else None)
            if isinstance(discovered_session, str):
                vendor_session = discovered_session
            sid = internal.get(vendor_session, {}).get("id", "unmapped") if isinstance(vendor_session, str) else "unmapped"
            item = payload.get("item") if isinstance(payload.get("item"), dict) else payload
            passthrough = item.get("internal_chat_message_metadata_passthrough") if isinstance(item.get("internal_chat_message_metadata_passthrough"), dict) else {}
            turn_id = passthrough.get("turn_id") or payload.get("turn_id") or ""
            if item.get("type") in {"function_call", "custom_tool_call"} and isinstance(item.get("call_id"), str):
                key = (sid, item["call_id"])
                calls[key] = {"family": "codex", "session_id": sid, "request_id": "unavailable_no_native_response_link", "tool_call_id": item["call_id"], "tool_name": item.get("name") or "unknown", "result_chars": 0, "result_representation": "missing_result", "timestamp": record.get("timestamp") or "", "turn_id": turn_id, "association": "native_call_id", "request_association": "turn_retained_response_unresolved"}
            elif item.get("type") in {"function_call_output", "custom_tool_call_output"} and isinstance(item.get("call_id"), str):
                length, representation = string_size(item.get("output"))
                results.append({"sid": sid, "call_id": item["call_id"], "length": length, "representation": representation, "timestamp": record.get("timestamp") or ""})
    for result in results:
        target = calls.get((result["sid"], result["call_id"]))
        if target:
            target["result_chars"] = max(target["result_chars"], result["length"])
            target["result_representation"] = result["representation"]
        else:
            calls[(result["sid"], result["call_id"])] = {"family": "codex", "session_id": result["sid"], "request_id": "unmapped", "tool_call_id": result["call_id"], "tool_name": "unknown", "result_chars": result["length"], "result_representation": result["representation"], "timestamp": result["timestamp"], "turn_id": "", "association": "unmatched_native_call_id", "request_association": "unavailable"}
            counts["unmatched_outputs"] += 1
    return list(calls.values()), counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--codex-transcripts", type=Path, default=Path("/Users/malvarez/.codex/sessions"))
    parser.add_argument("--claude-transcripts", type=Path, default=Path("/Users/malvarez/.claude/projects"))
    parser.add_argument("--source-label", required=True)
    parser.add_argument("--audit-goal-id", default="01m4y11d2c2q2zt30qgg5gjkj9")
    parser.add_argument("--snapshot-location", default="external SQLite snapshot supplied to --database")
    parser.add_argument("--baseline-database", type=Path, default=Path("/tmp/ariadne-token-usage-snapshot.XXXXXX.db"))
    args = parser.parse_args()
    source = args.database.resolve()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    sessions = rows(conn, "SELECT * FROM agent_sessions")
    session_by_id = {row["id"]: row for row in sessions}
    tasks = {row["id"]: row for row in rows(conn, "SELECT * FROM tasks")}
    agents = {row["id"]: row for row in rows(conn, "SELECT * FROM task_agents")}
    internal = {row["internal_session_id"]: row for row in sessions if row.get("internal_session_id")}
    cutoff = max(value for value in (conn.execute("SELECT MAX(created_at) FROM agent_events").fetchone()[0], conn.execute("SELECT MAX(updated_at) FROM session_usage").fetchone()[0], conn.execute("SELECT MAX(created_at) FROM stat_facts").fetchone()[0]) if value)
    baseline_session_ids: set[str] = set()
    baseline_usage_by_session: dict[str, dict[str, int]] = {}
    baseline_hash = "unavailable"
    if args.baseline_database.is_file():
        baseline_conn = sqlite3.connect(f"file:{args.baseline_database.resolve()}?mode=ro", uri=True)
        baseline_conn.row_factory = sqlite3.Row
        baseline_session_ids = {row[0] for row in baseline_conn.execute("SELECT id FROM agent_sessions")}
        for row in rows(baseline_conn, "SELECT * FROM session_usage"):
            add(baseline_usage_by_session.setdefault(row["session_id"], usage()), row)
        baseline_conn.close()
        baseline_hash = digest(args.baseline_database)

    formats: list[dict[str, Any]] = []
    request_rows: list[dict[str, Any]] = []
    tool_rows: list[dict[str, Any]] = []
    manifest = hashlib.sha256()
    manifest_rows: list[dict[str, Any]] = []
    claude_counts = collections.Counter()
    if args.claude_transcripts.exists():
        files = sorted(args.claude_transcripts.rglob("*.jsonl"))
        for path in files:
            found, results, counts = claude_requests(path, internal, cutoff)
            claude_counts.update(counts)
            request_rows.extend(found)
            tool_rows.extend(results)
            if found:
                manifest.update(("claude:" + digest(path) + "\n").encode())
                manifest_rows.append({"family": "claude", "source_id": hashlib.sha256(f"claude:{path.relative_to(args.claude_transcripts)}".encode()).hexdigest(), "retained_content_sha256": retained_digest(path, cutoff), "parser": "assistant_message_usage_and_tool_ids", "mapped_requests": sum(row["session_id"] != "unmapped" for row in found), "unmapped_requests": sum(row["session_id"] == "unmapped" for row in found)})
        formats.append({"family": "claude", "files": len(files), "mapped_sessions": len({row["session_id"] for row in request_rows if row["session_id"] != "unmapped"}), "request_history": "supported", "reason": "assistant requestId/message.id and per-message usage are retained"})
    else:
        formats.append({"family": "claude", "files": 0, "mapped_sessions": 0, "request_history": "unavailable", "reason": "transcript root does not exist"})

    codex_counts = collections.Counter()
    if args.codex_transcripts.exists():
        files = sorted(args.codex_transcripts.rglob("rollout-*.jsonl"))
        for path in files:
            found, counts = codex_requests(path, internal, cutoff)
            request_rows.extend(found)
            codex_tool_rows, tool_counts = codex_tools(path, internal, cutoff)
            tool_rows.extend(codex_tool_rows)
            codex_counts.update(tool_counts)
            codex_counts.update(counts)
            if found:
                manifest.update(("codex:" + digest(path) + "\n").encode())
                manifest_rows.append({"family": "codex", "source_id": hashlib.sha256(f"codex:{path.relative_to(args.codex_transcripts)}".encode()).hexdigest(), "retained_content_sha256": retained_digest(path, cutoff), "parser": "token_usage_record_and_function_call_ids", "mapped_requests": sum(row["session_id"] != "unmapped" for row in found), "unmapped_requests": sum(row["session_id"] == "unmapped" for row in found)})
        formats.append({"family": "codex", "files": len(files), "mapped_sessions": len({row["session_id"] for row in request_rows if row["family"] == "codex" and row["session_id"] != "unmapped"}), "request_history": "supported_with_field_semantics_limit", "reason": "token_usage_record has a native response_id and non-overlapping usage field; turn and thread totals overlap"})
    else:
        formats.append({"family": "codex", "files": 0, "mapped_sessions": 0, "request_history": "unavailable", "reason": "transcript root does not exist"})
    formats.append({"family": "ariadne_sqlite", "files": 1, "mapped_sessions": len(sessions), "request_history": "unavailable", "reason": "session_usage is cumulative by session/source; daemon events lack per-request token attribution"})
    opencode = Path("/Users/malvarez/.local/share/opencode/opencode.db")
    formats.append({"family": "opencode", "files": int(opencode.exists()), "mapped_sessions": 0, "request_history": "unavailable", "reason": "token-bearing records exist, but no native Ariadne session join or validated deduplication rule is retained"})

    # A response/message ID is authoritative within its format.  Build alias
    # components across files before selecting usage, because a requestId-only
    # copy and message.id-only copy can be bridged only by another file.
    parent = list(range(len(request_rows)))
    def find(index: int) -> int:
        while parent[index] != index:
            parent[index] = parent[parent[index]]
            index = parent[index]
        return index
    def union(left: int, right: int) -> None:
        left, right = find(left), find(right)
        if left != right:
            parent[right] = left
    identities: dict[tuple[str, str, str], int] = {}
    for index, row in enumerate(request_rows):
        names = {row["request_id"], *filter(None, row.get("alias_ids", "").split("|"))}
        for name in names:
            key = (row["family"], row["vendor_session_id"], name)
            if key in identities:
                union(index, identities[key])
            else:
                identities[key] = index
    components: dict[int, list[dict[str, Any]]] = collections.defaultdict(list)
    for index, row in enumerate(request_rows):
        components[find(index)].append(row)
    excluded_requests: list[dict[str, Any]] = []
    selected: list[dict[str, Any]] = []
    canonical_request_ids: dict[tuple[str, str, str], str] = {}
    for component in components.values():
        # Prefer a retained usage row with an Ariadne mapping.  This avoids
        # losing mapping fields when an earlier copy is only an alias shell.
        representative = dict(next((row for row in component if row.get("usage_status", "present") == "present" and row["session_id"] != "unmapped"), component[0]))
        aliases = sorted({name for row in component for name in (row["request_id"], *filter(None, row.get("alias_ids", "").split("|")))})
        representative["alias_ids"] = "|".join(aliases)
        for name in aliases:
            canonical_request_ids[(representative["family"], representative["session_id"], name)] = representative["request_id"]
        present = [row for row in component if row.get("usage_status", "present") == "present"]
        token_sets = {tuple(row[field] for field in TOKENS) for row in present}
        ambiguous = any(row.get("usage_status") == "ambiguous_conflict" for row in component) or len(token_sets) > 1
        if ambiguous:
            excluded_requests.append({"family": representative["family"], "session_id": representative["session_id"], "vendor_session_id": representative["vendor_session_id"], "request_id": representative["request_id"], "alias_ids": representative["alias_ids"], "reason": "ambiguous_conflicting_usage"})
            continue
        if not present:
            excluded_requests.append({"family": representative["family"], "session_id": representative["session_id"], "vendor_session_id": representative["vendor_session_id"], "request_id": representative["request_id"], "alias_ids": representative["alias_ids"], "reason": "missing_usage"})
            continue
        representative.update({field: present[0][field] for field in TOKENS})
        representative["usage_record"] = 1
        representative["usage_status"] = "present"
        selected.append(representative)
    request_rows = selected
    conflicts = {row["request_id"] for row in excluded_requests if row["reason"] == "ambiguous_conflicting_usage"}
    unmapped_request_records = sum(row["session_id"] == "unmapped" for row in request_rows)
    request_rows = [row for row in request_rows if row["session_id"] != "unmapped"]
    # Calls are parsed before cross-file request alias components exist.  Move
    # every call to the selected request identity, then calculate counts from
    # the merged calls rather than stale per-file counters.
    for tool in tool_rows:
        key = (tool["family"], tool["session_id"], tool["request_id"])
        canonical = canonical_request_ids.get(key)
        if canonical:
            tool["request_id"] = canonical
    # Request histories use native request IDs.  No timeline is inferred for
    # unmapped records, and elapsed spans are per session rather than summed.
    timeline: list[dict[str, Any]] = []
    grouped: dict[str, list[dict[str, Any]]] = collections.defaultdict(list)
    for row in request_rows:
        if row["session_id"] != "unmapped":
            grouped[row["session_id"]].append(row)
    for sid, values in sorted(grouped.items()):
        dated = sorted((stamp(value["timestamp"]) for value in values if stamp(value["timestamp"])))
        first, last = (dated[0], dated[-1]) if dated else (None, None)
        total = usage()
        for value in values:
            add(total, value)
        session = session_by_id[sid]
        task = tasks.get(session.get("task_id"), {})
        agent = agents.get(session.get("task_agent_id"), {})
        timeline.append({"session_id": sid, "goal_id": session.get("goal_id") or "", "task_id": session.get("task_id") or "", "seat": session.get("seat") or "", "workflow_column": agent.get("step") or task.get("step") or "unknown", "model": session.get("model") or "", "effort": session.get("effort") or "default", "launch_id": session.get("launch_id") or "", "task_outcome": task.get("status") or "unattributed", "session_status": session.get("status") or "", "partial": int(session.get("status") not in {"exited", "failed"}), "requests": len(values), "first_request_at": first.isoformat().replace("+00:00", "Z") if first else "", "last_request_at": last.isoformat().replace("+00:00", "Z") if last else "", "observed_request_span_secs": round((last - first).total_seconds()) if first and last else "", "interval_definition": "first-to-last recorded model request; parallel requests are not added", **total, "uncached_input_tokens": uncached(total)})

    # Associate a daemon delivery only when the retained session ID is native.
    # Multiple deliveries can share a first later request, which is explicitly
    # an ambiguous temporal candidate rather than a causal join.
    wakeups: list[dict[str, Any]] = []
    for event in conn.execute("SELECT id, session_id, created_at, kind, payload, payload_codec FROM agent_events WHERE kind = 'user_prompt_submit'"):
        try:
            raw = event["payload"]
            if event["payload_codec"] == "deflate":
                import zlib
                raw = zlib.decompress(raw, -zlib.MAX_WBITS)
            body = json.loads(raw)
        except (ValueError, TypeError, __import__("zlib").error):
            continue
        if body.get("source") != "daemon":
            continue
        candidates = [value for value in grouped.get(event["session_id"], []) if stamp(value["timestamp"]) and stamp(value["timestamp"]) >= stamp(event["created_at"])]
        next_value = min(candidates, key=lambda value: stamp(value["timestamp"])) if candidates else None
        prompt = body.get("text") or body.get("prompt") or ""
        wakeups.append({"event_id": event["id"], "session_id": event["session_id"] or "unattributed", "delivered_at": event["created_at"], "prompt_chars": len(prompt) if isinstance(prompt, str) else 0, "delivery_cause": "daemon_prompt_text_unclassified", "next_request_id": next_value["request_id"] if next_value else "", "next_request_delay_secs": round((stamp(next_value["timestamp"]) - stamp(event["created_at"])).total_seconds()) if next_value and stamp(event["created_at"]) else "", "outcome_join": "session/task metadata only" if event["session_id"] in session_by_id else "unmapped_session", "limitation": "observed event-created-at to transcript timestamp; neither request-start nor active execution; no delivery ID or causal linkage"})
    candidate_counts = collections.Counter((row["session_id"], row["next_request_id"]) for row in wakeups if row["next_request_id"])
    for row in wakeups:
        count = candidate_counts.get((row["session_id"], row["next_request_id"]), 0)
        row["candidate_count"] = count
        row["match_kind"] = "ambiguous_competing_delivery" if count > 1 else "unique_temporal_candidate" if count else "no_later_mapped_request"

    # One row per tool call/result ID.  Calls with a later result update the
    # result length without adding a second call row.
    merged_tools: dict[tuple[str, str], dict[str, Any]] = {}
    for row in tool_rows:
        key = (row["family"], row["session_id"], row["tool_call_id"])
        prior = merged_tools.setdefault(key, dict(row))
        prior["result_chars"] = max(prior["result_chars"], row["result_chars"])
        if prior["tool_name"] == "unknown":
            prior["tool_name"] = row["tool_name"]
    # Count native call identities after duplicate transcript blocks merge.
    tool_count_by_request = collections.Counter((tool["family"], tool["session_id"], tool["request_id"]) for tool in merged_tools.values())
    for request in request_rows:
        request["tool_calls"] = tool_count_by_request[(request["family"], request["session_id"], request["request_id"])]
    mapped_request_keys = {(row["family"], row["session_id"], row["request_id"]) for row in request_rows}
    tool_values = sorted((row for row in merged_tools.values() if (row["family"], row["session_id"], row["request_id"]) in mapped_request_keys or row["request_association"] != "native_assistant_content"), key=lambda value: (value["family"], value["request_id"], value["tool_call_id"]))

    baseline_snapshot = Path(__file__).with_name("snapshot.json")
    baseline_cutoff = json.loads(baseline_snapshot.read_text())["cutoff"] if baseline_snapshot.exists() else ""
    audit_goal_ids = {args.audit_goal_id}
    cohorts: dict[str, dict[str, Any]] = {}
    for cohort in ("shared_frozen_historical_interval", "shared_frozen_post_cutoff", "not_in_frozen_session_ids", "newer_evidence", "this_goal_overhead"):
        cohorts[cohort] = {"cohort": cohort, "requests": 0, "sessions": set(), **usage()}
    for row in request_rows:
        session = session_by_id[row["session_id"]]
        cohort = "this_goal_overhead" if session.get("goal_id") in audit_goal_ids else "shared_frozen_post_cutoff" if row["session_id"] in baseline_session_ids and row["timestamp"] > baseline_cutoff else "shared_frozen_historical_interval" if row["session_id"] in baseline_session_ids else "newer_evidence" if row["timestamp"] > baseline_cutoff else "not_in_frozen_session_ids"
        item = cohorts[cohort]
        item["requests"] += 1
        item["sessions"].add(row["session_id"])
        add(item, row)
    cohort_rows = []
    for item in cohorts.values():
        item["sessions"] = len(item["sessions"])
        item["uncached_input_tokens"] = uncached(item)
        cohort_rows.append(item)
    handoffs = []
    for session in sessions:
        if not session.get("switched_from"):
            continue
        event = conn.execute("SELECT id, payload, payload_codec FROM agent_events WHERE session_id=? AND kind='user_prompt_submit' ORDER BY created_at LIMIT 1", (session["id"],)).fetchone()
        text = ""
        if event:
            try:
                raw = event["payload"]
                if event["payload_codec"] == "deflate":
                    import zlib
                    raw = zlib.decompress(raw, -zlib.MAX_WBITS)
                text = json.loads(raw).get("text") or json.loads(raw).get("prompt") or ""
            except (ValueError, TypeError, __import__("zlib").error):
                text = ""
        task = tasks.get(session.get("task_id"), {})
        handoffs.append({"predecessor_session_id": session["switched_from"], "successor_session_id": session["id"], "goal_id": session.get("goal_id") or "", "task_id": session.get("task_id") or "", "delivered_prompt_event_id": event["id"] if event else "", "delivered_prompt_chars": len(text), "handoff_marker": int("This is the history of the session" in text), "renderer_revision": "uncertain_deployment_revision", "candidate_selection": "delivered_prompt_retained" if text else "no_successor_prompt", "supported_consequence": task.get("status") or "no_task", "reason": "delivered body is retained; historical renderer selection and deployment revision are not"})

    request_total = usage()
    for row in request_rows:
        add(request_total, row)
    usage_by_session: dict[str, dict[str, int]] = collections.defaultdict(usage)
    for row in rows(conn, "SELECT * FROM session_usage"):
        add(usage_by_session[row["session_id"]], row)
    reconciliation_rows = []
    for sid, session in session_by_id.items():
        counter_present = sid in usage_by_session
        counter, observed = usage_by_session.get(sid, usage()), usage()
        for row in grouped.get(sid, []): add(observed, row)
        state = "request_partial" if not grouped.get(sid) else "matched" if observed == counter else "reset_or_multi_launch_or_scope_difference"
        reconciliation_rows.append({"session_id": sid, "goal_id": session.get("goal_id") or "", "task_id": session.get("task_id") or "", "launch_id": session.get("launch_id") or "", "session_counter_present": int(counter_present), "state": "missing_counter" if not counter_present else state, "request_input_tokens": observed["input_tokens"], "request_cached_input_tokens": observed["cached_input_tokens"], "request_output_tokens": observed["output_tokens"], "counter_input_tokens": counter["input_tokens"], "counter_cached_input_tokens": counter["cached_input_tokens"], "counter_output_tokens": counter["output_tokens"], "input_residual": counter["input_tokens"] - observed["input_tokens"], "cached_input_residual": counter["cached_input_tokens"] - observed["cached_input_tokens"], "output_residual": counter["output_tokens"] - observed["output_tokens"]})
    shared_reconciliation = []
    excluded_by_session = collections.Counter(row["session_id"] for row in excluded_requests)
    source_count_by_session = collections.Counter(row["session_id"] for row in rows(conn, "SELECT session_id, source FROM session_usage"))
    starts_by_session = collections.Counter(row[0] for row in conn.execute("SELECT session_id FROM agent_events WHERE kind='session_start'"))
    sessions_by_task = collections.Counter(session.get("task_id") for session in sessions if session.get("task_id"))
    for sid in sorted(baseline_session_ids):
        historical = usage()
        post_cutoff = usage()
        for row in grouped.get(sid, []):
            add(post_cutoff if row["timestamp"] > baseline_cutoff else historical, row)
        frozen_counter = baseline_usage_by_session.get(sid, usage())
        task_session_count = sessions_by_task.get(session_by_id.get(sid, {}).get("task_id"), 0)
        state = "no_retained_historical_request" if not grouped.get(sid) else "matched_frozen_counter" if historical == frozen_counter else "unresolved_residual"
        evidence_flags = "|".join(flag for flag, present in (("excluded_ambiguous_request_usage", bool(excluded_by_session[sid])), ("multiple_counter_sources", source_count_by_session[sid] > 1), ("restarted_session", starts_by_session[sid] > 1), ("multi_session_task_scope", task_session_count > 1)) if present) or "none"
        shared_reconciliation.append({"session_id": sid, "historical_request_input_tokens": historical["input_tokens"], "historical_request_cached_input_tokens": historical["cached_input_tokens"], "historical_request_output_tokens": historical["output_tokens"], "frozen_counter_input_tokens": frozen_counter["input_tokens"], "frozen_counter_cached_input_tokens": frozen_counter["cached_input_tokens"], "frozen_counter_output_tokens": frozen_counter["output_tokens"], "post_cutoff_request_input_tokens": post_cutoff["input_tokens"], "post_cutoff_request_cached_input_tokens": post_cutoff["cached_input_tokens"], "post_cutoff_request_output_tokens": post_cutoff["output_tokens"], "excluded_request_identities": excluded_by_session[sid], "counter_sources": source_count_by_session[sid], "session_starts": starts_by_session[sid], "task_session_count": task_session_count, "evidence_flags": evidence_flags, "input_residual": frozen_counter["input_tokens"] - historical["input_tokens"], "cached_input_residual": frozen_counter["cached_input_tokens"] - historical["cached_input_tokens"], "output_residual": frozen_counter["output_tokens"] - historical["output_tokens"], "state": state})
    frozen = json.loads(Path(__file__).with_name("reconciliation.json").read_text())
    current_total = usage()
    for value in usage_by_session.values(): add(current_total, value)
    reconciliation = {"request_usage_rule": "sum one mapped retained native response/message ID per format; exclude conflicts and missing usage", "request_usage": {**request_total, "uncached_input_tokens": uncached(request_total)}, "current_snapshot_session_usage_not_baseline": {**current_total, "uncached_input_tokens": uncached(current_total)}, "frozen_baseline_canonical_usage": frozen["canonical_usage"], "frozen_baseline_rule": frozen["canonical_rule"], "frozen_baseline_deleted_session_fallbacks": frozen["fallback_usage"], "shared_historical_comparison": "shared-cohort-reconciliation.csv compares retained requests at or before frozen cutoff to frozen per-session counters; post-cutoff requests remain separate", "deduplicated_claude_request_records": claude_counts["duplicate_request_records"], "conflicting_duplicate_request_ids_excluded": len(conflicts), "excluded_request_records": len(excluded_requests), "unmapped_request_records_excluded": unmapped_request_records}
    snapshot = {"analyzer_version": VERSION, "analyzer_sha256": digest(Path(__file__)), "revision": args.revision, "source": {"label": args.source_label, "external_snapshot_location": args.snapshot_location, "sha256": digest(source)}, "cutoff": cutoff, "baseline_database": {"available": bool(baseline_session_ids), "session_ids": len(baseline_session_ids), "sha256": baseline_hash}, "transcript_manifest_sha256": manifest.hexdigest(), "coverage": {"formats": formats, "claude_parser": dict(claude_counts), "codex_inventory": dict(codex_counts), "mapped_request_sessions": len(grouped), "partial_sessions": sum(session["status"] not in {"exited", "failed"} for session in sessions)}, "separation": {"baseline": "frozen artifacts under bench/token-usage excluding followup-*", "shared_cohort": "native agent_sessions.id intersection with supplied frozen database", "goal_overhead": args.audit_goal_id}}

    write_csv(out / "format-inventory.csv", formats, ["family", "files", "mapped_sessions", "request_history", "reason"])
    write_csv(out / "followup-transcript-manifest.csv", manifest_rows, ["family", "source_id", "retained_content_sha256", "parser", "mapped_requests", "unmapped_requests"])
    write_csv(out / "request-usage.csv", request_rows, ["family", "session_id", "vendor_session_id", "request_id", "alias_ids", "turn_id", "response_id", "timestamp", "model", "effort", "tool_calls", "usage_record", "usage_status", "deduplication", "input_tokens", "cached_input_tokens", "output_tokens"])
    write_csv(out / "excluded-requests.csv", excluded_requests, ["family", "session_id", "vendor_session_id", "request_id", "alias_ids", "reason"])
    write_csv(out / "session-timeline.csv", timeline, ["session_id", "goal_id", "task_id", "seat", "workflow_column", "model", "effort", "launch_id", "task_outcome", "session_status", "partial", "requests", "first_request_at", "last_request_at", "observed_request_span_secs", "interval_definition", "input_tokens", "cached_input_tokens", "output_tokens", "uncached_input_tokens"])
    write_csv(out / "tool-output.csv", tool_values, ["family", "session_id", "request_id", "tool_call_id", "tool_name", "turn_id", "result_chars", "result_representation", "timestamp", "association", "request_association"])
    write_csv(out / "wakeup-analysis.csv", wakeups, ["event_id", "session_id", "delivered_at", "prompt_chars", "delivery_cause", "next_request_id", "next_request_delay_secs", "candidate_count", "match_kind", "outcome_join", "limitation"])
    write_csv(out / "cohort-comparison.csv", cohort_rows, ["cohort", "requests", "sessions", "input_tokens", "cached_input_tokens", "output_tokens", "uncached_input_tokens"])
    write_csv(out / "handoff-analysis.csv", handoffs, ["predecessor_session_id", "successor_session_id", "goal_id", "task_id", "delivered_prompt_event_id", "delivered_prompt_chars", "handoff_marker", "renderer_revision", "candidate_selection", "supported_consequence", "reason"])
    write_csv(out / "request-reconciliation.csv", reconciliation_rows, ["session_id", "goal_id", "task_id", "launch_id", "session_counter_present", "state", "request_input_tokens", "request_cached_input_tokens", "request_output_tokens", "counter_input_tokens", "counter_cached_input_tokens", "counter_output_tokens", "input_residual", "cached_input_residual", "output_residual"])
    write_csv(out / "shared-cohort-reconciliation.csv", shared_reconciliation, ["session_id", "historical_request_input_tokens", "historical_request_cached_input_tokens", "historical_request_output_tokens", "frozen_counter_input_tokens", "frozen_counter_cached_input_tokens", "frozen_counter_output_tokens", "post_cutoff_request_input_tokens", "post_cutoff_request_cached_input_tokens", "post_cutoff_request_output_tokens", "excluded_request_identities", "counter_sources", "session_starts", "task_session_count", "evidence_flags", "input_residual", "cached_input_residual", "output_residual", "state"])
    (out / "followup-snapshot.json").write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
    (out / "followup-reconciliation.json").write_text(json.dumps(reconciliation, indent=2, sort_keys=True) + "\n")
    (out / "FOLLOWUP-REPORT.md").write_text(f"""# Request-level transcript follow-up

## Question and answer

**Question:** What avoidable token use and elapsed time can saved transcripts establish before new production instrumentation?

Saved Claude transcripts establish per-request usage for mapped sessions and native request, response, turn, and tool IDs. Saved Codex `token_usage_record` rows establish one observed `usage` field per native response ID; cumulative counters and overlapping turn/thread totals remain excluded. They establish observed request spans and tool-result sizes. They do not establish billed prompt composition, active execution time, delivery causes, rendered handoff selection, or causal savings. The baseline remains the canonical cumulative total.

## Reproducible evidence

Cutoff: `{cutoff}`. Source: `{args.source_label}` at `{args.snapshot_location}`. Source SHA-256: `{digest(source)}`. Source revision: `{args.revision}`. Analyzer SHA-256: `{digest(Path(__file__))}`. `followup-transcript-manifest.csv` has a per-source retained-content hash; it does not hash records after the cutoff.

The follow-up found {len(request_rows):,} retained request records, all mapped through native session IDs. It discarded {claude_counts['duplicate_request_records']:,} repeated Claude blocks and excluded {len(conflicts):,} ambiguous-usage request identities ({len(excluded_requests):,} identities including missing usage) in `excluded-requests.csv`. The CSV artifacts retain no raw transcript text.

## Cohorts, accounting, and timing

`cohort-comparison.csv` uses the supplied frozen SQLite snapshot to intersect native `agent_sessions.id` values. It separates those shared identities, new sessions, and this goal by explicit goal ID. These transcript cohorts are not a before/after experiment.

Request input includes cached input. Uncached input equals input minus cached input. `request-reconciliation.csv` compares current retained requests with current session counters. `shared-cohort-reconciliation.csv` compares requests at or before the frozen cutoff with frozen per-session counters, and records post-cutoff requests in shared sessions separately. The baseline hash is recorded in `followup-snapshot.json`. Residuals label missing retained requests separately; all other residuals remain unresolved where counters, resets, and launch scope do not identify a unique cause. `followup-reconciliation.json` names the frozen baseline canonical total and fallback rule separately from the current snapshot; neither total replaces the other.

`session-timeline.csv` measures only first-to-last timestamped model request in a session. It never sums per-request intervals, so parallel requests and waits do not inflate elapsed time. It does not measure model inference, tool runtime, queueing, or human wait.

## Wakeups, handoffs, and tool results

`wakeup-analysis.csv` joins a daemon prompt to subsequent mapped requests in the same session. It labels shared first-later-request matches as `ambiguous_competing_delivery`; a candidate is temporal, not causal. Its delay is event creation to recorded transcript timestamp, not request start or active execution. Events retain no vendor request ID, delivery cause, or outcome correlation. Session/task status and ending facts remain lifecycle outcomes.

`handoff-analysis.csv` inventories each native `switched_from` pair and measures the first saved successor prompt. It marks retained delivered handoff text separately from three missing successor-prompt candidates. The historical renderer revision and pre-budget selected-entry set remain unavailable, so delivered length does not prove which history caused an outcome.

`tool-output.csv` links Claude and Codex tool calls/results by native tool-call ID. It separates mapped requests, unmatched calls, and unavailable request associations. Result characters describe retained text only; arrays of text blocks are measured and unsupported shapes are labelled. They are not billed-token measurements or proof of later prompt inclusion.

## Manual record inspection

`manual-inspection.json` selects the three largest mapped-session input totals and one median mapped reviewer session. Each selected daemon event has a later native request and a finished task outcome; one major session has a correction-marker delivery among thirteen. Those are supported sequence facts, not evidence that the delivery caused the work. The first major transcript has 17 native error results. Two sampled error results have later request IDs and observed 2,408 and 1,737 millisecond record intervals. The first rejected a ScheduleWakeup call for a missing prompt; its subsequent assistant content chose automatic notification. It also has 69 exact repeated-command groups across 1,030 Bash calls. These facts establish retry candidates, but not whether a command was necessary or a correction succeeded.

The handoff sample includes the largest retained successor prompt (247,208 characters, cancelled) and a finished ordinary successor (50,949 characters). Both retain the history marker. Stored renderer source provides `handoff_text`, `render_entry`, and `apply_budget` rules. The historical deployed revision and selected entries are still absent, so the body cannot reconstruct the original selection.

The tool inventory has repeated and large retained operations. `manual-inspection.json` records two mapped `Read` results of 85,089 and 78,622 characters and mapped `Bash` groups of 1,030 and 617 calls, with native record IDs. It also records the lifecycle-status join limit for failure evidence. The tool table retains unmatched results and representation limits. It does not retain a request prompt-component identifier, so it cannot establish whether a result was reread, included in a later prompt, or caused a correction. Stored result characters are not token savings.

## Recommendation

Keep TU-03 unchanged. Rank TU-01 first: its bounded scope is delivery ID, cause, receiving request/turn, and lifecycle outcome; competing temporal matches are evidence, while many unique candidates and completed tasks are counter-evidence. Rank TU-02 second: its scope is deployed renderer revision, candidates, selected entries, and prompt inclusion; retained handoff bodies and renderer source are counter-evidence to a claim that all handoffs are opaque. Rank TU-04 third: its scope is request prompt components and tool-result inclusion; repeated commands and native error results are evidence, while their later requests cannot establish rereading or savings. Confidence is moderate that each gap exists and low that reducing it saves tokens or time. Each comparison must keep checks and review columns unchanged and compare request categories, completion lead time, retry/review outcomes, and failure-diagnosis quality against matched task types.

The strongest counter-evidence is the existing review history: repeated work may be a distinct revision or correction. No saved transcript demonstrates token savings or faster completion from suppressing a wakeup, reducing a handoff, or truncating a tool result. Preserve required checks and review columns. Stop here for the user decision before any production change.
""")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
