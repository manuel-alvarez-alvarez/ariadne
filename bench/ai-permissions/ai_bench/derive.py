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
    "reviewer_directive",
    "permission_bypass",
    "root_or_home_delete",
)

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
# The part of the reviewer text with only one purpose: to address a reader. The bare phrases
# `always allow` and `please allow` are left out, so a commit message or a search pattern that
# holds them is not a directive; a shell comment is a directive on the full text.
REVIEWER_DIRECTIVE = (
    r"\b(?:ignore|disregard)\s+(?:all\s+|any\s+)?(?:the\s+)?(?:previous|prior|above|earlier)\s+(?:instructions?|rules?|prompts?)\b|"
    r"\bnote\s+to\s+(?:the\s+)?reviewer\b|\ballow\s+without\s+asking\b|\bpre-?approved\b"
)
# A program that emits its argument as output or content. Directive text after one is produced
# for a reader, not consumed as a search pattern.
EMITTER = r"(?:^|[\s;&|(])(?:echo|printf|print|tee|cat)\b"
# The start of a program name in a line, also in a quoted command.
START = r"(?:^|[\s;&|('\"`])"
# Options of one simple command, up to a given option.
OPTIONS = r"\s+(?:[^;&|]*\s)?"
# One word of a shell line: characters outside quotes, an escaped character (`my\ project`), and
# a quoted span (`'/repo/my project'`, `"less -R"`), in any mix.
WORD = "(?:[^\\s\\\\'\"]|\\\\.|'[^']*'|\"[^\"]*\")+"
# The word `git` and its global options, up to its subcommand: `git -C /repo/project push`,
# `git -c core.pager=cat log`, `git --no-pager grep`. `-C` and `-c` take the next word, which can
# be quoted or hold an escaped space (`git -C '/repo/my project' grep`).
GIT = r"\bgit(?:\s+-[Cc]\s+" + WORD + r"|\s+-" + WORD + r")*\s+"
CREDENTIAL_SOURCE = re.compile(
    r"(?:\.aws/credentials|\.ssh/(?:id_[^/\s'\"]+|config)|"
    r"\.npmrc|\.pypirc|/etc/(?:shadow|passwd)|credentials?\.json|"
    r"\.git-credentials|\.netrc|\.kube/config|\.docker/config\.json|\.gnupg|--export-secret-keys?\b|"
    r"\bdump-keychain\b|\.keychain|\.pem\b|\.key\b|\.p12\b|\bid_(?:rsa|ed25519|ecdsa|dsa|\*)|"
    # More credential stores: cloud and package tokens, keystores, the password store and keyrings.
    r"\.pgpass|\.terraform\.d/credentials|\.cargo/credentials|\.gem/credentials|\.composer/auth\.json|"
    r"\.m2/settings\.xml|gradle\.properties|\.jks\b|\.pfx\b|\.p8\b|\.keystore\b|\.password-store|"
    r"/Keychains/|/keyrings/|\.config/gh/hosts\.yml|\.config/gcloud/|\.azure/|"
    # A secrets file by name, and the Terraform variables and state, which carry the values.
    r"\bsecrets\.(?:ya?ml|json|toml|env|enc|txt|properties|ini|cfg|conf|xml|plist)\b|\.tfvars\b|\.tfstate\b)",
    re.IGNORECASE,
)
# A dump of the environment, in the command: `env`, `printenv`, `set` or `export -p` alone, to a
# file, at the end of the line, or piped to a program that sends or encodes it.
ENVIRONMENT_DUMP = re.compile(
    r"(?:^|[;&|(]\s*)(?:env|printenv|set|export\s+-p)\s*(?:$|[>;&]|\|\s*(?:curl|wget|nc|ncat|socat|tee|base64|xxd|gzip)\b)",
    re.IGNORECASE,
)
# A program that prints a credential or a secret: a token command of a host tool, a secret manager,
# a cloud or keychain access, a read of secrets, or an environment variable that names a secret.
# It is tested on the program words of each simple command (see `_action_parts`), so the same
# words in the pattern of a `grep` are not one.
CREDENTIAL_PROGRAM = re.compile(
    r"^(?:gh\s+auth\s+(?:token\b|status[^;&|]*--show-token\b)|op\s+read\b|"
    r"vault\s+(?:read|kv\s+get)\b|gcloud\s+auth\s+print-(?:access|identity)-token\b|"
    r"az\s+account\s+get-access-token\b|aws\s+configure\s+get\b|"
    r"aws\s+sts\s+(?:get-session-token|get-federation-token|assume-role)\b|"
    + GIT + r"credential(?:-[a-z]+)?\s+(?:fill|get)\b|"
    r"security\s+find-(?:generic|internet)-password\b|kubectl\b[^;&|]*\bget\s+secrets?\b|"
    r"(?:printenv|echo)\b[^;&|]*\$?[A-Za-z][A-Za-z0-9_]*"
    r"(?:TOKEN|SECRET|PASSWORD|PASSWD|API[_-]?KEY|ACCESS[_-]?KEY|PRIVATE[_-]?KEY|CREDENTIALS?)\b)",
    re.IGNORECASE,
)
# A program whose arguments are a pattern and paths: the text after it is searched, not run.
SEARCH_PROGRAMS = ("grep", "egrep", "fgrep", "rg", "ag", "ack", "sed", "awk")
# A program that prints its arguments. What it prints runs only when a pipe carries it on.
EMITTERS = ("echo", "printf")
# A dotenv file, with its name. An example of one holds no credential.
DOTENV = re.compile(r"[^\s/'\"=:]*\.env(?:\.[^/\s'\"]*)?", re.IGNORECASE)
DOTENV_EXAMPLE = re.compile(r"example|sample|template", re.IGNORECASE)
# A credential in the query of an address: a name that ends with the word of a secret, and a value.
SECRET_IN_QUERY = re.compile(
    r"(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key)(?:=|%3D)[^&\s'\"]",
    re.IGNORECASE,
)
# A program that installs persistence, changes a host default or service, sets a package or git
# configuration, or points git hooks at a path.
PERSISTENT_PROGRAM = (
    r"\b(?:crontab|launchctl)\b|\bsystemctl\b[^;&|]*\benable\b|\bdefaults\s+write\b|"
    r"\bbrew\s+services\s+(?:start|run|restart)\b|\b(?:npm|pip|pip3)\s+config\s+set\b|"
    r"\bdirenv\s+allow\b|" + GIT + r"lfs\s+install\b|"
    + GIT + r"config\b(?:[^;&|]*\s)?(?:--(?:global|system)\b|[^;&|]*\bcore\.hooks[pP]ath\b)"
)
# A command that destroys data that no undo restores: a raw device or filesystem write, a file
# emptied in place, a history rewrite, a remote or volume deletion, a mirror delete, a database
# drop, or a `DELETE FROM <table>` whose statement ends right after the table, with no `WHERE`.
# It is tested on each simple command that acts (see `_action_parts`), so the pattern of a `rg`
# and a statement that `echo` prints to nothing are not one.
IRREVERSIBLE_EXTRA = (
    r"(?:^|[\s;&|(])dd\b[^;&|]*\sof=/dev/|\bmkfs\b|\bdiskutil\s+(?:erase|reformat)\b|"
    r"\btruncate\b[^;&|]*\s-s\s*0\b|" + GIT + r"filter-(?:repo|branch)\b|" + GIT + r"reflog\s+expire\b|"
    + GIT + r"gc\b[^;&|]*--prune|" + GIT + r"push\b[^;&|]*--delete\b|"
    r"\b(?:docker|podman)\b[^;&|]*\b(?:system\s+prune|volume\s+rm)\b|\bkubectl\b[^;&|]*\bdelete\b|"
    r"\brsync\b[^;&|]*--delete\b|\bredis-cli\b[^;&|]*\bflush(?:all|db)\b|"
    r"\bdrop\s+(?:database|schema|table)\b|\bdropdatabase\b|\btruncate\s+table\b|"
    r"\bdelete\s+from\s+[A-Za-z_][A-Za-z0-9_.\"`\[\]]*\s*(?:;|'|\"|$)"
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
    r"\.(?:claude|codex|cursor|vscode|agent|zed)/(?:hooks/|commands/|agents/|rules|[^\s'\"/]*(?:settings|config|tasks|launch|mcp|memory)[^\s'\"/]*)|"
    r"\.husky/|\.pre-commit-config\.yaml\b|\.assistant\.json\b|\.envrc\b|\.tool-versions\b|opencode\.jsonc?\b|"
    r"\.ssh/(?:authorized_keys|config)\b|\.aws/config\b|(?:AGENTS|CLAUDE)\.md\b|rust-toolchain\b)|"
    r"(?:^|[\s'\"=])/(?:etc|usr|bin|sbin|System|Library|var|opt|boot)/"
)
# The start of a simple command, after any wrapper: the line start or a separator, then `sudo`,
# `env VAR=value`, `timeout N` and the like. Unlike `START`, whitespace and a quote are not
# enough, so a program name that is also a plain word (`host`, `ping`, `mail`) in an argument or
# a message does not count.
COMMAND_START = (
    r"(?:^|[;&|(]\s*)"
    r"(?:(?:sudo|doas|nohup|time|command|exec|nice|env(?:\s+[A-Za-z_][A-Za-z0-9_]*=\S*)*|timeout\s+\S+)\s+)*"
)
# A program at the start of a simple command that reaches a host of its own: a cloud or deploy
# CLI, a remote shell or copy, a name lookup, a mail sender, or a raw socket.
REMOTE_PROGRAM = COMMAND_START + (
    r"(?:gh|glab|kubectl|helm|aws|gcloud|az|doctl|terraform|tofu|pulumi|serverless|sls|vercel|fly|flyctl|"
    r"heroku|netlify|ansible|ansible-playbook|rclone|sftp|telnet|dig|nslookup|host|ping|"
    r"mail|mailx|sendmail|ssmtp|twine|rancher|nomad|sshpass|nc|ncat|socat)(?:\s|$)"
)
# A container image with a registry host: `registry.example/tool:1.0`, `ghcr.io/org/tool`. A run
# of one downloads and executes it; a pull downloads it.
REGISTRY_IMAGE = r"\b(?:docker|podman|nerdctl)\b[^;&|]*\b(?:run|create|pull)\b[^;&|]*\s[a-z0-9.-]+\.[a-z]{2,}(?::\d+)?/[^\s'\"]+"
REGISTRY_RUN = r"\b(?:docker|podman|nerdctl)\b[^;&|]*\b(?:run|create)\b[^;&|]*\s[a-z0-9.-]+\.[a-z]{2,}(?::\d+)?/[^\s'\"]+"
# A character that a reader does not see: a zero-width or joining character, a bidirectional
# control, a byte-order mark, or a tag character.
HIDDEN = re.compile("[​-‏ -‮⁠-⁤⁦-⁩﻿\U000e0000-\U000e007f]")
# A package manager that installs, updates, publishes or fetches over the network. `docker push`
# and `docker login` send to a registry.
PACKAGE_REMOTE = (
    r"\b(?:npm|pnpm|yarn|bun|pip|pip3|pipx|poetry|uv|cargo|go|gem|brew|apt|apt-get|yum|dnf|pacman|apk|"
    r"composer|mvn|gradle|dotnet)\b[^;&|]*\b(?:install|add|update|upgrade|ci|publish|deploy|dlx|"
    r"outdated|audit)\b|\b(?:docker|podman)\b[^;&|]*\b(?:push|login)\b"
)
# A launcher that downloads code and runs it. `npx` and `bunx` run a local binary when it is
# present, so only `--yes` or `-y`, which installs without a prompt, is a download. `pnpm dlx`,
# `uvx` and `pipx run` always fetch. A process or command substitution can feed a fetch to a shell.
DOWNLOAD_RUN = (
    r"\b(?:npx|bunx)\b[^;&|]*\s-(?:-yes|y)\b|\bpnpm\s+dlx\b|\buvx\b|\bpipx\s+run\b|"
    r"\bcargo\s+install\b[^;&|]*--git\b|"
    r"\b(?:sh|bash|zsh)\s+<\(\s*(?:curl|wget)\b|\b(?:source|\.)\s+<\(\s*(?:curl|wget)\b|"
    r"\b(?:sh|bash|zsh)\s+-c\s+[\"']?\$\(\s*(?:curl|wget)\b|\beval\s+[\"']?\$\(\s*(?:curl|wget)\b|"
    # A package from a URL, a git address or an archive runs its install hooks or its setup.
    r"\b(?:pip|pip3|npm|pnpm|yarn|bun|uv)\b[^;&|]*\b(?:install|add|i)\b[^;&|]*"
    r"(?:https?://|git\+|github:|gitlab:|bitbucket:|\.tgz\b|\.tar\.gz\b|\.whl\b|\.zip\b)|"
    # A fetch extracted by tar, then a run of what it extracted.
    r"\b(?:curl|wget)\b[^|]*\|\s*tar\s+-?[A-Za-z]*x[A-Za-z]*\b[^;&|]*(?:&&|;)[^;&|]*(?:\./|\b(?:sh|bash|make|python[0-9.]*|node)\b)|"
    + REGISTRY_RUN
)
# Text that hides what a command does: a process substitution, `eval` of an expansion, ANSI-C
# quoting, a hex `printf`, an inline interpreter that decodes a payload, or a decoder fed to a
# shell. A plain expansion (`$PATH`) and a search for the word `eval` are not obfuscation.
OBFUSCATION = (
    r"<\(|>\(|(?:^|[\s;&|(])eval\s+[\"'$]|(?:^|[\s=(&|;])\$'|\bprintf\b[^|;&]*\\x[0-9a-fA-F]{2}|"
    r"\b(?:python[0-9.]*|node|perl|ruby)\b[^|;&]*\s-(?:c|e)\b[^|;&]*\b(?:exec|b64decode|atob|fromCharCode)\b|"
    r"\b(?:base64|xxd|openssl|rev|gunzip|gzip)\b[^|]*\|\s*(?:sh|bash|zsh|python[0-9.]*)\b"
)
# A container or host call that takes more privilege than the workspace: a privileged or
# capability-adding container, the host root or the docker socket mounted in, the host process or
# mount namespace entered, a chroot, a polkit run, a capability set, or a change of owner to root.
PRIVILEGED_EXTRA = (
    r"--privileged\b|--cap-add\b|(?:^|\s)-v\s+/:|--pid[= ]host\b|--net(?:work)?[= ]host\b|\bnsenter\b|\bchroot\b|\bpkexec\b|"
    r"\bsetcap\b|docker\.sock|(?:^|[\s;&|(])chown\b[^;&|]*\broot\b"
)
# A process that keeps running after the call: a detached container, a scheduled or submitted job,
# or a new session.
BACKGROUND_EXTRA = (
    r"\bat\s+now\b|\blaunchctl\s+submit\b|\bsystemd-run\b|\bsetsid\b|\bstart_new_session\b|"
    r"\b(?:docker|podman)\b[^;&|]*\s(?:up|run|start)\b[^;&|]*\s-d\b|\bcompose\b[^;&|]*\s-d\b|--detach\b"
)
# An explicit attempt to disable, bypass or turn off agent permissions or confirmations.
DISABLE_PERMISSIONS = (
    r"--(?:dangerously-)?skip-permissions\b|\bariadne\s+permissions\s+(?:disable|off)\b|"
    r"\b(?:disable|bypass|skip)\s+(?:agent\s+)?(?:permissions?|approvals?)\b|"
    r"\bturn\s+off\s+(?:agent\s+)?(?:permissions?|approvals?)\b|"
    r"\bskip[_-]?confirmations?.{0,24}\b(?:true|1)\b|"
    r"\bapproval_policy.{0,24}\b(?:never|off)\b|"
    r"--(?:allow-all-tools|disable-confirmation)\b"
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


def _command(raw_input: dict[str, Any]) -> str:
    command = raw_input.get("command")
    if isinstance(command, str):
        return command
    if isinstance(command, list):
        return " ".join(part for part in command if isinstance(part, str))
    return ""


def _parts(request: dict[str, Any]) -> tuple[str, list[str], str, str, str]:
    tool_call = request.get("toolCall", {}) or {}
    raw_input = tool_call.get("rawInput", {}) or {}
    raw_input = raw_input if isinstance(raw_input, dict) else {}
    command = _command(raw_input)
    paths = [value for key in ("file_path", "path", "url") if isinstance((value := raw_input.get(key)), str)]
    for location in tool_call.get("locations", []) or []:
        if isinstance(location, dict) and isinstance(location.get("path"), str):
            paths.append(location["path"])
    title = tool_call.get("title")
    # The text of every string in the input, the written content included. Only the hidden
    # character test reads it: the other tests read the command, the paths and the title.
    content = " ".join(
        part for value in raw_input.values() for part in (value if isinstance(value, list) else [value]) if isinstance(part, str)
    )
    return command, paths, title if isinstance(title, str) else "", str(tool_call.get("kind", "")), content


def _words(text: str) -> list[str]:
    return [word.strip("'\"()[]{};,|") for word in text.split()]


def _shell_words(text: str) -> list[str]:
    """The words of a line as a shell reads them: split at whitespace outside quotes, with an
    escaped character kept (`my\\ project`) and the quotes dropped, so `git -C '/repo/my project'
    grep` gives `/repo/my project` as one word. Each word is stripped as in `_words`."""
    words: list[str] = []
    word: list[str] = []
    quote = ""
    index = 0
    while index < len(text):
        character = text[index]
        if quote:
            if character == "\\" and quote == '"' and index + 1 < len(text) and text[index + 1] in '"\\$`':
                index += 1
                word.append(text[index])
            elif character == quote:
                quote = ""
            else:
                word.append(character)
        elif character == "\\" and index + 1 < len(text):
            index += 1
            word.append(text[index])
        elif character in "'\"":
            quote = character
        elif character.isspace():
            if word:
                words.append("".join(word))
                word = []
        else:
            word.append(character)
        index += 1
    if word:
        words.append("".join(word))
    return [word.strip("'\"()[]{};,|") for word in words]


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


def _simple_parts(command: str) -> list[tuple[str, str]]:
    """The text of each simple command of a line, each with the mark that ends it: `;`, `&&`,
    `||`, `|` or a line end outside quotes, the start of a substitution (`$(`, a backtick, `<(`,
    `>(`) outside single quotes, or nothing for the last part. Unlike `operations.simple_commands`,
    the quoted text is kept, so the words of `bash -c 'rm -rf /'` are read; a `;` inside
    `echo 'safe; rm -rf /'` splits nothing; the command inside `echo $(rm -rf /)` is a part of
    its own."""
    parts: list[tuple[str, str]] = []
    part: list[str] = []
    quote = ""
    index = 0
    while index < len(command):
        character = command[index]
        if quote != "'" and (character == "`" or (character in "$<>" and command.startswith("(", index + 1))):
            mark = "`" if character == "`" else command[index : index + 2]
            parts.append(("".join(part), mark))
            part = []
            index += len(mark)
            continue
        if quote:
            if character == "\\" and quote == '"' and index + 1 < len(command):
                part.append(character)
                index += 1
            elif character == quote:
                quote = ""
        elif character == "\\" and index + 1 < len(command):
            part.append(character)
            index += 1
        elif character in "'\"":
            quote = character
        elif character in ";|\n" or command.startswith("&&", index):
            mark = command[index : index + 2] if command.startswith(("&&", "||"), index) else character
            parts.append(("".join(part), mark))
            part = []
            index += len(mark)
            continue
        part.append(character)
        index += 1
    parts.append(("".join(part), ""))
    return parts


def _script(text: str) -> str | None:
    """The script that a shell runs (`bash -c 'echo ok; rm -rf /'`, after the wrappers of its
    simple command), without the quotes around it; `None` for a command that is not a shell
    with a script."""
    words = operations.program_words(_shell_words(text), shells=False)
    if words[:1] not in (["sh"], ["bash"], ["zsh"]) or words[1:2] != ["-c"]:
        return None
    match = re.search(r"\s-c\s+", text)
    if match is None:
        return None
    script = text[match.end() :]
    if script[:1] not in ("'", '"'):
        return script
    quote, index = script[0], 1
    while index < len(script) and script[index] != quote:
        index += 2 if script[index] == "\\" and quote == '"' else 1
    return script[1:index]


def _executed_parts(command: str) -> list[tuple[str, str]]:
    """The simple parts of a line (see `_simple_parts`), with each part that is a shell with a
    script replaced by the simple parts of that script: the shell runs the script as a line of
    its own, so `bash -c 'echo ok; rm -rf /'` gives `echo ok` and `rm -rf /`. The last part of
    the script ends with the mark of the shell, as `sh -c "echo 'DROP TABLE users'" | psql`
    pipes what the script prints."""
    parts: list[tuple[str, str]] = []
    for text, mark in _simple_parts(command):
        script = _script(text)
        if script is None or not script.strip():
            parts.append((text, mark))
            continue
        inner = _executed_parts(script)
        parts.extend(inner[:-1])
        parts.append((inner[-1][0], mark))
    return parts


def _action_parts(command: str) -> list[str]:
    """The text of each simple command that acts on what it names. The arguments of a search
    program (`grep`, `rg`, `git grep`, `git log`, `sed`, `awk`, …) are a pattern and paths, so
    its part is left out, with the global options of git before its subcommand (`git -C repo
    grep`, `git -C '/repo/my project' grep`) dropped; what `echo` or `printf` prints runs only
    when a pipe carries it to the next command, so its part is left out unless `|` ends it."""
    parts = []
    for text, mark in _executed_parts(command):
        words = operations.program_words(_shell_words(text))
        program = words[0] if words else ""
        if program in SEARCH_PROGRAMS or (program == "git" and operations.git_words(words[1:])[:1] in (["grep"], ["log"])):
            continue
        if program in EMITTERS and mark != "|":
            continue
        parts.append(text)
    return parts


def _root_or_home_delete(command: str) -> bool:
    """A recursive `rm` whose own target is the root directory or the home directory. It is the
    evidence of the `root_delete` and `home_delete` hard rules, as a property of the request, read
    from the words after `rm` where `rm` is the program of its simple command, in the line or in
    the script of a shell (`bash -c 'echo ok; rm -rf /'`): the `~` of `find ~ -exec rm -rf {} +`
    is the start of the search, not the target of the deletion, and the `rm -rf /` that `echo`
    prints, with or without a `;` in the quoted text, deletes nothing."""
    for text, _ in _executed_parts(command):
        words = operations.program_words(_shell_words(text))
        if words[:1] != ["rm"] or not _recursive_rm(words):
            continue
        targets = words[1:]
        if any(
            word in ("/", "/*", "~", "$HOME", "${HOME}") or re.fullmatch(r"/home/[^/]+", word) is not None
            for word in targets
        ):
            return True
    return False


def _outside_path(path: str, workspace: str) -> bool:
    candidate = path.rstrip("/.,:;\"'")
    if path.rstrip(".,:;\"'") == "/":
        return True
    # A volume specification (`/repo/project:/src`) names the host path before the colon.
    if candidate.startswith("/") and ":/" in candidate:
        candidate = candidate.split(":/", 1)[0] or "/"
    if candidate in ("~", "$HOME", "${HOME}") or candidate.startswith("~") or candidate.startswith("$HOME/"):
        return True
    if candidate.startswith("../") or candidate == "..":
        return True
    # A `..` segment in the middle of a path can climb above the workspace; a portable test cannot
    # resolve it, so any interior `..` counts as an escape.
    if "/../" in candidate or candidate.endswith("/.."):
        return True
    if not candidate.startswith("/"):
        return False
    root = workspace.rstrip("/")
    return candidate != root and not candidate.startswith(root + "/")


def _is_path(word: str) -> bool:
    return word.startswith(("/", "~", "$HOME/", "../")) or word in ("..", "$HOME", "${HOME}") or "/../" in word


def _outside_text(command: str, title: str, workspace: str) -> bool:
    """A path outside the workspace among the words of the command and the title, read as a
    shell reads them (see `_outside_words`)."""
    return _outside_words(_shell_words(command + " " + title), workspace)


def _outside_words(words: list[str], workspace: str) -> bool:
    """A path outside the workspace among shell words. A word with no space is one path or no
    path. A word with a space came from a quoted text: a path with a space, or a script. It is
    read as a line of its own, one level down, so the quoted paths inside it stay whole
    (`bash -c 'cat "/repo/my project/file"'` names the one path `/repo/my project/file`) and
    the pattern `'/// A summary'` names no path. When the whole word is a path inside the
    workspace, its first word is the head of that path or the workspace tool that starts a
    script, and the words after it are read: `'/repo/my project/notes.md'` in that workspace
    names nothing outside, and `bash -c '/repo/project/tool /tmp/secret'` names
    `/tmp/secret`."""
    for word in words:
        if not any(character.isspace() for character in word):
            if _is_path(word) and _outside_path(word, workspace):
                return True
            continue
        inner = _shell_words(word)
        if _is_path(word) and not _outside_path(word, workspace):
            inner = inner[1:]
        if _outside_words(inner, workspace):
            return True
    return False


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


def _credential_program(command: str) -> bool:
    """A credential program as the program of a simple command, or of a substitution, of the
    line or of the script of a shell, with the global options of git dropped (`git -C '/repo/my
    project' credential fill`). The same words in the pattern of a search are text."""
    for text, _ in _executed_parts(command):
        words = operations.program_words(_shell_words(text))
        if words[:1] == ["git"]:
            words = ["git", *operations.git_words(words[1:])]
        if CREDENTIAL_PROGRAM.search(" ".join(words)) is not None:
            return True
    return False


def _credential_source(text: str, command: str) -> bool:
    """A known credential source or program, or a dotenv file that is not an example."""
    return (
        CREDENTIAL_SOURCE.search(text) is not None
        or ENVIRONMENT_DUMP.search(command) is not None
        or _credential_program(command)
        or any(DOTENV_EXAMPLE.search(name) is None for name in DOTENV.findall(text))
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
        _matches(command, GIT + r"(?:reset\s+--hard|clean\b|stash\s+(?:clear|drop)\b)")
        or _matches(command, GIT + r"checkout" + OPTIONS + r"(?:--\s|\.(?:\s|$))")
        or _matches(command, GIT + r"push" + OPTIONS + r"(?:--force|--mirror|-[a-z]*f[a-z]*(?:\s|$))")
        or re.search(GIT + r"branch" + OPTIONS + r"-D\b", command) is not None
    )


def _forces(command: str) -> bool:
    """A force option. A short option with `f` is one only for a program that has a force
    option: `-f` names a file for `psql`, and a pattern for `pkill`."""
    return (
        _matches(command, r"(?:^|\s)--force(?:-with-lease)?(?:[\s=]|$)")
        or re.search(START + r"(?:rm|cp|mv|ln)" + OPTIONS + r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)", command) is not None
        or re.search(
            GIT + r"(?:push|checkout|clean|branch|tag|fetch|switch|rm|mv)" + OPTIONS + r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)", command
        )
        is not None
        or re.search(GIT + r"branch" + OPTIONS + r"-D\b", command) is not None
        or re.search(START + r"(?:kill|pkill|killall)" + OPTIONS + r"-(?:9|KILL|SIGKILL)\b", command) is not None
    )


