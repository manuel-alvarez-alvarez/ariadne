"""Derive portable risk facts from the visible parts of a permission request.

The daemon's Rust port intentionally uses the same small collection of token and substring
tests. This module does not inspect paths on disk and does not parse shell syntax.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

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
CREDENTIAL_SOURCE = re.compile(
    r"(?:\.aws/credentials|\.ssh/(?:id_[^/\s'\"]+|config)|\.env(?:\.[^/\s'\"]*)?|"
    r"\.npmrc|\.pypirc|/etc/(?:shadow|passwd)|credentials?\.json|\bfind-generic-password\b)",
    re.IGNORECASE,
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


def _tags(command: str, paths: list[str], title: str, workspace: str | None) -> set[str]:
    text = " ".join((command, title, *paths))
    words = _words(command)
    tags: set[str] = set()
    if workspace is not None and any(_outside_path(path, workspace) for path in [*paths, *_paths_in_text(command, title)]):
        tags.add("outside_workspace")
    if _recursive_rm(words) or "--recursive" in words:
        tags.add("recursive")
    if "*" in command or _matches(command, r"\b(?:xargs|find)\b"):
        tags.add("bulk")
    if _matches(command, r"\b(?:rm|rmdir|unlink|shred|wipe)\b") or _matches(command, r"\bgit\s+(?:reset\s+--hard|clean\b)"):
        tags.add("irreversible")
    if URL.search(text) or _matches(command, r"\bgit\s+(?:fetch|pull|push|clone|remote)\b|\b(?:curl|wget|ssh|scp|rsync)\b"):
        tags.add("remote")
    if _matches(text, r"\b(?:production|prod)\b"):
        tags.add("production")
    if CREDENTIAL_SOURCE.search(text):
        tags.add("credential_access")
    if _matches(text, r"\b(?:sudo|doas|su)\b|\bchmod\s+[0-7]*[67][0-7]*\b|\bsetuid\b|/etc/(?:sudoers|systemd)"):
        tags.add("privileged")
    if "$(" in command or "`${" in command or "${" in command or "`" in command:
        tags.add("shell_interpolation")
    if _matches(command, r"\b(?:curl|wget)\b") and _matches(command, r"\|\s*(?:sh|bash|zsh|python\d*)\b"):
        tags.add("download_and_execute")
    external_target = URL.search(command) is not None or _matches(command, r"\b[A-Za-z0-9._-]+@[A-Za-z0-9.-]+:")
    transfer = external_target and _matches(
        command, r"\b(?:curl|wget)\b.*(?:-d|--data|--upload-file|-T)\b|\b(?:scp|rsync|nc|ncat)\b"
    )
    if transfer:
        tags.add("unknown_destination")
    if "credential_access" in tags and transfer:
        tags.add("credential_transfer")
    if _matches(command, r"(?:^|\s)--force(?:\s|$)|(?:^|\s)-[a-z]*f[a-z]*(?:\s|$)"):
        tags.add("force")
    if _matches(command, r"(?:^|\s)(?:nohup|disown)\b") or _matches(command, r"&\s*$"):
        tags.add("background_process")
    if _matches(text, r"\b(?:crontab|launchctl)\b|\bsystemctl\s+enable\b|\.git/hooks/|\.(?:bashrc|zshrc|profile)\b"):
        tags.add("persistent_change")
    return tags


def _rule(command: str, tags: set[str]) -> str | None:
    words = _words(command)
    if "credential_transfer" in tags:
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
    return None


def _operation(command: str, paths: list[str], title: str, kind: str, tags: set[str]) -> str | None:
    text = (command + " " + title).lower()
    if "credential_transfer" in tags or "download_and_execute" in tags:
        return "destructive_or_exfiltration"
    if "privileged" in tags:
        return "system_privileged"
    if "credential_access" in tags:
        return "secrets_credentials"
    if _matches(command, r"\b(?:rm|rmdir|unlink|shred|wipe)\b"):
        return "delete_workspace"
    if _matches(command, r"\bgit\s+(?:push|commit|merge|rebase|reset|branch\s+-[dD]|tag\s+-d)\b"):
        return "version_control_mutation"
    if _matches(command, r"\b(?:npm|pnpm|yarn|pip|pipx)\s+(?:install|add|remove|uninstall)\b|\bcargo\s+(?:add|remove|update)\b"):
        return "dependency_change"
    if _matches(command, r"\b(?:cargo|npm|pnpm|yarn)\s+(?:test|check|build|run)\b|\b(?:pytest|unittest|vitest)\b"):
        return "build_test"
    if _matches(command, r"\b(?:curl|wget)\b.*(?:-d|--data|--upload-file|-T)\b|\b(?:scp|rsync)\b"):
        return "external_mutation"
    if _matches(command, r"\bgit\s+(?:fetch|pull|clone)\b|\b(?:curl|wget)\b") or URL.search(text):
        return "network_read"
    if kind == "read" or _matches(title, r"^read\b"):
        return "read_workspace"
    if kind in ("edit", "write") or _matches(title, r"^(?:write|edit)\b"):
        return "write_workspace"
    if kind == "execute" and _matches(command, r"^\s*(?:echo|pwd|ls|cat|rg|grep|find)\b"):
        return "local_execution"
    return None


def derive(request: dict[str, Any], workspace: str | None) -> Derived:
    """Return the operation hint, ordered risk tags, and a narrow hard-deny rule."""
    command, paths, title, kind = _parts(request)
    found_tags = _tags(command, paths, title, workspace)
    return Derived(
        _operation(command, paths, title, kind, found_tags),
        [tag for tag in TAGS if tag in found_tags],
        _rule(command, found_tags),
    )
