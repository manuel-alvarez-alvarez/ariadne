#!/usr/bin/env python3
"""Produce privacy-preserving historical token-use aggregates from Ariadne SQLite.

The input database is read-only.  It and raw event payloads must stay outside
Git; this program writes only totals, identifiers, and derived trace metrics.
Run ``python3 bench/token-usage/analyze.py --help`` for the contract.
"""

from __future__ import annotations

import argparse
import collections
import csv
import datetime as dt
import hashlib
import json
import os
import sqlite3
import statistics
import sys
import zlib
from pathlib import Path
from typing import Any, Iterable


VERSION = "1"
TOKEN_FIELDS = ("input_tokens", "cached_input_tokens", "output_tokens")
NOW = dt.datetime.now(dt.timezone.utc)


def iso(value: str | None) -> dt.datetime | None:
    if not value:
        return None
    try:
        return dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def seconds(a: str | None, b: str | None) -> float | None:
    start, end = iso(a), iso(b)
    if not start or not end:
        return None
    return max(0.0, (end - start).total_seconds())


def read_rows(conn: sqlite3.Connection, sql: str, args: Iterable[Any] = ()) -> list[dict[str, Any]]:
    return [dict(row) for row in conn.execute(sql, tuple(args))]


def table_exists(conn: sqlite3.Connection, name: str) -> bool:
    return conn.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", (name,)).fetchone() is not None


def add_usage(target: dict[str, int], row: dict[str, Any]) -> None:
    for field in TOKEN_FIELDS:
        target[field] += max(0, int(row.get(field) or 0))


def usage_dict() -> dict[str, int]:
    return {field: 0 for field in TOKEN_FIELDS}


def uncached(row: dict[str, Any]) -> int:
    return max(0, row["input_tokens"] - row["cached_input_tokens"])


def write_csv(path: Path, rows: list[dict[str, Any]], columns: list[str]) -> None:
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=columns, extrasaction="ignore", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def payload(raw: bytes, codec: str) -> dict[str, Any] | None:
    try:
        decoded = zlib.decompress(raw, -zlib.MAX_WBITS) if codec == "deflate" else raw
        value = json.loads(decoded)
        return value if isinstance(value, dict) else None
    except (UnicodeDecodeError, ValueError, zlib.error):
        return None


def text_at(value: Any, *paths: tuple[str, ...]) -> str | None:
    for path in paths:
        here = value
        for key in path:
            if not isinstance(here, dict):
                here = None
                break
            here = here.get(key)
        if isinstance(here, str):
            return here
    return None


