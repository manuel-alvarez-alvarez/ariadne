"""Load and validate bench case files (the JSON Lines contract in README.md)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable
from urllib.parse import urlsplit

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
    "reviewer_directive",
    "permission_bypass",
    "root_or_home_delete",
)
HELDOUT_SUFFIX = "-heldout.jsonl"

# The three permission options of a real request (README "Case format"). The allow-always
# option is optional; about one real request in five carries only the other two.
ALLOW_ONCE = {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"}
REJECT_ONCE = {"optionId": "reject", "name": "No", "kind": "reject_once"}
ALLOW_ALWAYS_ID = "allow-with-updates"
ALLOW_ALWAYS_KIND = "allow_always"

# The kind and the required rawInput keys of a tool the shape table names (README "Case
# format"). `mcp__*` takes its own branch in `_check_shape`: kind "other", with no key required
# by name, since the table gives it "the tool's arguments".
TOOL_SHAPE = {
    "Bash": ("execute", ("command",)),
    "Edit": ("edit", ("file_path", "old_string", "new_string")),
    "Write": ("edit", ("file_path", "content")),
    "Read": ("read", ("file_path",)),
    "WebFetch": ("fetch", ("url", "prompt")),
    "WebSearch": ("fetch", ("query",)),
}


def _bash_prefix(command: str) -> str:
    """The words a `Bash` allow-always option names: the program, and the next word when it is
    not a flag."""
    words = command.split()
    if not words:
        return ""
    prefix = [words[0]]
    if len(words) > 1 and not words[1].startswith("-"):
        prefix.append(words[1])
    return " ".join(prefix)


def _last_dir(file_path: str) -> str:
    """The directory name a `Read` allow-always option names: the last path component before
    the file."""
    parent = file_path.rsplit("/", 1)[0] or "/"
    return parent.rsplit("/", 1)[-1] or "/"


def _tool_words(name: str) -> str:
    """The words an `mcp__*` allow-always option names: the tool's own name, after its server,
    split on `_` and title-cased."""
    tail = name.rsplit("__", 1)[-1]
    return " ".join(word.capitalize() for word in tail.split("_") if word)


def expected_title(tool_call: dict[str, Any], repository: str | None) -> str | None:
    """The title the real request fixes for `tool_call` (README "Case format"). `None` for a
    tool the table does not name, which carries no title rule."""
    name = tool_call.get("name", "")
    raw_input = tool_call.get("rawInput", {}) or {}
    if name == "Bash":
        return str(raw_input.get("command", ""))
    if name in ("Edit", "Write"):
        file_path = str(raw_input.get("file_path", ""))
        if repository and (file_path == repository or file_path.startswith(repository + "/")):
            file_path = file_path[len(repository) + 1 :]
        return "%s %s" % (name, file_path)
    if name == "Read":
        return "Read %s" % raw_input.get("file_path", "")
    if name == "WebFetch":
        return "Fetch %s" % raw_input.get("url", "")
    if name == "WebSearch":
        return 'Search "%s"' % raw_input.get("query", "")
    if name.startswith("mcp__"):
        return name
    return None


def expected_allow_always_name(tool_call: dict[str, Any]) -> str:
    """The name of the allow-always option for `tool_call` (README "Case format"). A tool the
    table does not name takes the fallback the table gives `WebSearch`: its own name."""
    name = tool_call.get("name", "")
    raw_input = tool_call.get("rawInput", {}) or {}
    if name == "Bash":
        return "Yes, and don't ask again for %s * commands" % _bash_prefix(str(raw_input.get("command", "")))
    if name in ("Edit", "Write"):
        return "Yes, allow all edits during this session"
    if name == "Read":
        return "Yes, allow reading from %s/ during this session" % _last_dir(str(raw_input.get("file_path", "")))
    if name == "WebFetch":
        return "Yes, and don't ask again for %s" % (urlsplit(str(raw_input.get("url", ""))).hostname or "")
    if name.startswith("mcp__"):
        return "Yes, and don't ask again for %s commands" % _tool_words(name)
    return "Yes, and don't ask again for %s commands" % name


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
    _check_shape(case, source, line_no)


def _check_shape(case: dict[str, Any], source: str, line_no: int | None) -> None:
    """The shape of a real `session/request_permission` request (README "Case format"): the
    title and the input per tool, and the permission options, each regardless of label."""
    tool_call = case["request"]["toolCall"]
    repository = case.get("repository")
    name = tool_call.get("name", "")
    raw_input = tool_call.get("rawInput")
    if not isinstance(raw_input, dict):
        raise CaseError(source, line_no, "rawInput must be an object, got %r" % (raw_input,))

    if name in TOOL_SHAPE:
        expected_kind, required_keys = TOOL_SHAPE[name]
        if tool_call.get("kind") != expected_kind:
            raise CaseError(source, line_no, "%s must have kind %r, got %r" % (name, expected_kind, tool_call.get("kind")))
        for key in required_keys:
            if not isinstance(raw_input.get(key), str):
                raise CaseError(source, line_no, "%s rawInput.%s must be a string, got %r" % (name, key, raw_input.get(key)))
    elif name.startswith("mcp__") and tool_call.get("kind") != "other":
        raise CaseError(source, line_no, "an mcp tool must have kind 'other', got %r" % (tool_call.get("kind"),))

    title = expected_title(tool_call, repository)
    if title is not None and tool_call.get("title") != title:
        raise CaseError(source, line_no, "title must be %r for %s, got %r" % (title, name, tool_call.get("title")))

    if name in ("Edit", "Write", "Read"):
        file_path = raw_input.get("file_path")
        if not isinstance(file_path, str) or not file_path.startswith("/"):
            raise CaseError(source, line_no, "%s rawInput.file_path must be an absolute path, got %r" % (name, file_path))
        if tool_call.get("locations") != [{"path": file_path}]:
            raise CaseError(
                source, line_no, "locations must be [{'path': file_path}] for %s, got %r" % (name, tool_call.get("locations"))
            )

    if name == "Edit" and not isinstance(raw_input.get("replace_all"), bool):
        raise CaseError(source, line_no, "Edit rawInput.replace_all must be a bool, got %r" % (raw_input.get("replace_all"),))

    options = case["request"]["options"]
    if len(options) not in (2, 3):
        raise CaseError(source, line_no, "request.options must hold 2 or 3 options, got %d" % len(options))
    if options[0] != ALLOW_ONCE:
        raise CaseError(source, line_no, "the first option must be %r, got %r" % (ALLOW_ONCE, options[0]))
    if options[-1] != REJECT_ONCE:
        raise CaseError(source, line_no, "the last option must be %r, got %r" % (REJECT_ONCE, options[-1]))
    if len(options) == 3:
        middle = options[1]
        expected_name = expected_allow_always_name(tool_call)
        if middle.get("optionId") != ALLOW_ALWAYS_ID or middle.get("kind") != ALLOW_ALWAYS_KIND or middle.get("name") != expected_name:
            raise CaseError(
                source,
                line_no,
                "the allow-always option must be {'optionId': %r, 'name': %r, 'kind': %r}, got %r"
                % (ALLOW_ALWAYS_ID, expected_name, ALLOW_ALWAYS_KIND, middle),
            )


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
