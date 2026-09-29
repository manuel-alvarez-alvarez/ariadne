"""The operation of a command line, from the first words of each of its simple commands.

`operation(command)` splits the line at `&&`, `||`, `;`, `|` and the line end, outside quotes,
and gives each simple command the operation of its program and subcommand. The line takes the
operation with the largest effect. A simple command that no table names makes the line
ambiguous: the line then has an operation only if a known part already has a large effect.

Each test is a lookup of plain words, so the Rust port can give the same result. Nothing here
reads the file system.
"""
from __future__ import annotations

# The order of the effect, largest first. A line takes the first operation that a part has.
ORDER = (
    "destructive_or_exfiltration",
    "system_privileged",
    "secrets_credentials",
    "external_mutation",
    "delete_workspace",
    "version_control_mutation",
    "dependency_change",
    "network_read",
    "local_execution",
    "write_workspace",
    "build_test",
    "read_workspace",
)
# An ambiguous line keeps a known operation only from this list.
LARGE = ORDER[:7]

# Programs that run the rest of the simple command. `timeout` also takes a duration.
WRAPPERS = ("sudo", "doas", "time", "nohup", "command", "exec", "nice", "xargs", "timeout")
READS = (
    "ls", "cat", "head", "tail", "wc", "rg", "grep", "egrep", "fgrep", "pwd", "echo", "printf", "which", "type",
    "stat", "file", "du", "df", "ps", "uptime", "tree", "diff", "sort", "uniq", "cut", "tr", "jq", "date", "whoami",
    "id", "uname", "hostname", "basename", "dirname", "realpath", "readlink", "test", "sleep", "cd", "pushd",
    "popd", "column", "nl", "less", "more", "md5sum", "shasum", "sha256sum", "lsof",
)  # fmt: skip
DELETES = ("rm", "rmdir", "unlink", "shred", "wipe")
WRITES = ("mkdir", "touch", "cp", "mv", "ln", "tee", "truncate", "patch", "chown", "chgrp")
CHECKS = (
    "tsc", "eslint", "prettier", "ruff", "black", "mypy", "pytest", "vitest", "jest", "rustfmt", "shellcheck",
    "flake8", "pylint", "biome", "stylelint", "gofmt", "golangci-lint",
)  # fmt: skip
CHECK_TARGETS = (
    "test", "tests", "build", "check", "lint", "fmt", "format", "typecheck", "type-check", "compile", "generate",
    "all", "bench",
)  # fmt: skip
RUN_TARGETS = ("dev", "start", "serve", "preview", "watch")
INTERPRETERS = ("sh", "bash", "zsh", "python", "python3", "node", "ruby", "perl", "deno", "bun")
MIGRATIONS = (("sqlx", "migrate"), ("alembic", "upgrade"), ("diesel", "migration"))
HOST_INSTALLERS = ("brew", "apt", "apt-get", "yum", "dnf", "pacman", "apk", "pipx", "gem", "port", "snap")
PROJECT_INSTALLERS = ("npm", "pnpm", "yarn", "bun", "pip", "pip3", "poetry", "uv")
INSTALLS = ("install", "i", "add", "remove", "uninstall", "rm", "update", "upgrade", "ci", "dedupe", "sync", "lock")
UPLOADS = (
    "-d", "--data", "--data-binary", "--data-raw", "--data-urlencode", "-F", "--form", "-T", "--upload-file",
    "--post-file", "--post-data",
)  # fmt: skip
CHANGING_METHODS = ("POST", "PUT", "PATCH", "DELETE")

GIT_READS = (
    "status", "diff", "log", "show", "blame", "rev-parse", "merge-base", "ls-files", "ls-tree", "describe",
    "shortlog", "reflog", "grep", "cat-file", "rev-list", "for-each-ref", "count-objects", "whatchanged", "name-rev",
)  # fmt: skip
GIT_FETCHES = ("fetch", "clone", "ls-remote")
GIT_CHANGES = (
    "add", "commit", "checkout", "switch", "restore", "merge", "rebase", "reset", "cherry-pick", "revert", "push",
    "pull", "rm", "mv", "clean", "am", "apply", "init", "worktree", "submodule", "update-index", "subtree", "bundle",
    "gc",
)  # fmt: skip
GIT_LISTS = {
    "branch": ("-a", "-r", "-v", "-vv", "-l", "--list", "--all", "--remotes", "--show-current", "--merged", "--no-merged"),
    "tag": ("-l", "--list", "-n"),
    "remote": ("-v", "--verbose", "show", "get-url"),
    "stash": ("list", "show"),
    "config": ("--get", "--get-all", "--get-regexp", "--list", "-l"),
}