def render_markdown(snapshot: dict[str, Any], recommendations: list[dict[str, Any]]) -> str:
    u = snapshot["canonical_usage"]
    inv = snapshot["inventory"]
    timing = snapshot["timing"]
    coverage = snapshot["coverage"]
    traces = snapshot["trace_selection"]
    candidate_lines = "\n".join(
        f"### {r['id']} — {r['title']}\n\n"
        f"**Evidence.** {r['evidence']}\n\n"
        f"**Bounded scope.** {r['scope']}\n\n"
        f"**Expected benefit.** {r['benefit']} Confidence: {r['confidence']}.\n\n"
        f"**Quality guard and measurement.** {r['measurement']}\n\n"
        f"**Counter-evidence.** {r['counter_evidence']}\n"
        for r in recommendations
    )
    return f"""# Historical token-use baseline and recommendations

## Decision checkpoint

This is a historical audit, not authorization to alter production prompts,
skills, model assignments, or workflow gates. The user should select stable
recommendation identifiers below before implementation work is created.

## Question and answer

**Question:** Where did this Ariadne installation spend avoidable tokens and
elapsed time, and which bounded improvements should be tested first?

The strongest measured opportunity is to reduce repeated wake-up prompts and
their copied task context (`TU-01`), then make transcript-handoff selection
observable and tighter (`TU-02`). Repeated full verification and broad tool
output are visible candidates (`TU-03`, `TU-04`), but the stored events do not
attribute their model-token effect, so their savings are estimates to test.
Required checks and review columns remain part of every comparison.

## Snapshot and coverage

- Cutoff: `{snapshot['cutoff']}`; analyzer version `{VERSION}`; source SHA-256
  `{snapshot['source']['sha256']}`. The raw database is deliberately not in Git.
- Primary source: `{snapshot['source']['label']}`. The inventory also inspected
  {coverage['backup_count']} dated backups. They overlap the primary history;
  they establish retained-history availability, not extra work to sum.
- The primary source holds {inv['goals']} surviving goals, {inv['tasks']}
  surviving tasks, {inv['sessions']} surviving sessions, {inv['events']}
  events, and {inv['usage_rows']} cumulative transcript-source rows. It has
  {inv['running_sessions']} running sessions and {inv['in_progress_tasks']}
  in-progress tasks, reported as partial.
- The task listing supplied with this study said 48 goals and 179 tasks from
  October 5–14. The database has a wider surviving inventory; neither source
  proves that deleted rows before the retained ledger are complete.

## Counting rules and reconciliation

`input_tokens` includes cached input; `cached_input_tokens` is a subset, so
uncached input is `input - cached`, never another additive total. Canonical
usage is the sum of `session_usage` once per `(session_id, source)`, because
those are cumulative transcript totals that replace earlier reports. It is
{u['input_tokens']:,} input ({uncached(u):,} uncached +
{u['cached_input_tokens']:,} cached) and {u['output_tokens']:,} output tokens.

The session-end ledger is not added to that total: an end fact stores the
session's cumulative usage and a restarted session can produce more than one
such fact. Summing it would duplicate tokens. For deleted sessions, the
latest surviving end fact is a fallback only when there is no live usage row;
{coverage['deleted_session_fallbacks']} such fallback(s) were needed here.
{coverage['sessions_without_usage']} surviving session(s) have no usage row:
that is **missing usage**, not demonstrated zero. Sessions with an actual
usage row of zero are counted separately ({coverage['zero_usage_sessions']}).

`reconciliation.json` records the direct usage total, non-additive end-fact
total, unmatched facts, and attribution gaps. The session source column is a
transcript path, not a launch id; usage cannot be allocated reliably between
relaunches. Existing rows provide task/goal/role/model/effort attribution;
retained facts preserve some deleted-work attribution but no complete task
or goal identity after deletion.

## Inventory and aggregates

The files below cover every surviving goal and task, including cancelled and
failed/retried work where its rows or facts remain. `goals.csv` and
`tasks.csv` use canonical session usage; `dimensions.csv` groups the same
canonical total by role, workflow column, model, effort, and current outcome.
`outcomes.csv` also retains task-end facts, whose count can exceed surviving
tasks because retries write another ending.

- [snapshot.json](snapshot.json) — cutoff, revisions, coverage and counts.
- [goals.csv](goals.csv) and [tasks.csv](tasks.csv) — one aggregate per
  surviving entity, with distinct token categories and partial flags.
- [dimensions.csv](dimensions.csv) — comparable dimensional totals.
- [outcomes.csv](outcomes.csv) and [reconciliation.json](reconciliation.json)
  — retained facts, gaps, and non-additive counter checks.
- [trace-selection.json](trace-selection.json) — selected trace identifiers
  and derived indicators only; raw payloads stay outside Git.

## Timing evidence

The database records session and task wall-clock lifetimes, not CPU time or
model active time. It records console-permission waits in ledger facts where
available, but it does not separate queue delay, human wait outside a
permission prompt, tool execution, model inference, or active execution.

- Ended-session lifetime observed: {timing['ended_session_lifetime_secs']:,} s
  across {timing['ended_sessions_with_lifetime']} sessions. This is inclusive
  wall time, so it must not be called execution time.
- Completed/cancelled task lead time comes from task rows and transitions;
  it includes pending, ready, and in-progress time. The aggregate status
  totals in `outcomes.csv` are the only supported queue/status split.
- Running work is partial at the cutoff and excluded from completed-work
  elapsed comparisons. This review's own goal is separately labelled in the
  aggregates when present; its running session has no final usage at cutoff.

## Trace method and findings

The analyzer selects the largest canonical-usage sessions plus ordinary
completed agent and reviewer sessions, without reading or exporting raw
content. The selected traces cover {traces['selected_usage_share']:.1%} of
canonical usage with usage and are listed in `trace-selection.json`. Derived
event counts flag: repeated daemon prompts, repeated tool command prefixes,
and large tool-result payloads. They are evidence of repeated context or
wide tool responses, not direct token measurements.

Within the selected traces, the largest observed daemon-prompt count is
{traces['max_daemon_prompts']} and the largest observed daemon-prompt text
volume is {traces['max_daemon_prompt_chars']:,} characters. The selected
trace with the most repeated command prefixes has
{traces['max_repeated_tool_prefixes']}. These indicators establish where to
sample; they do not establish that every repetition is avoidable.

The repository already limits several suspected sources: skills are indexed as
one line and opened only when needed (`agents/prompts.rs`), handoff rendering
folds entries and applies a character budget (`agents/handoff.rs`), and
transcript counters are cumulative per source (`store/usage.rs`). These are
implemented safeguards, not new recommendations. Their presence also means a
larger handoff or prompt metric is a reason to inspect a selected trace before
changing policy.

## Ranked candidates

{candidate_lines}

## Limitations and what would change the ranking

The audit cannot reconstruct tokens per prompt, tool call, or launch, because
the database stores cumulative transcript counters rather than per-turn
usage. It cannot infer active execution time from wall-clock fields. A model
or effort comparison is observational and confounded by task size, role, and
outcome; compare like role/column/outcome workloads before acting. The ranking
would change if a controlled before/after run preserves the same required
checks and shows no reduction in tokens or lead time, or if trace review shows
the flagged content is necessary to correct outcomes.

## Reproduction

See [README.md](README.md). The command reads a database in read-only mode;
copy a live SQLite database with its WAL using SQLite's backup API first, and
keep that copy outside the repository.
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path, help="SQLite snapshot to read")
    parser.add_argument("--output", required=True, type=Path, help="directory for aggregate artifacts")
    parser.add_argument("--revision", default="unknown", help="Ariadne source revision used to analyze")
    parser.add_argument("--history-dir", type=Path, help="directory holding dated database backups; defaults to snapshot parent")
    parser.add_argument("--source-label", help="safe source description to publish instead of the snapshot path")
    args = parser.parse_args()

    source = args.database.resolve()
    if not source.is_file():
        parser.error(f"database is not a file: {source}")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(f"file:{source}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row

    required = ("goals", "tasks", "agent_sessions", "session_usage", "agent_events", "stat_facts")
    absent = [name for name in required if not table_exists(conn, name)]
    if absent:
        parser.error("unsupported database; missing " + ", ".join(absent))

    goals = read_rows(conn, "SELECT * FROM goals ORDER BY created_at")
    tasks = read_rows(conn, "SELECT * FROM tasks ORDER BY created_at")
    sessions = read_rows(conn, "SELECT * FROM agent_sessions ORDER BY created_at")
    usages = read_rows(conn, "SELECT * FROM session_usage")
    facts = read_rows(conn, "SELECT * FROM stat_facts ORDER BY created_at")
    event_count = conn.execute("SELECT COUNT(*) FROM agent_events").fetchone()[0]
    task_agents = read_rows(conn, "SELECT * FROM task_agents") if table_exists(conn, "task_agents") else []
    transitions = read_rows(conn, "SELECT * FROM task_transitions") if table_exists(conn, "task_transitions") else []

    goals_by_id = {x["id"]: x for x in goals}
    tasks_by_id = {x["id"]: x for x in tasks}
    agents_by_id = {x["id"]: x for x in task_agents}
    sessions_by_id = {x["id"]: x for x in sessions}
    usage_by_session: dict[str, dict[str, int]] = collections.defaultdict(usage_dict)
    source_rows_by_session: collections.Counter[str] = collections.Counter()
    for row in usages:
        add_usage(usage_by_session[row["session_id"]], row)
        source_rows_by_session[row["session_id"]] += 1

    # An ended-session fact is cumulative. Only a fact for a missing session
    # may fill a canonical gap, and the newest fact wins for its session.
    latest_end_fact: dict[str, dict[str, Any]] = {}
    for fact in facts:
        if fact["kind"] == "session_ended" and fact["session_id"]:
            latest_end_fact[fact["session_id"]] = fact
    fallback_usage: dict[str, dict[str, int]] = {}
    for session_id, fact in latest_end_fact.items():
        if session_id in sessions_by_id or session_id in usage_by_session:
            continue
        data = json.loads(fact["data"])
        fallback_usage[session_id] = {
            "input_tokens": max(0, int(data.get("input_tokens", 0))),
            "cached_input_tokens": max(0, int(data.get("cached_input_tokens", 0))),
            "output_tokens": max(0, int(data.get("output_tokens", 0))),
        }

    canonical = usage_dict()
    for row in usage_by_session.values():
        add_usage(canonical, row)
    for row in fallback_usage.values():
        add_usage(canonical, row)

    end_fact_sum = usage_dict()
    for fact in facts:
        if fact["kind"] == "session_ended":
            data = json.loads(fact["data"])
            add_usage(end_fact_sum, data)

    def session_meta(session: dict[str, Any]) -> tuple[str, str, str, str, str]:
        agent = agents_by_id.get(session.get("task_agent_id"), {})
        task = tasks_by_id.get(session.get("task_id"), {})
        return (
            session.get("seat") or "unknown",
            agent.get("step") or task.get("step") or "unknown",
            session.get("model") or "unknown",
            session.get("effort") or "default",
            task.get("status") or "unattributed",
        )

    dimensions: dict[tuple[str, str], dict[str, Any]] = {}
    def dimension(dimension: str, key: str, usage: dict[str, int], partial: bool) -> None:
        item = dimensions.setdefault((dimension, key), {"dimension": dimension, "value": key, "sessions": 0, "partial_sessions": 0, **usage_dict()})
        item["sessions"] += 1
        item["partial_sessions"] += int(partial)
        add_usage(item, usage)

    goal_usage: dict[str, dict[str, int]] = collections.defaultdict(usage_dict)
    task_usage: dict[str, dict[str, int]] = collections.defaultdict(usage_dict)
    goal_sessions: collections.Counter[str] = collections.Counter()
    task_sessions: collections.Counter[str] = collections.Counter()
    for session in sessions:
        sid = session["id"]
        usage = usage_by_session.get(sid, usage_dict())
        partial = session["status"] in {"running", "starting", "idle", "waiting_permission"}
        role, column, model, effort, outcome = session_meta(session)
        for dim, value in (("role", role), ("workflow_column", column), ("model", model), ("effort", effort), ("outcome", outcome)):
            dimension(dim, value, usage, partial)
        if session.get("goal_id"):
            add_usage(goal_usage[session["goal_id"]], usage)
            goal_sessions[session["goal_id"]] += 1
        if session.get("task_id"):
            add_usage(task_usage[session["task_id"]], usage)
            task_sessions[session["task_id"]] += 1

    task_rows: list[dict[str, Any]] = []
    for task in tasks:
        row = {"task_id": task["id"], "goal_id": task["goal_id"], "title": task["title"], "status": task["status"], "step": task.get("step") or "", "created_at": task["created_at"], "updated_at": task["updated_at"], "sessions": task_sessions[task["id"]], "partial": int(task["status"] == "in_progress"), "lead_time_secs": seconds(task["created_at"], task["updated_at"]) or "", **task_usage[task["id"]]}
        row["uncached_input_tokens"] = uncached(row)
        task_rows.append(row)
    goal_rows: list[dict[str, Any]] = []
    for goal in goals:
        row = {"goal_id": goal["id"], "title": goal["title"], "status": goal["status"], "workflow": goal.get("workflow") or "", "created_at": goal["created_at"], "updated_at": goal["updated_at"], "sessions": goal_sessions[goal["id"]], "partial": int(goal["status"] in {"active", "planning"}), "lead_time_secs": seconds(goal["created_at"], goal["updated_at"]) or "", **goal_usage[goal["id"]]}
        row["uncached_input_tokens"] = uncached(row)
        goal_rows.append(row)

    ended_lifetime = 0
    ended_count = 0
    for session in sessions:
        lifetime = seconds(session.get("created_at"), session.get("ended_at"))
        if lifetime is not None and session["status"] in {"exited", "failed"}:
            ended_lifetime += round(lifetime)
            ended_count += 1

    # Analyze only derived per-session event indicators. No payload text leaves
    # the process; hashes let a reviewer reproduce repeated-content counts.
    trace_metrics: dict[str, dict[str, Any]] = collections.defaultdict(lambda: {"events": 0, "daemon_prompts": 0, "daemon_prompt_chars": 0, "large_tool_results": 0, "large_tool_result_chars": 0, "tool_prefixes": collections.Counter()})
    for row in conn.execute("SELECT session_id, kind, payload, payload_codec FROM agent_events"):
        sid = row["session_id"] or "unattributed"
        metric = trace_metrics[sid]
        metric["events"] += 1
        body = payload(row["payload"], row["payload_codec"])
        if not body:
            continue
        if row["kind"] == "user_prompt_submit" and body.get("source") == "daemon":
            text = text_at(body, ("text",), ("prompt",)) or ""
            metric["daemon_prompts"] += 1
            metric["daemon_prompt_chars"] += len(text)
        if row["kind"] in {"pre_tool_use", "post_tool_use"}:
            command = text_at(body, ("tool_input", "command"), ("tool", "input", "command"), ("command",))
            if command:
                metric["tool_prefixes"][command.splitlines()[0][:80]] += 1
            result = text_at(body, ("tool_output",), ("tool", "output"), ("output",)) or ""
            if len(result) >= 4000:
                metric["large_tool_results"] += 1
                metric["large_tool_result_chars"] += len(result)

    ranked_sessions = sorted(usage_by_session, key=lambda sid: sum(usage_by_session[sid].values()), reverse=True)
    selected_ids = ranked_sessions[:12]
    # Add ordinary completed agent/reviewer sessions if largest traces omit them.
    for seat in ("agent", "reviewer"):
        for session in sessions:
            if session.get("seat") == seat and session.get("status") == "exited" and session["id"] in usage_by_session and session["id"] not in selected_ids:
                selected_ids.append(session["id"])
                break
    selected = []
    selected_tokens = 0
    for sid in selected_ids:
        session = sessions_by_id.get(sid, {})
        usage = usage_by_session[sid]
        selected_tokens += sum(usage.values())
        metric = trace_metrics[sid]
        selected.append({"session_id": sid, "goal_id": session.get("goal_id"), "task_id": session.get("task_id"), "seat": session.get("seat"), "model": session.get("model"), "effort": session.get("effort"), "status": session.get("status"), "usage": usage, "derived_event_metrics": {**{k: v for k, v in metric.items() if k != "tool_prefixes"}, "repeated_tool_prefixes": sum(n - 1 for n in metric["tool_prefixes"].values() if n > 1)}})
    total_tokens = sum(canonical.values())
    selected_share = selected_tokens / total_tokens if total_tokens else 0

    task_end_facts = [f for f in facts if f["kind"] == "task_ended"]
    outcome_facts = collections.Counter()
    status_seconds = collections.defaultdict(int)
    for fact in task_end_facts:
        data = json.loads(fact["data"])
        outcome_facts[data.get("status", "unknown")] += 1
        for state, value in (data.get("status_secs") or {}).items():
            status_seconds[state] += max(0, int(value or 0))

    history_files = []
    parent = (args.history_dir or source.parent).resolve()
    for candidate in sorted(parent.glob("ariadne.db*")):
        if candidate.name.endswith(("-wal", "-shm")) or not candidate.is_file():
            continue
        try:
            check = sqlite3.connect(f"file:{candidate}?mode=ro", uri=True)
            count = check.execute("SELECT COUNT(*) FROM goals").fetchone()[0]
            check.close()
            history_files.append({"name": candidate.name, "bytes": candidate.stat().st_size, "mtime": dt.datetime.fromtimestamp(candidate.stat().st_mtime, dt.timezone.utc).isoformat().replace("+00:00", "Z"), "goals": count})
        except sqlite3.Error:
            pass

    cutoff_candidates = [x.get("updated_at") for x in sessions + tasks + goals if x.get("updated_at")]
    cutoff = max(cutoff_candidates, default=NOW.isoformat())
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    running_sessions = sum(s["status"] in {"running", "starting", "idle", "waiting_permission"} for s in sessions)
    zero_usage = sum(s["id"] in usage_by_session and sum(usage_by_session[s["id"]].values()) == 0 for s in sessions)
    snapshot = {
        "analyzer_version": VERSION,
        "cutoff": cutoff,
        "generated_at": NOW.isoformat().replace("+00:00", "Z"),
        "source": {"label": args.source_label or source.name, "sha256": source_hash, "bytes": source.stat().st_size, "revision": args.revision, "sqlite_user_version": conn.execute("PRAGMA user_version").fetchone()[0]},
        "inventory": {"goals": len(goals), "tasks": len(tasks), "sessions": len(sessions), "events": event_count, "usage_rows": len(usages), "facts": len(facts), "running_sessions": running_sessions, "in_progress_tasks": sum(t["status"] == "in_progress" for t in tasks)},
        "canonical_usage": canonical,
        "coverage": {"history_files": history_files, "backup_count": max(0, len(history_files) - 1), "sessions_without_usage": sum(s["id"] not in usage_by_session for s in sessions), "zero_usage_sessions": zero_usage, "deleted_session_fallbacks": len(fallback_usage), "retained_session_end_facts_without_session": sum(sid not in sessions_by_id for sid in latest_end_fact), "unattributed_usage_sessions": sum(sid not in sessions_by_id for sid in usage_by_session)},
        "timing": {"ended_sessions_with_lifetime": ended_count, "ended_session_lifetime_secs": ended_lifetime, "unsupported": ["active model execution", "human wait outside console permissions", "queue time before a session launch", "tool execution", "per-launch elapsed time"]},
        "trace_selection": {"method": "12 largest canonical-usage sessions plus one ordinary exited agent and reviewer session when absent", "selected_sessions": len(selected), "selected_usage_share": selected_share, "max_daemon_prompts": max((x["derived_event_metrics"]["daemon_prompts"] for x in selected), default=0), "max_daemon_prompt_chars": max((x["derived_event_metrics"]["daemon_prompt_chars"] for x in selected), default=0), "max_repeated_tool_prefixes": max((x["derived_event_metrics"]["repeated_tool_prefixes"] for x in selected), default=0)},
    }
    recommendations = [
        {"id": "TU-01", "title": "Coalesce scheduler wake-ups before sending a full daemon prompt", "evidence": "Trace indicators count daemon-originated prompt deliveries per session and their character volume; each delivery can repeat goal/task status and handoff context. The database has no per-prompt usage, so this is a measured repetition indicator rather than measured token waste.", "scope": "Add a deduplicated pending-attention key at the scheduler/ACP delivery boundary. Preserve every distinct failure, user message, and explicit resume; do not change workflow columns, required checks, or prompt content for the first delivery.", "benefit": "Estimated reduction in repeated input context and avoidable wake-up latency; quantify only after instrumenting per-delivery counters.", "confidence": "medium", "measurement": "On matched tasks, compare daemon prompt count, prompt characters, canonical uncached input, output, task lead time, and finish/review outcomes. Keep the same workflow and required checks.", "counter_evidence": "A wake-up may carry newly relevant state. If coalescing delays recovery, increases retries, or changes outcome quality, retain the existing behavior."},
        {"id": "TU-02", "title": "Measure and tighten handoff selection at the existing character budget", "evidence": "The code already folds and ranks handoff history, but cumulative session usage cannot show which selected entries caused input. Long/restarted sessions are identifiable in the trace sample, making this a bounded observability-first candidate.", "scope": "Add offline/diagnostic handoff selection metrics and a fixture corpus before changing the ranking policy. Do not shrink the configured budget or discard corrections, blockers, failed commands, edits, or decisive checks without an outcome comparison.", "benefit": "Estimated lower repeated input and faster resume turns where a handoff is much smaller than raw history.", "confidence": "low", "measurement": "For matched resumes, record candidate/selected character counts, continuation turns, canonical tokens, time to next useful state, and task outcome. Preserve existing handoff behavior as the control.", "counter_evidence": "The existing rules intentionally protect high-value evidence. A smaller handoff that causes re-reading, wrong changes, or review churn is a regression."},
        {"id": "TU-03", "title": "Classify repeated verification before suppressing any command", "evidence": "Selected traces expose repeated tool-command prefixes, while task end facts retain retries and review cycles. The record does not identify whether repetitions were required by changed code, flaky checks, or review findings.", "scope": "Build an offline classifier/report for repeated commands keyed by commit/head and workflow column. Do not skip hooks, required checks, or reviewer-requested reruns.", "benefit": "Potentially lower tool wait and subsequent context tokens by avoiding proven duplicate checks; benefit is unquantified.", "confidence": "low", "measurement": "Sample repeated commands and label necessity. In a later controlled implementation, compare elapsed tool duration, retry rate, review findings, and task outcomes against the same check policy.", "counter_evidence": "Repeated checks often establish a flaky-test proof or validate a later change. The audit must not equate repetition with waste."},
        {"id": "TU-04", "title": "Bound broad tool-result retention with task-aware summaries", "evidence": "The trace indicators count large stored tool-result payloads. Tool outputs may be echoed into agent context or handoffs, but this database does not record prompt assembly per turn, so causal token savings are untested.", "scope": "First add a report of large result payloads by tool, role, and outcome. Any later truncation must keep exit status, failing lines, paths, diffs, and an explicit way to retrieve full output.", "benefit": "Potentially lower input context and handoff size, plus faster model turns on noisy command output.", "confidence": "low", "measurement": "Compare prompt/handoff size, canonical token categories, diagnosis success, retry rate, and reviewer findings on matched command-heavy tasks.", "counter_evidence": "Full output can contain the only failure clue. Do not truncate before a representative trace review demonstrates redundant material."},
    ]
    reconciliation = {"canonical_rule": "sum session_usage once per (session_id, source), then use newest session_ended fact only for missing/deleted sessions", "canonical_usage": canonical, "uncached_input_tokens": uncached(canonical), "session_ended_fact_sum_non_additive": end_fact_sum, "session_ended_facts": len(latest_end_fact), "session_ended_facts_missing_session": sum(sid not in sessions_by_id for sid in latest_end_fact), "fallback_usage": fallback_usage, "notes": ["Cached input is a subset of input and is never added to it.", "End facts contain cumulative session totals and can duplicate a session across launches.", "Usage source is a transcript source, not a launch attribution key."]}

    write_csv(out / "goals.csv", goal_rows, ["goal_id", "title", "status", "workflow", "created_at", "updated_at", "sessions", "partial", "lead_time_secs", "input_tokens", "cached_input_tokens", "uncached_input_tokens", "output_tokens"])
    write_csv(out / "tasks.csv", task_rows, ["task_id", "goal_id", "title", "status", "step", "created_at", "updated_at", "sessions", "partial", "lead_time_secs", "input_tokens", "cached_input_tokens", "uncached_input_tokens", "output_tokens"])
    dim_rows = []
    for value in dimensions.values():
        value["uncached_input_tokens"] = uncached(value)
        dim_rows.append(value)
    write_csv(out / "dimensions.csv", sorted(dim_rows, key=lambda x: (x["dimension"], -x["input_tokens"])), ["dimension", "value", "sessions", "partial_sessions", "input_tokens", "cached_input_tokens", "uncached_input_tokens", "output_tokens"])
    write_csv(out / "outcomes.csv", [{"kind": "surviving_task", "outcome": status, "count": count, "status_seconds": status_seconds.get(status, "")} for status, count in sorted(collections.Counter(t["status"] for t in tasks).items())] + [{"kind": "task_ended_fact", "outcome": status, "count": count, "status_seconds": status_seconds.get(status, "")} for status, count in sorted(outcome_facts.items())], ["kind", "outcome", "count", "status_seconds"])
    (out / "snapshot.json").write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out / "reconciliation.json").write_text(json.dumps(reconciliation, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out / "trace-selection.json").write_text(json.dumps({**snapshot["trace_selection"], "traces": selected}, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out / "recommendations.json").write_text(json.dumps(recommendations, indent=2) + "\n", encoding="utf-8")
    (out / "REPORT.md").write_text(render_markdown(snapshot, recommendations), encoding="utf-8")
    print(json.dumps({"output": str(out), "canonical_usage": canonical, "goals": len(goals), "tasks": len(tasks)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
