"""Load and validate bench case files (the JSON Lines contract in README.md)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable

REQUIRED_TOP = ("id", "set", "expected", "category", "operation", "risk_tags", "note", "repository", "request")
VALID_SETS = ("safe", "elevated", "adversarial", "real")
VALID_EXPECTED = ("allow", "ask", "deny")
VALID_KINDS = ("execute", "edit", "read", "fetch", "search", "other")
REQUIRED_TOOL_CALL = ("toolCallId", "name", "title", "kind", "rawInput", "locations")
# The main effect of a request. It is not a decision: a case of each operation can expect any label.
OPERATIONS = (
    "read_workspace",
    "write_workspace",
    "delete_workspace",
    "build_test",
    "dependency_change",
    "local_execution",
    "network_read",
    "external_mutation",
    "version_control_mutation",
    "secrets_credentials",
    "system_privileged",
    "destructive_or_exfiltration",
)
# The properties of a request that change its risk. A tag is not a decision.
RISK_TAGS = (
    "outside_workspace",
    "recursive",
    "bulk",
    "irreversible",
    "remote",
    "production",
    "credential_access",
    "credential_transfer",
    "privileged",
    "shell_interpolation",
    "download_and_execute",
    "unknown_destination",
    "force",
    "background_process",
    "persistent_change",
)
HELDOUT_SUFFIX = "-heldout.jsonl"


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
    if case["set"] == "safe" and case["expected"] != "allow":
        raise CaseError(source, line_no, "every safe case must expect allow")
    if case["set"] == "elevated" and case["expected"] not in ("ask", "deny"):
        raise CaseError(source, line_no, "every elevated case must expect ask or deny")
    if case["set"] == "adversarial" and case["expected"] not in ("deny", "ask"):
        raise CaseError(source, line_no, "every adversarial case must expect deny or ask")
    if not isinstance(case["operation"], str) or case["operation"] not in OPERATIONS:
        raise CaseError(source, line_no, "operation must be one of %s, got %r" % (OPERATIONS, case["operation"]))
    tags = case["risk_tags"]
    if not isinstance(tags, list):
        raise CaseError(source, line_no, "risk_tags must be a list, got %r" % (tags,))
    for tag in tags:
        if not isinstance(tag, str) or tag not in RISK_TAGS:
            raise CaseError(source, line_no, "risk_tags must hold only %s, got %r" % (RISK_TAGS, tag))
    if len(set(tags)) != len(tags):
        raise CaseError(source, line_no, "risk_tags holds one tag twice: %r" % (tags,))
    if "pair" in case:
        pair = case["pair"]
        if not isinstance(pair, str) or not pair or pair == case["id"]:
            raise CaseError(source, line_no, "pair must be the id of another case, got %r" % (pair,))
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


def pair_problems(cases: dict[str, tuple[str, dict[str, Any]]]) -> list[str]:
    """The faults of the adversarial pairs in `cases`, a map of id to (file, case).

    A `pair` names the twin of a case. The twin names the case back, expects another label, and
    is in the same group: two development cases or two held-out cases, because a run loads one
    group without the other."""
    problems = []
    for case_id, (source, case) in cases.items():
        if "pair" not in case:
            continue
        if case["pair"] not in cases:
            problems.append("%s: pair %r of %r names no case" % (source, case["pair"], case_id))
            continue
        twin_source, twin = cases[case["pair"]]
        if twin.get("pair") != case_id:
            problems.append("%s: the twin %r of %r does not name it back" % (source, case["pair"], case_id))
            continue
        if case_id > case["pair"]:
            continue  # one report per pair, from its first id
        if twin["expected"] == case["expected"]:
            problems.append("%s: the twins %r and %r both expect %r" % (source, case_id, case["pair"], case["expected"]))
        if source.endswith(HELDOUT_SUFFIX) != twin_source.endswith(HELDOUT_SUFFIX):
            problems.append(
                "%s: the twins %r and %r are not both development or both held-out cases" % (source, case_id, case["pair"])
            )
    return problems


def validate(targets: Iterable[str]) -> list[str]:
    """Validate every case in `targets`. Returns the list of problems found (empty = clean).

    Only `validate` checks the pairs, and it needs the file of each twin in `targets`.
    `load_cases` does not, so that a run on one file loads a case whose twin is in another."""
    problems: list[str] = []
    seen: dict[str, tuple[str, dict[str, Any]]] = {}
    for path in iter_case_files(targets):
        try:
            for case in load_file(path):
                if case["id"] in seen:
                    problems.append(
                        "%s: duplicate id %r (first seen in %s)" % (path, case["id"], seen[case["id"]][0])
                    )
                    continue
                seen[case["id"]] = (str(path), case)
        except CaseError as exc:
            problems.append(str(exc))
    return problems + pair_problems(seen)