GH_READS = ("view", "list", "diff", "checks", "status")
KUBECTL_READS = ("get", "describe", "logs", "top", "version")
KUBECTL_CHANGES = ("apply", "delete", "exec", "scale", "rollout", "patch", "edit", "create", "replace", "drain", "cordon")
DOCKER_READS = ("ps", "images", "logs", "inspect", "version", "info")
DOCKER_RUNS = ("build", "run", "exec", "compose", "start", "stop", "restart")
DOCKER_SENDS = ("push", "login")
PACKAGE_READS = ("ls", "list", "show", "freeze")
PACKAGE_FETCHES = ("outdated", "view", "info", "audit", "search")
PACKAGE_RUNS = ("run", "run-script")
PACKAGE_TESTS = ("test", "t")
CARGO_CHECKS = ("test", "check", "build", "clippy", "fmt", "bench", "doc", "nextest")
CARGO_READS = ("tree", "metadata", "version")
CARGO_CHANGES = ("add", "remove", "update", "fetch")
CARGO_OTHERS = {"run": "local_execution", "install": "system_privileged", "publish": "external_mutation", "clean": "delete_workspace"}
GO_CHECKS = ("test", "build", "vet", "fmt", "generate")
GO_READS = ("list", "version", "env")
GO_OTHERS = {"run": "local_execution", "get": "dependency_change", "mod": "dependency_change", "install": "system_privileged"}

# A change of a file under one of these directories, or of a shell startup file, changes the host.
SYSTEM_DIRECTORIES = (
    "/etc/", "/usr/", "/bin/", "/sbin/", "/System/", "/Library/", "/var/", "/opt/", "/boot/", "/private/etc/",
)  # fmt: skip
STARTUP_FILES = (".bashrc", ".bash_profile", ".bash_login", ".profile", ".zshrc", ".zshenv", ".zprofile", ".zlogin")
LOCK_FILES = (
    "Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lockb", "poetry.lock", "uv.lock",
    "Gemfile.lock", "go.sum", "composer.lock",
)  # fmt: skip


def _scan(command: str) -> tuple[list[str], list[str]]:
    """The simple commands of a line, and its comments. Each quoted part of a simple command
    is `_`. A `#` that starts a word outside quotes starts a comment, which goes to the line
    end."""
    parts: list[str] = []
    comments: list[str] = []
    part: list[str] = []
    quote = ""
    comment = False
    index = 0
    while index < len(command):
        character = command[index]
        if comment:
            comment = character != "\n"
            if comment:
                comments[-1] += character
            else:
                parts.append("".join(part))
                part = []
        elif quote:
            if character == "\\" and quote == '"':
                index += 1
            elif character == quote:
                quote = ""
                part.append("_")
        elif character == "\\":
            index += 1
            part.append("_")
        elif character in "'\"":
            quote = character
        elif character == "#" and (not part or part[-1].isspace()):
            comment = True
            comments.append("")
        elif command.startswith(("&&", "||"), index):
            parts.append("".join(part))
            part = []
            index += 1
        elif character in ";|\n":
            parts.append("".join(part))
            part = []
        else:
            part.append(character)
        index += 1
    parts.append("".join(part))
    return [found for found in (one.strip() for one in parts) if found], comments


def simple_commands(command: str) -> list[str]:
    """The simple commands of a line: see `_scan`."""
    return _scan(command)[0]


def comments(command: str) -> list[str]:
    """The text of each shell comment of a line: see `_scan`."""
    return _scan(command)[1]


def changes_the_host(path: str) -> bool:
    """A path of the operating system, or a shell startup file."""
    name = path.rstrip("/").rsplit("/", 1)[-1]
    return path.startswith(SYSTEM_DIRECTORIES) or name in STARTUP_FILES


def write_operation(path: str) -> str:
    """The operation of a change of the file `path`."""
    if changes_the_host(path):
        return "system_privileged"
    if path.rsplit("/", 1)[-1] in LOCK_FILES:
        return "dependency_change"
    return "write_workspace"


def _redirect_target(words: list[str]) -> str | None:
    """The file that a simple command writes with `>` or `>>`, or `None`. A copy of one
    stream to another (`2>&1`) and `/dev/null` are not files."""
    for index, word in enumerate(words):
        mark = word.lstrip("0123456789&")
        if not mark.startswith(">"):
            continue
        target = mark.lstrip(">") or (words[index + 1] if index + 1 < len(words) else "")
        if target and not target.startswith("&") and target != "/dev/null":
            return target
    return None