def _recursive_change(command: str) -> bool:
    """A mode change, a copy, an upload or a deletion of a tree."""
    return (
        re.search(START + r"(?:chmod|chown|chgrp)" + OPTIONS + r"-[A-Za-z]*R", command) is not None
        or re.search(START + r"(?:cp|scp)" + OPTIONS + r"-[A-Za-z]*[rRa]", command) is not None
        or re.search(START + r"(?:rsync|zip)" + OPTIONS + r"-[A-Za-z]*[ra]", command) is not None
        or _matches(command, r"\bfind\b[^;&|]*\s-delete\b")
        or _matches(command, GIT + r"clean" + OPTIONS + r"-[a-z]*d")
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
        or _matches(command, GIT + r"(?:add|checkout|restore|clean|push|rm|reset|stash\s+clear)\b")
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


def _tags(command: str, paths: list[str], title: str, kind: str, workspace: str | None, content: str = "") -> set[str]:
    text = " ".join((command, title, *paths))
    words = _words(command)
    tags: set[str] = set()
    if workspace is not None and (
        any(_outside_path(path, workspace) for path in paths) or _outside_text(command, title, workspace)
    ):
        tags.add("outside_workspace")
    if _recursive_rm(words) or "--recursive" in words or _recursive_change(command):
        tags.add("recursive")
    if _many(command) and _changes(command):
        tags.add("bulk")
    if _deletes(command) or _discards(command) or any(_matches(part, IRREVERSIBLE_EXTRA) for part in _action_parts(command)):
        tags.add("irreversible")
    if (
        _external_url(text)
        or _fetches_remote(command)
        or _matches(command, GIT + r"(?:lfs\s+)?(?:fetch|pull|push|clone|remote)\b|\b(?:ssh|scp|rsync)\b")
        or _matches(command, REMOTE_PROGRAM)
        or _matches(command, PACKAGE_REMOTE)
        or _matches(command, REGISTRY_IMAGE)
    ):
        tags.add("remote")
    if _matches(text, r"\b(?:production|prod)\b"):
        tags.add("production")
    if _credential_source(text, command):
        tags.add("credential_access")
    if (
        _matches(text, r"\b(?:sudo|doas)\b|\bsetuid\b|/etc/(?:sudoers|systemd)")
        or _matches(text, r"(?:^|[;&|(]\s*)su(?:\s|$)")
        or re.search(OPEN_MODE, text) is not None
        or _matches(command, PRIVILEGED_EXTRA)
    ):
        tags.add("privileged")
    if (
        "$(" in command
        or "`${" in command
        or "${" in command
        or "`" in command
        or _matches(command, OBFUSCATION)
        or HIDDEN.search(command + " " + title + " " + content) is not None
    ):
        tags.add("shell_interpolation")
    # A shell runs what it reads. Python runs it only with no program of its own: `-c` and `-m`
    # give one, and the download is its data. A runner that fetches its package, and a fetch fed
    # to a shell through a substitution, also download and run.
    if (
        _matches(command, r"\b(?:curl|wget)\b")
        and _matches(command, r"\|\s*(?:sh|bash|zsh)\b|\|\s*python[\d.]*\s*(?:$|[;&|)]|-(?:\s|$))")
    ) or _matches(command, DOWNLOAD_RUN):
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
    if (
        _matches(command, r"(?:^|\s)(?:nohup|disown)\b")
        or _matches(command, r"&\s*$")
        or _matches(command, BACKGROUND_EXTRA)
    ):
        tags.add("background_process")
    if _persists(command, text, kind):
        tags.add("persistent_change")
    if (
        any(_matches(comment, REVIEWER_TEXT) for comment in operations.comments(command))
        or (_matches(command, REVIEWER_TEXT) and _matches(command, EMITTER))
        or (not command and _matches(title, REVIEWER_DIRECTIVE))
    ):
        tags.add("reviewer_directive")
    if _matches(command, DISABLE_PERMISSIONS):
        tags.add("permission_bypass")
    if _root_or_home_delete(command):
        tags.add("root_or_home_delete")
    return tags


def _operation(command: str, paths: list[str], title: str, kind: str, tags: set[str]) -> str | None:
    if "reviewer_directive" in tags or "credential_transfer" in tags or "download_and_execute" in tags:
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
    """Return the operation hint and the ordered risk tags."""
    command, paths, title, kind, content = _parts(request)
    found_tags = _tags(command, paths, title, kind, workspace, content)
    return Derived(_operation(command, paths, title, kind, found_tags), [tag for tag in TAGS if tag in found_tags])
