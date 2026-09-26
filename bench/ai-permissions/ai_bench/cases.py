"""Load and validate bench case files (the JSON Lines contract in README.md)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable

REQUIRED_TOP = ("id", "set", "expected", "category", "note", "repository", "request")
VALID_SETS = ("safe", "elevated", "adversarial", "real")
VALID_EXPECTED = ("allow", "escalate")
VALID_KINDS = ("execute", "edit", "read", "fetch", "search", "other")
REQUIRED_TOOL_CALL = ("toolCallId", "name", "title", "kind", "rawInput", "locations")


class CaseError(ValueError):
    def __init__(self, source: str, line_no: int | None, message: str):
        where = "%s:%s" % (source, line_no) if line_no is not None else source
        super().__init__("%s: %s" % (where, message))
        self.source = source
        self.line_no = line_no


def _check(case: dict[str, Any], source: str, line_no: int | None) -> None:
    if not isinstance(case, dict):
        raise CaseError(source, line_no, "a case must be a JSON object")
    for field in REQUIRED_TOP:
        if field not in case:
            raise CaseError(source, line_no, "missing field %r" % field)
    if case["set"] not in VALID_SETS:
        raise CaseError(source, line_no, "set must be one of %s, got %r" % (VALID_SETS, case["set"]))
    if case["expected"] not in VALID_EXPECTED:
        raise CaseError(source, line_no, "expected must be one of %s, got %r" % (VALID_EXPECTED, case["expected"]))
    if case["set"] in ("elevated", "adversarial") and case["expected"] != "escalate":
        raise CaseError(source, line_no, "every elevated/adversarial case must expect escalate")
    request = case["request"]
    if not isinstance(request, dict) or "toolCall" not in request or "options" not in request:
        raise CaseError(source, line_no, "request must hold toolCall and options")
    tool_call = request["toolCall"]
    for field in REQUIRED_TOOL_CALL:
        if field not in tool_call:
            raise CaseError(source, line_no, "request.toolCall missing field %r" % field)
    if tool_call["kind"] not in VALID_KINDS:
        raise CaseError(source, line_no, "toolCall.kind must be one of %s, got %r" % (VALID_KINDS, tool_call["kind"]))
    if not isinstance(request["options"], list) or not request["options"]:
        raise CaseError(source, line_no, "request.options must be a non-empty list")


def load_file(path: Path) -> list[dict[str, Any]]:
    cases = []
    with open(path, encoding="utf-8") as f:
        for line_no, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                case = json.loads(line)
            except json.JSONDecodeError as exc:
                raise CaseError(str(path), line_no, "invalid JSON: %s" % exc) from exc
            _check(case, str(path), line_no)
            cases.append(case)
    return cases


def iter_case_files(targets: Iterable[str]) -> list[Path]:
    files = []
    for target in targets:
        p = Path(target)
        if p.is_dir():
            files.extend(sorted(p.glob("*.jsonl")))
        else:
            files.append(p)
    return files


def load_cases(targets: Iterable[str]) -> list[dict[str, Any]]:
    """Load and validate every case in `targets`. Raises CaseError on the first fault."""
    cases: list[dict[str, Any]] = []
    seen_ids: dict[str, str] = {}
    for path in iter_case_files(targets):
        for case in load_file(path):
            if case["id"] in seen_ids:
                raise CaseError(str(path), None, "duplicate id %r (first seen in %s)" % (case["id"], seen_ids[case["id"]]))
            seen_ids[case["id"]] = str(path)
            cases.append(case)
    return cases


def validate(targets: Iterable[str]) -> list[str]:
    """Validate every case in `targets`. Returns the list of problems found (empty = clean)."""
    problems: list[str] = []
    seen_ids: dict[str, str] = {}
    for path in iter_case_files(targets):
        try:
            for case in load_file(path):
                if case["id"] in seen_ids:
                    problems.append(
                        "%s: duplicate id %r (first seen in %s)" % (path, case["id"], seen_ids[case["id"]])
                    )
                    continue
                seen_ids[case["id"]] = str(path)
        except CaseError as exc:
            problems.append(str(exc))
    return problems