def _first_plain(words: list[str]) -> str:
    return next((word for word in words if not word.startswith("-")), "")


def _first_of(words: list[str], *names: tuple[str, ...] | dict[str, str]) -> str:
    """The first word that is a subcommand of one of `names`. An option and its value can come
    before the subcommand (`npm --prefix ui run build`), so the position does not name it."""
    return next((word for word in words if any(word in known for known in names)), "")


def _git(words: list[str]) -> str | None:
    # `-C <path>` and `-c <name>=<value>` come before the subcommand, each with a value.
    rest = list(words)
    while rest and rest[0].startswith("-"):
        rest = rest[2:] if rest[0] in ("-C", "-c") else rest[1:]
    if not rest:
        return None
    subcommand, arguments = rest[0], rest[1:]
    if subcommand in GIT_READS:
        return "read_workspace"
    if subcommand in GIT_FETCHES:
        return "network_read"
    if subcommand in GIT_LISTS:
        # `git branch` and `git tag` list with no argument. `git stash` changes with none.
        if arguments[:1] == [] and subcommand != "stash":
            return "read_workspace"
        if arguments[:1] != [] and arguments[0] in GIT_LISTS[subcommand]:
            return "read_workspace"
        if subcommand == "config" and any(argument in GIT_LISTS["config"] for argument in arguments):
            return "read_workspace"
        if subcommand == "config" and ("--global" in arguments or "--system" in arguments):
            return "system_privileged"
        return "version_control_mutation"
    return "version_control_mutation" if subcommand in GIT_CHANGES else None


def _script_target(program: str, words: list[str]) -> str | None:
    """`run <script>` of a JavaScript package manager, `make <target>` and `just <recipe>`."""
    target = _first_plain(words)
    if target in CHECK_TARGETS or (program == "make" and not target):
        return "build_test"
    if target in RUN_TARGETS:
        return "local_execution"
    if target == "clean":
        return "delete_workspace"
    if target == "install" and program in ("make", "just"):
        return "system_privileged"
    return None


def _package_manager(program: str, words: list[str]) -> str | None:
    subcommand = _first_of(words, INSTALLS, PACKAGE_TESTS, PACKAGE_RUNS, PACKAGE_READS, PACKAGE_FETCHES, ("publish",))
    if subcommand in INSTALLS:
        if "--dry-run" in words:
            return "network_read"
        return "system_privileged" if "-g" in words or "--global" in words else "dependency_change"
    if subcommand in PACKAGE_TESTS:
        return "build_test"
    if subcommand in PACKAGE_RUNS:
        return _script_target(program, words[words.index(subcommand) + 1 :])
    if subcommand in PACKAGE_READS:
        return "read_workspace"
    if subcommand in PACKAGE_FETCHES:
        return "network_read"
    if subcommand == "publish":
        return "external_mutation"
    if program in ("yarn", "pnpm", "bun"):
        return _script_target(program, words)
    return None


def _cargo(words: list[str]) -> str | None:
    subcommand = _first_of(words, CARGO_CHECKS, CARGO_READS, CARGO_CHANGES, CARGO_OTHERS)
    if subcommand in CARGO_CHECKS:
        return "build_test"
    if subcommand in CARGO_READS:
        return "read_workspace"
    if subcommand in CARGO_CHANGES:
        return "network_read" if "--dry-run" in words else "dependency_change"
    return CARGO_OTHERS.get(subcommand)


def _go(words: list[str]) -> str | None:
    subcommand = _first_of(words, GO_CHECKS, GO_READS, GO_OTHERS)
    if subcommand in GO_CHECKS:
        return "build_test"
    if subcommand in GO_READS:
        return "read_workspace"
    return GO_OTHERS.get(subcommand)


def _fetch(words: list[str], local: bool) -> str:
    """`curl`, `wget` and `http`: an upload option or a changing method changes the service."""
    changes = any(word in UPLOADS or word.startswith(("--data", "--post-")) for word in words) or any(
        word in CHANGING_METHODS for word in words
    )
    if local:
        return "local_execution" if changes else "read_workspace"
    return "external_mutation" if changes else "network_read"


def _find(words: list[str]) -> str | None:
    if "-delete" in words:
        return "delete_workspace"
    for option in ("-exec", "-execdir", "-ok"):
        if option in words:
            return simple_operation(words[words.index(option) + 1 :], local=False)
    return "read_workspace"


