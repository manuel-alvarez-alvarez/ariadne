"""Derive portable risk facts from the visible parts of a permission request.

The daemon's Rust port intentionally uses the same small collection of token and substring
tests. This module does not inspect paths on disk and does not parse shell syntax.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

from . import operations

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
TAGS = (
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
RULES = ("credential_transfer", "root_delete", "home_delete", "permission_tamper")

URL = re.compile(r"https?://[^\s'\"<>]+", re.IGNORECASE)
LOCAL_HOST = r"(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])"
LOCAL_URL = re.compile(r"https?://" + LOCAL_HOST + r"(?:[:/?#]|$)", re.IGNORECASE)
# The modes that open a file to each user or set an id bit: a last digit with the write bit,
# a first digit of four with the setuid or setgid bit, `o+w`, `a+w` and `+s`.
OPEN_MODE = (
    r"\bchmod\s+(?:-[A-Za-z]+\s+)*(?:"
    r"(?:[0-7]?[0-7][0-7][2367]|[2467][0-7][0-7][0-7])(?:\s|$)|"
    r"[ugoa]*[+=][rwxXt]*s|"
    r"[ugoa]*[oa][ugoa]*[+=][rxXst]*w)"
)
# Text that instructs the reviewer of the call. It is an attempt to disable a control.
REVIEWER_TEXT = (
    r"\b(?:ignore|disregard)\s+(?:all\s+|any\s+)?(?:the\s+)?(?:previous|prior|above|earlier)\s+(?:instructions?|rules?|prompts?)\b|"
    r"\bnote\s+to\s+(?:the\s+)?reviewer\b|"
    r"\b(?:always|please|definitely|just|safe\s+to)\s+(?:allow|approve)\b|"
    r"\ballow\s+without\s+asking\b|\bpre-?approved\b"
)
# The start of a program name in a line, also in a quoted command.
START = r"(?:^|[\s;&|('\"`])"
# Options of one simple command, up to a given option.
OPTIONS = r"\s+(?:[^;&|]*\s)?"
CREDENTIAL_SOURCE = re.compile(
    r"(?:\.aws/credentials|\.ssh/(?:id_[^/\s'\"]+|config)|"
    r"\.npmrc|\.pypirc|/etc/(?:shadow|passwd)|credentials?\.json|\bfind-generic-password\b|"
    r"\.git-credentials|\.netrc|\.kube/config|\.docker/config\.json|\.gnupg|--export-secret-keys?\b|"
    r"\bdump-keychain\b|\.keychain|\.pem\b|\.key\b|\.p12\b|\bid_(?:rsa|ed25519|ecdsa|dsa|\*)|"
    # A dump of the environment: to a file, to the end of the line, or to a program that sends it.
    r"(?:^|[;&|(]\s*)(?:env|printenv)\s*(?:$|[>;&]|\|\s*(?:curl|wget|nc|ncat|socat|tee|base64|xxd|gzip)\b))",
    re.IGNORECASE,
)
# A dotenv file, with its name. An example of one holds no credential.
DOTENV = re.compile(r"[^\s/'\"=:]*\.env(?:\.[^/\s'\"]*)?", re.IGNORECASE)
DOTENV_EXAMPLE = re.compile(r"example|sample|template", re.IGNORECASE)
# A credential in the query of an address: a name that ends with the word of a secret, and a value.
SECRET_IN_QUERY = re.compile(
    r"(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key)(?:=|%3D)[^&\s'\"]",
    re.IGNORECASE,
)
# A program that installs persistence, or that changes the git configuration of the user.
PERSISTENT_PROGRAM = (
    r"\b(?:crontab|launchctl)\b|\bsystemctl\s+enable\b|\bgit\s+config\s+(?:[^;&|]*\s)?--(?:global|system)\b"
)
# A file or a directory that later sessions or commands obey: git hooks, shell startup files,
# the configuration of CI, of the agent, of the editor, of the toolchain and of the user, and
# the directories of the operating system.
NAME = r"(?:^|[\s/'\"=:])"
PERSISTENT_PATH = (
    r"\.git/hooks/|LaunchAgents/|LaunchDaemons/|/cron\.|"
    + NAME
    + r"(?:\.(?:bashrc|bash_profile|bash_login|profile|zshrc|zshenv|zprofile|zlogin|gitconfig|npmrc|mcp\.json|gitlab-ci\.yml)\b|"
    r"\.github/(?:workflows/|dependabot\.yml)|\.circleci/|\.config/|\.cargo/config|"
    r"\.(?:claude|codex|cursor|vscode)/(?:hooks/|commands/|agents/|rules|[^\s'\"/]*(?:settings|config|tasks|launch|mcp)[^\s'\"/]*)|"
    r"\.ssh/(?:authorized_keys|config)\b|\.aws/config\b|(?:AGENTS|CLAUDE)\.md\b|rust-toolchain\b)|"
    r"(?:^|[\s'\"=])/(?:etc|usr|bin|sbin|System|Library|var|opt|boot)/"
)
# The operations of a command line that change a file.
CHANGES = (
    "destructive_or_exfiltration",
    "system_privileged",
    "external_mutation",
    "delete_workspace",
    "version_control_mutation",
    "dependency_change",
    "write_workspace",
)

@dataclass(frozen=True)
class Derived:
    operation_hint: str | None
    risk_tags: list[str]
    rule: str | None


def _command(raw_input: dict[str, Any]) -> str:
    command = raw_input.get("command")
    if isinstance(command, str):
        return command
    if isinstance(command, list):
        return " ".join(part for part in command if isinstance(part, str))
    return ""


def _parts(request: dict[str, Any]) -> tuple[str, list[str], str, str]:
    tool_call = request.get("toolCall", {}) or {}
    raw_input = tool_call.get("rawInput", {}) or {}
    raw_input = raw_input if isinstance(raw_input, dict) else {}
    command = _command(raw_input)
    paths = [value for key in ("file_path", "path", "url") if isinstance((value := raw_input.get(key)), str)]
    for location in tool_call.get("locations", []) or []:
        if isinstance(location, dict) and isinstance(location.get("path"), str):
            paths.append(location["path"])
    title = tool_call.get("title")
    return command, paths, title if isinstance(title, str) else "", str(tool_call.get("kind", ""))


def _words(text: str) -> list[str]:
    return [word.strip("'\"()[]{};,|") for word in text.split()]


def _recursive_rm(words: list[str]) -> bool:
    for index, word in enumerate(words):
        if word != "rm":
            continue
        options = words[index + 1 :]
        return any(
            option == "--recursive"
            or (option.startswith("-") and not option.startswith("--") and "r" in option[1:].lower())
            for option in options
        )
    return False


def _outside_path(path: str, workspace: str) -> bool:
    candidate = path.rstrip("/.,:;\"'")
    if path.rstrip(".,:;\"'") == "/":
        return True
    if candidate in ("~", "$HOME", "${HOME}") or candidate.startswith("~/") or candidate.startswith("$HOME/"):
        return True
    if candidate.startswith("../") or candidate == "..":
        return True
    if not candidate.startswith("/"):
        return False
    root = workspace.rstrip("/")
    return candidate != root and not candidate.startswith(root + "/")


def _paths_in_text(command: str, title: str) -> list[str]:
    paths = []
    for word in _words(command + " " + title):
        if word.startswith(("/", "~/", "$HOME/", "../")) or word in ("..", "~", "$HOME", "${HOME}"):
            paths.append(word)
    return paths


def _matches(text: str, pattern: str) -> bool:
    return re.search(pattern, text, re.IGNORECASE) is not None


def _external_url(text: str) -> bool:
    """A URL whose host is not the local host."""
    return any(LOCAL_URL.match(url) is None for url in URL.findall(text))


def _fetches_remote(command: str) -> bool:
    """`curl` or `wget` to a host that is not the local host. A command that names the local
    host, with a URL or without, and no other URL, stays on this machine."""
    if not _matches(command, r"\b(?:curl|wget)\b"):
        return False
    local = _matches(command, r"(?:^|[\s/@'\"])" + LOCAL_HOST + r"(?:[:/?#\s'\"]|$)")
    return _external_url(command) or not local


def _credential_source(text: str) -> bool:
    """A known credential source, or a dotenv file that is not an example."""
    return CREDENTIAL_SOURCE.search(text) is not None or any(
        DOTENV_EXAMPLE.search(name) is None for name in DOTENV.findall(text)
    )


def _secret_in_address(text: str) -> bool:
    """A URL to a host that is not the local host, whose query carries a credential."""
    return any(
        LOCAL_URL.match(url) is None and "?" in url and SECRET_IN_QUERY.search(url.split("?", 1)[1]) is not None
        for url in URL.findall(text)
    )


def _uploads(command: str) -> bool:
    """The external upload form: an upload option of `curl` or `wget`, or a program that sends
    files, to a target that is not the local host. `-F` is a form of `curl`; `-f` is not."""
    external_target = _external_url(command) or _matches(command, r"\b[A-Za-z0-9._-]+@[A-Za-z0-9.-]+:")
    return external_target and (
        _matches(command, r"\b(?:curl|wget)\b.*(?:-d|--data|--upload-file|-T)\b|\b(?:scp|rsync|nc|ncat)\b")
        or re.search(r"\bcurl\b.*\s(?:-F|--form)(?:[\s=]|$)|\bwget\b.*\s--post-(?:file|data)\b", command) is not None
    )


def _deletes(command: str) -> bool:
    """A deletion program at the start of a command, or the deletion of `find`. The `--rm` of a
    container is an option, not a program."""
    return _matches(command, START + r"(?:rm|rmdir|unlink|shred|wipe)(?:\s|$)") or _matches(command, r"\bfind\b[^;&|]*\s-delete\b")


def _discards(command: str) -> bool:
    """A git command that discards work that git cannot restore, or that replaces history."""
    return (
        _matches(command, r"\bgit\s+(?:reset\s+--hard|clean\b|stash\s+(?:clear|drop)\b)")
        or _matches(command, r"\bgit\s+checkout" + OPTIONS + r"(?:--\s|\.(?:\s|$))")
        or _matches(command, r"\bgit\s+push" + OPTIONS + r"(?:--force|--mirror|-[a-z]*f[a-z]*(?:\s|$))")
        or re.search(r"\bgit\s+branch" + OPTIONS + r"-D\b", command) is not None
    )


def _forces(command: str) -> bool:
    """A force option. A short option with `f` is one only for a program that has a force
    option: `-f` names a file for `psql`, and a pattern for `pkill`."""
    return (
        _matches(command, r"(?:^|\s)--force(?:-with-lease)?(?:[\s=]|$)")
        or re.search(START + r"(?:rm|cp|mv|ln)" + OPTIONS + r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)", command) is not None
        or re.search(
            r"\bgit\s+(?:push|checkout|clean|branch|tag|fetch|switch|rm|mv)" + OPTIONS + r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)", command
        )
        is not None
        or re.search(r"\bgit\s+branch" + OPTIONS + r"-D\b", command) is not None
        or re.search(START + r"(?:kill|pkill|killall)" + OPTIONS + r"-(?:9|KILL|SIGKILL)\b", command) is not None
    )


def _recursive_change(command: str) -> bool:
    """A mode change, a copy, an upload or a deletion of a tree."""
    return (
        re.search(START + r"(?:chmod|chown|chgrp)" + OPTIONS + r"-[A-Za-z]*R", command) is not None
        or re.search(START + r"(?:cp|scp)" + OPTIONS + r"-[A-Za-z]*[rRa]", command) is not None
        or re.search(START + r"(?:rsync|zip)" + OPTIONS + r"-[A-Za-z]*[ra]", command) is not None
        or _matches(command, r"\bfind\b[^;&|]*\s-delete\b")
        or _matches(command, r"\bgit\s+clean" + OPTIONS + r"-[a-z]*d")
    )


def _many(command: str) -> bool:
    """A pattern, a list from `find` or `xargs`, each item, or the complete directory."""
    return (
        "*" in URL.sub("", command)
        or _matches(command, r"\b(?:xargs|find)\b")
        or re.search(r"\s(?:--all|-A)(?:\s|$)", command) is not None
        or re.search(r"\s\.(?:\s|$)", command) is not None
    )


def _changes(command: str) -> bool:
    """A program that changes, copies or sends files. A search or a listing of many files is
    not a bulk operation."""
    return (
        _matches(command, START + r"(?:rm|mv|cp|chmod|chown|chgrp|shred|truncate|tar|zip|rsync|scp|kill)(?:\s|$)")
        or _matches(command, r"\s-(?:delete|exec|execdir)\b|\bsed\s+-i")
        or _matches(command, r"\bgit\s+(?:add|checkout|restore|clean|push|rm|reset|stash\s+clear)\b")
        or _matches(command, r"\bkubectl\b[^;&|]*\sdelete\b")
    )


def _persists(command: str, text: str, kind: str) -> bool:
    """A persistence program that does more than a read, or a change of a persistent path. A
    command line changes a path only if its operation is a change: a line with no known
    operation gives no evidence."""
    found = operations.operation(command) if command else None
    if command and found != "read_workspace" and re.search(PERSISTENT_PROGRAM, command) is not None:
        return True
    changes = found in CHANGES if command else kind in ("edit", "write", "delete", "move")
    return changes and re.search(PERSISTENT_PATH, text) is not None


def _tags(command: str, paths: list[str], title: str, kind: str, workspace: str | None) -> set[str]:
    text = " ".join((command, title, *paths))
    words = _words(command)
    tags: set[str] = set()
    if workspace is not None and any(_outside_path(path, workspace) for path in [*paths, *_paths_in_text(command, title)]):
        tags.add("outside_workspace")
    if _recursive_rm(words) or "--recursive" in words or _recursive_change(command):
        tags.add("recursive")
    if _many(command) and _changes(command):
        tags.add("bulk")
    if _deletes(command) or _discards(command):
        tags.add("irreversible")
    if (
        _external_url(text)
        or _fetches_remote(command)
        or _matches(command, r"\bgit\s+(?:fetch|pull|push|clone|remote)\b|\b(?:ssh|scp|rsync)\b")
    ):
        tags.add("remote")
    if _matches(text, r"\b(?:production|prod)\b"):
        tags.add("production")
    if _credential_source(text):
        tags.add("credential_access")
    if (
        _matches(text, r"\b(?:sudo|doas)\b|\bsetuid\b|/etc/(?:sudoers|systemd)")
        or _matches(text, r"(?:^|[;&|(]\s*)su(?:\s|$)")
        or re.search(OPEN_MODE, text) is not None
    ):
        tags.add("privileged")
    if "$(" in command or "`${" in command or "${" in command or "`" in command:
        tags.add("shell_interpolation")
    # A shell runs what it reads. Python runs it only with no program of its own: `-c` and `-m`
    # give one, and the download is its data.
    if _matches(command, r"\b(?:curl|wget)\b") and _matches(
        command, r"\|\s*(?:sh|bash|zsh)\b|\|\s*python[\d.]*\s*(?:$|[;&|)]|-(?:\s|$))"
    ):
        tags.add("download_and_execute")
    # The address of a request goes to its host: a credential in it is sent.
    if _secret_in_address(text):
        tags.update(("credential_access", "credential_transfer", "unknown_destination"))
    if _uploads(command):
        tags.add("unknown_destination")
        if "credential_access" in tags:
            tags.add("credential_transfer")
    if _forces(command):
        tags.add("force")
    if _matches(command, r"(?:^|\s)(?:nohup|disown)\b") or _matches(command, r"&\s*$"):
        tags.add("background_process")
    if _persists(command, text, kind):
        tags.add("persistent_change")
    return tags


def _rule(command: str, tags: set[str]) -> str | None:
    words = _words(command)
    if "credential_access" in tags and _uploads(command):
        return "credential_transfer"
    if _recursive_rm(words) and any(word in ("/", "/*") for word in words):
        return "root_delete"
    if _recursive_rm(words) and any(
        word in ("~", "$HOME", "${HOME}") or re.fullmatch(r"/home/[^/]+", word) is not None for word in words
    ):
        return "home_delete"
    if _matches(
        command,
        r"--(?:dangerously-)?skip-permissions\b|\bariadne\s+permissions\s+(?:disable|off)\b|"
        r"\b(?:disable|bypass|skip)\s+(?:agent\s+)?(?:permissions?|approvals?)\b|"
        r"\bturn\s+off\s+(?:agent\s+)?(?:permissions?|approvals?)\b|"
        r"\bskip[_-]?confirmations?.{0,24}\b(?:true|1)\b|"
        r"\bapproval_policy.{0,24}\b(?:never|off)\b|"
        r"--(?:allow-all-tools|disable-confirmation)\b",
    ):
        return "permission_tamper"
    # A comment has no effect on the command. One that tells the reviewer to allow the call
    # has only that purpose.
    if any(_matches(comment, REVIEWER_TEXT) for comment in operations.comments(command)):
        return "permission_tamper"
    return None


def _operation(command: str, paths: list[str], title: str, kind: str, tags: set[str]) -> str | None:
    if _matches(command + " " + title, REVIEWER_TEXT):
        return "destructive_or_exfiltration"
    if "credential_transfer" in tags or "download_and_execute" in tags:
        return "destructive_or_exfiltration"
    if "privileged" in tags:
        return "system_privileged"
    if "credential_access" in tags:
        return "secrets_credentials"
    if command:
        local = not _external_url(command) and _matches(command, r"(?:^|[\s/@'\"])" + LOCAL_HOST + r"(?:[:/?#\s'\"]|$)")
        return operations.operation(command, local)
    if _external_url(" ".join(paths)):
        return "network_read"
    if kind in ("read", "search") or _matches(title, r"^read\b"):
        return "read_workspace"
    if kind in ("edit", "write") or _matches(title, r"^(?:write|edit)\b"):
        found = [operations.write_operation(path) for path in paths] or ["write_workspace"]
        return min(found, key=operations.ORDER.index)
    return None


def derive(request: dict[str, Any], workspace: str | None) -> Derived:
    """Return the operation hint, ordered risk tags, and a narrow hard-deny rule."""
    command, paths, title, kind = _parts(request)
    found_tags = _tags(command, paths, title, kind, workspace)
    return Derived(
        _operation(command, paths, title, kind, found_tags),
        [tag for tag in TAGS if tag in found_tags],
        _rule(command, found_tags),
    )
