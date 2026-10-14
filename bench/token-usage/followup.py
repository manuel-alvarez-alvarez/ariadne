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


VERSION = "1"
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


def string_size(value: Any) -> int:
    return len(value) if isinstance(value, str) else 0


def claude_requests(path: Path, internal: dict[str, dict[str, Any]], cutoff: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, int]]:
    """Return unique vendor requests and tool result records from one Claude file."""
    requests: list[dict[str, Any]] = []
    tools: list[dict[str, Any]] = []
    seen: set[str] = set()
    session_id: str | None = None
    last_request: str | None = None
    counts = collections.Counter()
    try:
        file = path.open(encoding="utf-8", errors="replace")
    except OSError:
        return requests, tools, counts
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
                if record.get("type") == "user" and last_request:
                    for part in (record.get("message") or {}).get("content") or []:
                        if isinstance(part, dict) and part.get("type") == "tool_result":
                            tools.append({"request_id": last_request, "tool_call_id": str(part.get("tool_use_id") or "missing"), "tool_name": "unknown", "result_chars": string_size(part.get("content")), "timestamp": record.get("timestamp") or ""})
                continue
            message = record["message"]
            vendor_id = record.get("requestId") or message.get("id")
            if not isinstance(vendor_id, str):
                counts["assistant_without_request_id"] += 1
                continue
            # Claude can repeat content blocks and usage for one message.
            if vendor_id in seen:
                counts["duplicate_request_records"] += 1
                continue
            seen.add(vendor_id)
            raw = message.get("usage") or {}
            if not isinstance(raw, dict):
                raw = {}
            direct_input = int(raw.get("input_tokens") or 0)
            cache_create = int(raw.get("cache_creation_input_tokens") or 0)
            cached = int(raw.get("cache_read_input_tokens") or 0)
            item = {"input_tokens": direct_input + cache_create + cached, "cached_input_tokens": cached, "output_tokens": int(raw.get("output_tokens") or 0)}
            tool_calls = [part for part in message.get("content") or [] if isinstance(part, dict) and part.get("type") == "tool_use"]
            requests.append({"family": "claude", "session_id": session_id or "unmapped", "vendor_session_id": record.get("sessionId") or "missing", "request_id": vendor_id, "turn_id": record.get("uuid") or "missing", "response_id": message.get("id") or "missing", "timestamp": record.get("timestamp") or "", "model": message.get("model") or "unknown", "effort": record.get("effort") or record.get("perTurnEffort") or "unknown", "tool_calls": len(tool_calls), "usage_record": int(bool(raw)), "deduplication": "request_id", **item})
            last_request = vendor_id
            for part in tool_calls:
                tools.append({"request_id": vendor_id, "tool_call_id": str(part.get("id") or "missing"), "tool_name": part.get("name") or "unknown", "result_chars": 0, "timestamp": record.get("timestamp") or ""})
    return requests, tools, counts


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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--codex-transcripts", type=Path, default=Path("/Users/malvarez/.codex/sessions"))
    parser.add_argument("--claude-transcripts", type=Path, default=Path("/Users/malvarez/.claude/projects"))
    parser.add_argument("--source-label", required=True)
    args = parser.parse_args()
    source = args.database.resolve()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    sessions = rows(conn, "SELECT * FROM agent_sessions")
    session_by_id = {row["id"]: row for row in sessions}
    internal = {row["internal_session_id"]: row for row in sessions if row.get("internal_session_id")}
    cutoff = max(value for value in (conn.execute("SELECT MAX(created_at) FROM agent_events").fetchone()[0], conn.execute("SELECT MAX(updated_at) FROM session_usage").fetchone()[0], conn.execute("SELECT MAX(created_at) FROM stat_facts").fetchone()[0]) if value)

    formats: list[dict[str, Any]] = []
    request_rows: list[dict[str, Any]] = []
    tool_rows: list[dict[str, Any]] = []
    manifest = hashlib.sha256()
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
        formats.append({"family": "claude", "files": len(files), "mapped_sessions": len({row["session_id"] for row in request_rows if row["session_id"] != "unmapped"}), "request_history": "supported", "reason": "assistant requestId/message.id and per-message usage are retained"})
    else:
        formats.append({"family": "claude", "files": 0, "mapped_sessions": 0, "request_history": "unavailable", "reason": "transcript root does not exist"})

    codex_counts = collections.Counter()
    if args.codex_transcripts.exists():
        files = sorted(args.codex_transcripts.rglob("rollout-*.jsonl"))
        for path in files:
            found, counts = codex_requests(path, internal, cutoff)
            request_rows.extend(found)
            codex_counts.update(counts)
            if found:
                manifest.update(("codex:" + digest(path) + "\n").encode())
        formats.append({"family": "codex", "files": len(files), "mapped_sessions": len({row["session_id"] for row in request_rows if row["family"] == "codex" and row["session_id"] != "unmapped"}), "request_history": "supported_with_field_semantics_limit", "reason": "token_usage_record has a native response_id and non-overlapping usage field; turn and thread totals overlap"})
    else:
        formats.append({"family": "codex", "files": 0, "mapped_sessions": 0, "request_history": "unavailable", "reason": "transcript root does not exist"})
    formats.append({"family": "ariadne_sqlite", "files": 1, "mapped_sessions": len(sessions), "request_history": "unavailable", "reason": "session_usage is cumulative by session/source; daemon events lack per-request token attribution"})
    opencode = Path("/Users/malvarez/.local/share/opencode/opencode.db")
    formats.append({"family": "opencode", "files": int(opencode.exists()), "mapped_sessions": 0, "request_history": "unavailable", "reason": "token-bearing records exist, but no native Ariadne session join or validated deduplication rule is retained"})

    # A response/message ID is authoritative within its format. Exact copies
    # collapse; conflicting copies are excluded rather than silently chosen.
    deduped: dict[tuple[str, str], dict[str, Any]] = {}
    conflicts: set[tuple[str, str]] = set()
    for row in request_rows:
        key = (row["family"], row["request_id"])
        prior = deduped.get(key)
        if prior is None:
            deduped[key] = row
        elif any(prior[field] != row[field] for field in TOKENS):
            conflicts.add(key)
    request_rows = [row for key, row in deduped.items() if key not in conflicts]
    unmapped_request_records = sum(row["session_id"] == "unmapped" for row in request_rows)
    request_rows = [row for row in request_rows if row["session_id"] != "unmapped"]
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
        timeline.append({"session_id": sid, "goal_id": session.get("goal_id") or "", "task_id": session.get("task_id") or "", "seat": session.get("seat") or "", "model": session.get("model") or "", "requests": len(values), "first_request_at": first.isoformat().replace("+00:00", "Z") if first else "", "last_request_at": last.isoformat().replace("+00:00", "Z") if last else "", "observed_request_span_secs": round((last - first).total_seconds()) if first and last else "", "interval_definition": "first-to-last recorded model request; parallel requests are not added", **total, "uncached_input_tokens": uncached(total)})

    # Associate a daemon delivery only when the retained session ID is native.
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
        wakeups.append({"event_id": event["id"], "session_id": event["session_id"] or "unattributed", "delivered_at": event["created_at"], "delivery_cause": "daemon_prompt_text_unclassified", "next_request_id": next_value["request_id"] if next_value else "", "next_request_delay_secs": round((stamp(next_value["timestamp"]) - stamp(event["created_at"])).total_seconds()) if next_value and stamp(event["created_at"]) else "", "outcome_join": "session/task metadata only" if event["session_id"] in session_by_id else "unmapped_session", "limitation": "no delivery ID, cause, or prompt-to-request linkage is retained"})

    # One row per tool call/result ID.  Calls with a later result update the
    # result length without adding a second call row.
    merged_tools: dict[tuple[str, str], dict[str, Any]] = {}
    for row in tool_rows:
        key = (row["request_id"], row["tool_call_id"])
        prior = merged_tools.setdefault(key, dict(row))
        prior["result_chars"] = max(prior["result_chars"], row["result_chars"])
        if prior["tool_name"] == "unknown":
            prior["tool_name"] = row["tool_name"]
    tool_values = sorted(merged_tools.values(), key=lambda value: (value["request_id"], value["tool_call_id"]))

    baseline_snapshot = Path(__file__).with_name("snapshot.json")
    baseline_cutoff = json.loads(baseline_snapshot.read_text())["cutoff"] if baseline_snapshot.exists() else ""
    audit_goal_ids = {row["id"] for row in rows(conn, "SELECT id FROM goals WHERE title LIKE '%request-level%' OR title LIKE '%Explain token usage%'")}
    cohorts: dict[str, dict[str, Any]] = {}
    for cohort in ("shared_baseline_cohort", "newer_evidence", "this_goal_overhead"):
        cohorts[cohort] = {"cohort": cohort, "requests": 0, "sessions": set(), **usage()}
    for row in request_rows:
        session = session_by_id[row["session_id"]]
        cohort = "this_goal_overhead" if session.get("goal_id") in audit_goal_ids else "shared_baseline_cohort" if row["timestamp"] <= baseline_cutoff else "newer_evidence"
        item = cohorts[cohort]
        item["requests"] += 1
        item["sessions"].add(row["session_id"])
        add(item, row)
    cohort_rows = []
    for item in cohorts.values():
        item["sessions"] = len(item["sessions"])
        item["uncached_input_tokens"] = uncached(item)
        cohort_rows.append(item)
    handoffs = [{"coverage": "unavailable", "reason": "historical renderer revision, candidate set, selected entries, and prompt inclusion are not retained together", "supported_consequence": "none"}]

    request_total = usage()
    for row in request_rows:
        add(request_total, row)
    baseline_total = usage()
    for row in rows(conn, "SELECT input_tokens, cached_input_tokens, output_tokens FROM session_usage"):
        add(baseline_total, row)
    reconciliation = {"request_usage_rule": "sum one mapped retained native response/message ID per format; exclude conflicting duplicate IDs", "request_usage": {**request_total, "uncached_input_tokens": uncached(request_total)}, "canonical_session_usage": {**baseline_total, "uncached_input_tokens": uncached(baseline_total)}, "comparison": "not additive: transcript request coverage is partial and vendor accounting may differ", "deduplicated_claude_request_records": claude_counts["duplicate_request_records"], "conflicting_duplicate_request_ids_excluded": len(conflicts), "codex_request_coverage": "token_usage_record usage field only; cumulative token_count and turn/thread totals excluded", "unmapped_request_records_excluded": unmapped_request_records}
    snapshot = {"analyzer_version": VERSION, "revision": args.revision, "source": {"label": args.source_label, "sha256": digest(source)}, "cutoff": cutoff, "transcript_manifest_sha256": manifest.hexdigest(), "coverage": {"formats": formats, "claude_parser": dict(claude_counts), "codex_inventory": dict(codex_counts), "mapped_request_sessions": len(grouped), "partial_sessions": sum(session["status"] not in {"exited", "failed"} for session in sessions)}, "separation": {"baseline": "frozen artifacts under bench/token-usage excluding followup-*", "newer_evidence": "this snapshot only", "goal_overhead": "the current goal is retained only through its own session identifiers and is excluded from historical claims"}}

    write_csv(out / "format-inventory.csv", formats, ["family", "files", "mapped_sessions", "request_history", "reason"])
    write_csv(out / "request-usage.csv", request_rows, ["family", "session_id", "vendor_session_id", "request_id", "turn_id", "response_id", "timestamp", "model", "effort", "tool_calls", "usage_record", "deduplication", "input_tokens", "cached_input_tokens", "output_tokens"])
    write_csv(out / "session-timeline.csv", timeline, ["session_id", "goal_id", "task_id", "seat", "model", "requests", "first_request_at", "last_request_at", "observed_request_span_secs", "interval_definition", "input_tokens", "cached_input_tokens", "output_tokens", "uncached_input_tokens"])
    write_csv(out / "tool-output.csv", tool_values, ["request_id", "tool_call_id", "tool_name", "result_chars", "timestamp"])
    write_csv(out / "wakeup-analysis.csv", wakeups, ["event_id", "session_id", "delivered_at", "delivery_cause", "next_request_id", "next_request_delay_secs", "outcome_join", "limitation"])
    write_csv(out / "cohort-comparison.csv", cohort_rows, ["cohort", "requests", "sessions", "input_tokens", "cached_input_tokens", "output_tokens", "uncached_input_tokens"])
    write_csv(out / "handoff-analysis.csv", handoffs, ["coverage", "reason", "supported_consequence"])
    (out / "followup-snapshot.json").write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
    (out / "followup-reconciliation.json").write_text(json.dumps(reconciliation, indent=2, sort_keys=True) + "\n")
    (out / "FOLLOWUP-REPORT.md").write_text(f"""# Request-level transcript follow-up

## Question and answer

**Question:** What avoidable token use and elapsed time can saved transcripts establish before new production instrumentation?

Saved Claude transcripts establish per-request usage for mapped sessions and native request, response, turn, and tool IDs. Saved Codex `token_usage_record` rows establish one observed `usage` field per native response ID; cumulative counters and overlapping turn/thread totals remain excluded. They establish observed request spans and tool-result sizes. They do not establish billed prompt composition, active execution time, delivery causes, rendered handoff selection, or causal savings. The baseline remains the canonical cumulative total.

## Reproducible evidence

Cutoff: `{cutoff}`. Source: `{args.source_label}`. Source SHA-256: `{digest(source)}`. Analyzer revision: `{args.revision}`. Transcript manifest: `{manifest.hexdigest()}`. See `followup-snapshot.json`, `format-inventory.csv`, and `followup-reconciliation.json`.

The follow-up found {len(request_rows):,} unique request records, of which {sum(row['session_id'] != 'unmapped' for row in request_rows):,} map through native session IDs. It discarded {claude_counts['duplicate_request_records']:,} repeated Claude records and excluded {len(conflicts):,} conflicting duplicate native IDs. The CSV artifacts retain no raw transcript text.

## Cohorts, accounting, and timing

`cohort-comparison.csv` separates mapped requests shared with the frozen baseline cutoff from newer evidence and this goal's overhead. These are transcript cohorts, not a before/after experiment. They do not show a saving.

Request input includes cached input. Uncached input equals input minus cached input. `followup-reconciliation.json` compares the request sum with the baseline cumulative session counter without adding them. The difference is expected: formats and mappings are incomplete, while vendor and daemon records can have different scopes.

`session-timeline.csv` measures only first-to-last timestamped model request in a session. It never sums per-request intervals, so parallel requests and waits do not inflate elapsed time. It does not measure model inference, tool runtime, queueing, or human wait.

## Wakeups, handoffs, and tool results

`wakeup-analysis.csv` joins a daemon prompt to the first later retained request only when both share the native Ariadne session ID. This is temporal sequencing, not delivery causation. The database has no delivery ID, cause, outcome link, or rendered-prompt membership, so corrections cannot be separated reliably from repeated deliveries.

Saved handoff text can be inspected locally, but historical rendering revision, candidate set, selection, and prompt inclusion are not retained as structured records. This study therefore does not reconstruct omitted handoff context or claim that later rereading came from a handoff.

`tool-output.csv` links Claude tool calls and results by native tool-call ID. Result characters describe retained text only. They are not billed-token measurements and do not establish that a result was later included in context.

## Recommendation

Keep TU-03 unchanged. Rank TU-01 first: add delivery IDs, causes, and resulting request/outcome links because existing records cannot resolve repeated delivery versus correction. Rank TU-02 second: record handoff candidate and selected lengths, renderer revision, and prompt inclusion because saved text cannot reconstruct selection. Rank TU-04 third: record request prompt components and tool-result inclusion because result size alone does not show token contribution. These are evidence-gap instruments, not production optimizations.

The strongest counter-evidence is the existing review history: repeated work may be a distinct revision or correction. No saved transcript demonstrates token savings or faster completion from suppressing a wakeup, reducing a handoff, or truncating a tool result. Preserve required checks and review columns. Stop here for the user decision before any production change.
""")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