def simple_operation(words: list[str], local: bool) -> str | None:
    """The operation of one simple command, or `None` for a program that no table names.
    `local` says that the only host of the line is the local host."""
    while words and ("=" in words[0] and not words[0].startswith(("-", "/", "."))):
        words = words[1:]
    while words and words[0] in WRAPPERS:
        words = words[2:] if words[0] == "timeout" else words[1:]
    if not words:
        return "read_workspace"
    target = _redirect_target(words)
    program, arguments = words[0], words[1:]
    found = _program_operation(program, arguments, local)
    if target is None or found is None:
        return found
    written = write_operation(target)
    return written if ORDER.index(written) < ORDER.index(found) else found


def _program_operation(program: str, arguments: list[str], local: bool) -> str | None:
    subcommand = _first_plain(arguments)
    if program == "find":
        return _find(arguments)
    if program == "sed":
        return "write_workspace" if any(argument.startswith("-i") for argument in arguments) else "read_workspace"
    if program in READS:
        return "read_workspace"
    if program in DELETES:
        return "delete_workspace"
    if program == "chmod":
        return "local_execution" if any(argument.endswith("+x") for argument in arguments) else "write_workspace"
    if program in WRITES:
        paths = [argument for argument in arguments if not argument.startswith("-")]
        return write_operation(paths[-1]) if paths else "write_workspace"
    if program == "git":
        return _git(arguments)
    if program == "cargo":
        return _cargo(arguments)
    if program == "go":
        return _go(arguments)
    if program in HOST_INSTALLERS:
        if subcommand in INSTALLS:
            return "system_privileged"
        return "read_workspace" if subcommand in ("list", "info", "search", "show") else None
    if program in PROJECT_INSTALLERS and (program != "bun" or subcommand in INSTALLS or subcommand in ("run", "test")):
        return _package_manager(program, arguments[1:] if program == "uv" and subcommand == "pip" else arguments)
    if program in ("make", "just"):
        return _script_target(program, arguments)
    if program in CHECKS:
        return "build_test"
    if program in ("curl", "wget", "http", "https"):
        return _fetch(arguments, local)
    if program == "gh":
        action = next((argument for argument in arguments[1:] if not argument.startswith("-")), "")
        if subcommand == "api":
            changes = any(argument in ("-X", "--method", "-f", "-F", "--field", "--raw-field", "--input") for argument in arguments)
            return "external_mutation" if changes else "network_read"
        if subcommand in ("pr", "issue", "run", "repo", "release", "workflow"):
            return "network_read" if action in GH_READS else "external_mutation"
        return None
    if program == "kubectl":
        action = _first_of(arguments, KUBECTL_READS, KUBECTL_CHANGES)
        if action == "":
            return None
        return "network_read" if action in KUBECTL_READS else "external_mutation"
    if program in ("docker", "podman"):
        action = _first_of(arguments, DOCKER_READS, DOCKER_RUNS, DOCKER_SENDS)
        if action in DOCKER_READS:
            return "read_workspace"
        if action in DOCKER_SENDS:
            return "external_mutation"
        return "local_execution" if action in DOCKER_RUNS else None
    if (program, subcommand) in MIGRATIONS:
        return "local_execution"
    if program == "sqlite3" and "-readonly" in arguments:
        return "read_workspace"
    if program == "crontab":
        return "read_workspace" if arguments == ["-l"] else "system_privileged"
    if program in ("scp", "rsync"):
        return "external_mutation" if any(":" in argument for argument in arguments) else "write_workspace"
    if program in INTERPRETERS or program.rstrip("0123456789.") == "python":
        if subcommand == "" or not arguments or arguments[0].startswith("-"):
            if arguments[:2] == ["-m", "pytest"] or arguments[:2] == ["-m", "unittest"]:
                return "build_test"
            return "read_workspace" if arguments[:2] == ["-m", "json.tool"] else None
        return "local_execution"
    if program.startswith(("./", "../")) or ("/" in program and not program.startswith("/")):
        return "local_execution"
    return None


def operation(command: str, local: bool = False) -> str | None:
    """The operation of a command line: see the module text."""
    found = [simple_operation(part.split(), local) for part in simple_commands(command)]
    if "$(" in command or "`" in command or "<(" in command:
        # A substitution runs one more command, which no part shows.
        found.append(None)
    known = [one for one in found if one is not None]
    if not known:
        return None
    largest = min(known, key=ORDER.index)
    if len(known) < len(found) and largest not in LARGE:
        return None
    return largest
