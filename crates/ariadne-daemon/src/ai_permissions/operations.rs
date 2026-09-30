//! Portable operation hints for shell commands used by AI permission facts.

const ORDER: [&str; 12] = [
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
];

const WRAPPERS: [&str; 10] = [
    "sudo", "doas", "time", "nohup", "command", "exec", "nice", "xargs", "timeout", "env",
];

fn wrapper_value_options(wrapper: &str) -> &'static [&'static str] {
    match wrapper {
        "sudo" => &[
            "-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-T", "-U", "--user", "--group",
            "--chdir", "--host", "--prompt", "--role", "--type",
        ],
        "doas" => &["-u", "-C"],
        "env" => &[
            "-u",
            "-C",
            "-S",
            "-P",
            "--unset",
            "--chdir",
            "--split-string",
        ],
        "nice" => &["-n", "--adjustment"],
        "timeout" => &["-k", "-s", "--kill-after", "--signal"],
        "exec" => &["-a"],
        "xargs" => &["-I", "-n", "-P", "-L", "-s", "-d", "-E", "-a"],
        _ => &[],
    }
}

const SHELLS: [&str; 3] = ["sh", "bash", "zsh"];
const INTERPRETERS: [&str; 10] = [
    "sh", "bash", "zsh", "python", "python3", "node", "ruby", "perl", "deno", "bun",
];
const READS: [&str; 49] = [
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "rg",
    "grep",
    "egrep",
    "fgrep",
    "pwd",
    "echo",
    "printf",
    "which",
    "type",
    "stat",
    "file",
    "du",
    "df",
    "ps",
    "uptime",
    "tree",
    "diff",
    "sort",
    "uniq",
    "cut",
    "tr",
    "jq",
    "date",
    "whoami",
    "id",
    "uname",
    "hostname",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "test",
    "sleep",
    "cd",
    "pushd",
    "popd",
    "column",
    "nl",
    "less",
    "more",
    "md5sum",
    "shasum",
    "sha256sum",
    "lsof",
];
const DELETES: [&str; 5] = ["rm", "rmdir", "unlink", "shred", "wipe"];
const WRITES: [&str; 10] = [
    "mkdir", "touch", "cp", "mv", "ln", "tee", "truncate", "patch", "chown", "chgrp",
];
const CHECKS: [&str; 17] = [
    "tsc",
    "eslint",
    "prettier",
    "ruff",
    "black",
    "mypy",
    "pytest",
    "vitest",
    "jest",
    "rustfmt",
    "shellcheck",
    "flake8",
    "pylint",
    "biome",
    "stylelint",
    "gofmt",
    "golangci-lint",
];
const CHECK_TARGETS: [&str; 13] = [
    "test",
    "tests",
    "build",
    "check",
    "lint",
    "fmt",
    "format",
    "typecheck",
    "type-check",
    "compile",
    "generate",
    "all",
    "bench",
];
const RUN_TARGETS: [&str; 5] = ["dev", "start", "serve", "preview", "watch"];
const HOST_INSTALLERS: [&str; 11] = [
    "brew", "apt", "apt-get", "yum", "dnf", "pacman", "apk", "pipx", "gem", "port", "snap",
];
const PROJECT_INSTALLERS: [&str; 8] = ["npm", "pnpm", "yarn", "bun", "pip", "pip3", "poetry", "uv"];
const INSTALLS: [&str; 12] = [
    "install",
    "i",
    "add",
    "remove",
    "uninstall",
    "rm",
    "update",
    "upgrade",
    "ci",
    "dedupe",
    "sync",
    "lock",
];
const UPLOADS: [&str; 11] = [
    "-d",
    "--data",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "-F",
    "--form",
    "-T",
    "--upload-file",
    "--post-file",
    "--post-data",
];

const GIT_LFS_READS: [&str; 6] = ["ls-files", "status", "env", "version", "logs", "locks"];
const GIT_LFS_FETCHES: [&str; 3] = ["pull", "fetch", "clone"];

const DEPLOYERS: [&str; 13] = [
    "terraform",
    "tofu",
    "pulumi",
    "serverless",
    "sls",
    "vercel",
    "fly",
    "flyctl",
    "heroku",
    "netlify",
    "ansible",
    "ansible-playbook",
    "helm",
];
const DEPLOY_READS: [&str; 19] = [
    "plan", "preview", "show", "output", "state", "status", "logs", "list", "ls", "version",
    "whoami", "env", "get", "history", "diff", "template", "info", "releases", "domains",
];
const DEPLOY_CHECKS: [&str; 4] = ["fmt", "validate", "lint", "check"];

const CLOUDS: [&str; 4] = ["aws", "gcloud", "az", "doctl"];
const CLOUD_READ_PREFIXES: [&str; 10] = [
    "describe", "list", "get", "show", "print-", "ls", "version", "help", "whoami", "download",
];
const CLOUD_VALUE_OPTIONS: &[&str] = &[
    "--profile",
    "--region",
    "--output",
    "-o",
    "--endpoint-url",
    "--query",
    "--color",
    "--cli-binary-format",
    "--ca-bundle",
    "--cli-read-timeout",
    "--cli-connect-timeout",
    "--project",
    "--account",
    "--configuration",
    "--format",
    "--zone",
    "--impersonate-service-account",
    "--billing-project",
    "--verbosity",
    "--subscription",
    "--resource-group",
    "-g",
    "--name",
    "-n",
    "--location",
    "-l",
    "--file",
    "-f",
    "--container-name",
    "-c",
    "--account-name",
    "--account-key",
    "--sas-token",
    "--acl",
    "--sse",
    "--sse-c",
    "--sse-c-key",
    "--sse-kms-key-id",
    "--storage-class",
    "--exclude",
    "--include",
    "--content-type",
    "--content-encoding",
    "--content-disposition",
    "--content-language",
    "--cache-control",
    "--expires",
    "--metadata",
    "--metadata-directive",
    "--grants",
    "--source-region",
    "--request-payer",
    "--expected-size",
    "--page-size",
    "--max-items",
    "--starting-token",
    "--filters",
    "--filter",
    "--context",
    "--function-name",
    "--bucket",
    "--key",
    "--body",
    "--payload",
    "--cluster",
    "--service",
    "--stack-name",
    "--template-file",
    "--parameters",
    "--tags",
    "--tag",
    "-t",
    "--image",
    "--machine-type",
    "--size",
    "--count",
    "--instance-ids",
    "--table-name",
    "--queue-url",
    "--topic-arn",
    "--message",
    "--role-arn",
    "--policy-arn",
    "--user-name",
    "--group-name",
    "--secret-id",
    "--parameter-name",
    "--vault-name",
    "--id",
];
const CLOUD_TRANSFERS: [&str; 6] = ["cp", "sync", "mv", "rsync", "upload", "upload-batch"];
const CLOUD_CHANGES: &[&str] = &[
    "create",
    "delete",
    "update",
    "set",
    "add",
    "remove",
    "deploy",
    "run",
    "start",
    "stop",
    "apply",
    "invoke",
    "publish",
    "terminate",
    "restart",
    "reboot",
    "import",
    "export",
    "enable",
    "disable",
    "attach",
    "detach",
    "put",
    "rm",
    "mb",
    "rb",
    "login",
    "logout",
    "revoke",
    "activate",
    "deactivate",
    "copy",
    "move",
    "scale",
    "resize",
    "submit",
    "cancel",
    "execute",
    "send",
    "register",
    "deregister",
    "configure",
    "init",
    "push",
    "pull",
    "tag",
    "untag",
    "reset",
    "rotate",
    "grant",
    "assign",
    "ssh",
    "scp",
    "exec",
    "up",
    "browse",
    "open",
    "purge",
    "wait",
];

const SSH_VALUE_OPTIONS: [&str; 14] = [
    "-i", "-p", "-o", "-l", "-L", "-R", "-D", "-J", "-F", "-b", "-c", "-e", "-m", "-w",
];

// A change of a file under one of these directories, or of a shell startup file, changes the host.
const SYSTEM_DIRECTORIES: [&str; 10] = [
    "/etc/",
    "/usr/",
    "/bin/",
    "/sbin/",
    "/System/",
    "/Library/",
    "/var/",
    "/opt/",
    "/boot/",
    "/private/etc/",
];
const STARTUP_FILES: [&str; 8] = [
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".zlogin",
];
const LOCK_FILES: [&str; 10] = [
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "poetry.lock",
    "uv.lock",
    "Gemfile.lock",
    "go.sum",
    "composer.lock",
];

pub(super) fn rank(operation: &str) -> usize {
    ORDER.iter().position(|found| *found == operation).unwrap()
}

fn scan(command: &str) -> (Vec<String>, Vec<String>) {
    let mut parts = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let mut part = String::new();
    let mut quote = None;
    let mut comment = false;
    let chars = command.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if comment {
            comment = character != '\n';
            if comment {
                comments.last_mut().expect("comment exists").push(character);
            } else {
                parts.push(std::mem::take(&mut part));
            }
        } else if let Some(mark) = quote {
            if character == '\\' && mark == '"' {
                index += 1;
            } else if character == mark {
                quote = None;
                part.push('_');
            }
        } else if character == '\\' {
            index += 1;
            part.push('_');
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '#' && part.chars().last().is_none_or(char::is_whitespace) {
            comment = true;
            comments.push(String::new());
        } else if matches!(character, '&' | '|')
            && chars.get(index + 1).is_some_and(|next| *next == character)
        {
            parts.push(std::mem::take(&mut part));
            index += 1;
        } else if matches!(character, ';' | '|' | '\n') {
            parts.push(std::mem::take(&mut part));
        } else {
            part.push(character);
        }
        index += 1;
    }
    parts.push(part);
    (
        parts
            .into_iter()
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        comments,
    )
}

fn simple_commands(command: &str) -> Vec<String> {
    scan(command).0
}

pub(super) fn comments(command: &str) -> Vec<String> {
    scan(command).1
}

fn changes_the_host(path: &str) -> bool {
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    SYSTEM_DIRECTORIES
        .iter()
        .any(|directory| path.starts_with(directory))
        || STARTUP_FILES.contains(&name)
}

pub(super) fn write_operation(path: &str) -> &'static str {
    if changes_the_host(path) {
        "system_privileged"
    } else if LOCK_FILES.contains(&path.rsplit('/').next().unwrap_or_default()) {
        "dependency_change"
    } else {
        "write_workspace"
    }
}

fn redirect_target<'a>(words: &'a [&str]) -> Option<&'a str> {
    for (index, word) in words.iter().enumerate() {
        let mark = word.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        if !mark.starts_with('>') {
            continue;
        }
        let target = mark.trim_start_matches('>');
        let target = if target.is_empty() {
            words.get(index + 1).copied().unwrap_or_default()
        } else {
            target
        };
        if !target.is_empty() && !target.starts_with('&') && target != "/dev/null" {
            return Some(target);
        }
    }
    None
}

fn first_plain<'a>(words: &'a [&str]) -> &'a str {
    words
        .iter()
        .copied()
        .find(|word| !word.starts_with('-'))
        .unwrap_or_default()
}

fn first_of<'a>(words: &'a [&str], groups: &[&[&str]]) -> &'a str {
    words
        .iter()
        .copied()
        .find(|word| groups.iter().any(|group| group.contains(word)))
        .unwrap_or_default()
}

/// The words of a git command from its subcommand on: the global options
/// before it are dropped, and `-C <path>` and `-c <name>=<value>` each take
/// the next word as their value.
pub(super) fn git_words<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let mut rest = words;
    while rest.first().is_some_and(|word| word.starts_with('-')) {
        rest = if matches!(rest[0], "-C" | "-c") {
            drop_front(rest, 2)
        } else {
            drop_front(rest, 1)
        };
    }
    rest
}

fn git(words: &[&str]) -> Option<&'static str> {
    const READS: [&str; 19] = [
        "status",
        "diff",
        "log",
        "show",
        "blame",
        "rev-parse",
        "merge-base",
        "ls-files",
        "ls-tree",
        "describe",
        "shortlog",
        "reflog",
        "grep",
        "cat-file",
        "rev-list",
        "for-each-ref",
        "count-objects",
        "whatchanged",
        "name-rev",
    ];
    const FETCHES: [&str; 3] = ["fetch", "clone", "ls-remote"];
    const CHANGES: [&str; 24] = [
        "add",
        "commit",
        "checkout",
        "switch",
        "restore",
        "merge",
        "rebase",
        "reset",
        "cherry-pick",
        "revert",
        "push",
        "pull",
        "rm",
        "mv",
        "clean",
        "am",
        "apply",
        "init",
        "worktree",
        "submodule",
        "update-index",
        "subtree",
        "bundle",
        "gc",
    ];
    let rest = git_words(words);
    let (&subcommand, arguments) = rest.split_first()?;
    if subcommand == "lfs" {
        let action = first_plain(arguments);
        return Some(if GIT_LFS_READS.contains(&action) {
            "read_workspace"
        } else if GIT_LFS_FETCHES.contains(&action) {
            "network_read"
        } else {
            "version_control_mutation"
        });
    }
    if READS.contains(&subcommand) {
        return Some("read_workspace");
    }
    if FETCHES.contains(&subcommand) {
        return Some("network_read");
    }
    let list_options: &[&str] = match subcommand {
        "branch" => &[
            "-a",
            "-r",
            "-v",
            "-vv",
            "-l",
            "--list",
            "--all",
            "--remotes",
            "--show-current",
            "--merged",
            "--no-merged",
        ],
        "tag" => &["-l", "--list", "-n"],
        "remote" => &["-v", "--verbose", "show", "get-url"],
        "stash" => &["list", "show"],
        "config" => &["--get", "--get-all", "--get-regexp", "--list", "-l"],
        _ => &[],
    };
    if !list_options.is_empty() {
        if arguments.is_empty() && subcommand != "stash"
            || arguments
                .first()
                .is_some_and(|argument| list_options.contains(argument))
            || subcommand == "config"
                && arguments
                    .iter()
                    .any(|argument| list_options.contains(argument))
        {
            return Some("read_workspace");
        }
        if subcommand == "config"
            && arguments
                .iter()
                .any(|argument| matches!(*argument, "--global" | "--system"))
        {
            return Some("system_privileged");
        }
        return Some("version_control_mutation");
    }
    CHANGES
        .contains(&subcommand)
        .then_some("version_control_mutation")
}

fn script_target(program: &str, words: &[&str]) -> Option<&'static str> {
    let target = first_plain(words);
    if CHECK_TARGETS.contains(&target) || program == "make" && target.is_empty() {
        Some("build_test")
    } else if RUN_TARGETS.contains(&target) {
        Some("local_execution")
    } else if target == "clean" {
        Some("delete_workspace")
    } else if target == "install" && matches!(program, "make" | "just") {
        Some("system_privileged")
    } else {
        None
    }
}

fn package_manager(program: &str, words: &[&str]) -> Option<&'static str> {
    const TESTS: [&str; 2] = ["test", "t"];
    const RUNS: [&str; 2] = ["run", "run-script"];
    const READS: [&str; 4] = ["ls", "list", "show", "freeze"];
    const FETCHES: [&str; 5] = ["outdated", "view", "info", "audit", "search"];
    let subcommand = first_of(
        words,
        &[&INSTALLS, &TESTS, &RUNS, &READS, &FETCHES, &["publish"]],
    );
    if INSTALLS.contains(&subcommand) {
        if words.contains(&"--dry-run") {
            Some("network_read")
        } else if words.iter().any(|word| matches!(*word, "-g" | "--global")) {
            Some("system_privileged")
        } else {
            Some("dependency_change")
        }
    } else if TESTS.contains(&subcommand) {
        Some("build_test")
    } else if RUNS.contains(&subcommand) {
        let index = words.iter().position(|word| *word == subcommand).unwrap();
        script_target(program, &words[index + 1..])
    } else if READS.contains(&subcommand) {
        Some("read_workspace")
    } else if FETCHES.contains(&subcommand) {
        Some("network_read")
    } else if subcommand == "publish" {
        Some("external_mutation")
    } else if matches!(program, "yarn" | "pnpm" | "bun") {
        script_target(program, words)
    } else {
        None
    }
}

fn cargo(words: &[&str]) -> Option<&'static str> {
    const CHECKS: [&str; 8] = [
        "test", "check", "build", "clippy", "fmt", "bench", "doc", "nextest",
    ];
    const READS: [&str; 3] = ["tree", "metadata", "version"];
    const CHANGES: [&str; 4] = ["add", "remove", "update", "fetch"];
    let subcommand = first_of(
        words,
        &[
            &CHECKS,
            &READS,
            &CHANGES,
            &["run", "install", "publish", "clean"],
        ],
    );
    if CHECKS.contains(&subcommand) {
        Some("build_test")
    } else if READS.contains(&subcommand) {
        Some("read_workspace")
    } else if CHANGES.contains(&subcommand) {
        Some(if words.contains(&"--dry-run") {
            "network_read"
        } else {
            "dependency_change"
        })
    } else {
        match subcommand {
            "run" => Some("local_execution"),
            "install" => Some("system_privileged"),
            "publish" => Some("external_mutation"),
            "clean" => Some("delete_workspace"),
            _ => None,
        }
    }
}

fn go(words: &[&str]) -> Option<&'static str> {
    const CHECKS: [&str; 5] = ["test", "build", "vet", "fmt", "generate"];
    const READS: [&str; 3] = ["list", "version", "env"];
    let subcommand = first_of(words, &[&CHECKS, &READS, &["run", "get", "mod", "install"]]);
    if CHECKS.contains(&subcommand) {
        Some("build_test")
    } else if READS.contains(&subcommand) {
        Some("read_workspace")
    } else {
        match subcommand {
            "run" => Some("local_execution"),
            "get" | "mod" => Some("dependency_change"),
            "install" => Some("system_privileged"),
            _ => None,
        }
    }
}

fn fetch(words: &[&str], local: bool) -> &'static str {
    let changes = words.iter().any(|word| {
        UPLOADS.contains(word)
            || word.starts_with("--data")
            || word.starts_with("--post-")
            || matches!(*word, "POST" | "PUT" | "PATCH" | "DELETE")
    });
    match (local, changes) {
        (true, true) => "local_execution",
        (true, false) => "read_workspace",
        (false, true) => "external_mutation",
        (false, false) => "network_read",
    }
}

fn find(words: &[&str]) -> Option<&'static str> {
    if words.contains(&"-delete") {
        return Some("delete_workspace");
    }
    for option in ["-exec", "-execdir", "-ok"] {
        if let Some(index) = words.iter().position(|word| *word == option) {
            return simple_operation(&words[index + 1..], false);
        }
    }
    Some("read_workspace")
}

fn ssh(words: &[&str]) -> Option<&'static str> {
    if words.iter().any(|word| {
        matches!(*word, "-L" | "-R" | "-D")
            || ["-L", "-R", "-D"]
                .iter()
                .any(|prefix| word.starts_with(prefix))
    }) {
        return Some("external_mutation");
    }
    let mut rest = words;
    while rest.first().is_some_and(|word| word.starts_with('-')) {
        rest = drop_front(
            rest,
            if SSH_VALUE_OPTIONS.contains(&rest[0]) {
                2
            } else {
                1
            },
        );
    }
    if rest.is_empty() {
        return None;
    }
    Some(if rest.len() > 1 {
        "external_mutation"
    } else {
        "network_read"
    })
}

fn cloud_operands<'a>(words: &'a [&'a str]) -> Vec<&'a str> {
    let mut operands = Vec::new();
    let mut rest = words;
    while let Some(&word) = rest.first() {
        if word.starts_with('-') {
            rest = drop_front(
                rest,
                if CLOUD_VALUE_OPTIONS.contains(&word) {
                    2
                } else {
                    1
                },
            );
        } else {
            operands.push(word);
            rest = drop_front(rest, 1);
        }
    }
    operands
}

fn cloud(words: &[&str]) -> &'static str {
    let operands = cloud_operands(words);
    let mut action: Vec<&str> = Vec::new();
    for &word in &operands {
        if !word.chars().all(|c| c.is_alphanumeric() || c == '-') {
            break;
        }
        action.push(word);
        if CLOUD_READ_PREFIXES
            .iter()
            .any(|prefix| word.starts_with(prefix))
            || CLOUD_TRANSFERS.contains(&word)
            || CLOUD_CHANGES.contains(&word)
        {
            break;
        }
    }
    if matches!(action.last(), Some(&"cp") | Some(&"sync") | Some(&"rsync")) {
        let rest = drop_front(&operands, action.len());
        let source = rest.first().copied().unwrap_or("");
        let destination = rest.get(1).copied().unwrap_or("");
        if source.contains("://") && !destination.is_empty() && !destination.contains("://") {
            return "network_read";
        }
    }
    let reads = action.iter().any(|word| {
        CLOUD_READ_PREFIXES
            .iter()
            .any(|prefix| word.starts_with(prefix))
    });
    if reads || words.contains(&"--dry-run") {
        "network_read"
    } else {
        "external_mutation"
    }
}

fn drop_front<'a>(words: &'a [&'a str], n: usize) -> &'a [&'a str] {
    words.get(n..).unwrap_or(&[])
}

/// The words of a simple command from its program on: the assignments and the
/// wrappers before it are dropped, with the options of each wrapper and the
/// value of an option that takes one. With `shells` false, a shell with a
/// script (`bash -c`) is the program, and its words stay.
pub(super) fn program_words<'a>(words: &'a [&'a str], shells: bool) -> &'a [&'a str] {
    let mut words = words;
    while let Some(&first) = words.first() {
        if first.contains('=') && !first.starts_with(['-', '/', '.']) {
            words = drop_front(words, 1);
        } else if WRAPPERS.contains(&first) {
            let values = wrapper_value_options(first);
            words = drop_front(words, 1);
            while words.first().is_some_and(|word| word.starts_with('-')) {
                words = drop_front(words, if values.contains(&words[0]) { 2 } else { 1 });
            }
            if first == "timeout" {
                words = drop_front(words, 1);
            }
        } else if shells && SHELLS.contains(&first) && words.get(1).copied() == Some("-c") {
            words = drop_front(words, 2);
        } else {
            break;
        }
    }
    words
}

fn simple_operation<'a>(words: &'a [&'a str], local: bool) -> Option<&'static str> {
    let words = program_words(words, true);
    if words.is_empty() {
        return Some("read_workspace");
    }
    let target = redirect_target(words);
    let found = program_operation(words[0], &words[1..], local);
    let (Some(target), Some(found_operation)) = (target, found) else {
        return found;
    };
    let written = write_operation(target);
    Some(if rank(written) < rank(found_operation) {
        written
    } else {
        found_operation
    })
}

fn program_operation(program: &str, arguments: &[&str], local: bool) -> Option<&'static str> {
    let subcommand = first_plain(arguments);
    if program == "find" {
        return find(arguments);
    }
    if program == "sed" {
        return Some(
            if arguments.iter().any(|argument| argument.starts_with("-i")) {
                "write_workspace"
            } else {
                "read_workspace"
            },
        );
    }
    if READS.contains(&program) {
        return Some("read_workspace");
    }
    if DELETES.contains(&program) {
        return Some("delete_workspace");
    }
    if program == "chmod" {
        return Some(
            if arguments.iter().any(|argument| argument.ends_with("+x")) {
                "local_execution"
            } else {
                "write_workspace"
            },
        );
    }
    if WRITES.contains(&program) {
        return Some(
            arguments
                .iter()
                .rev()
                .find(|argument| !argument.starts_with('-'))
                .map_or("write_workspace", |path| write_operation(path)),
        );
    }
    if program == "git" {
        return git(arguments);
    }
    if program == "cargo" {
        return cargo(arguments);
    }
    if program == "go" {
        return go(arguments);
    }
    if HOST_INSTALLERS.contains(&program) {
        return if INSTALLS.contains(&subcommand) {
            Some("system_privileged")
        } else if ["list", "info", "search", "show"].contains(&subcommand) {
            Some("read_workspace")
        } else {
            None
        };
    }
    if PROJECT_INSTALLERS.contains(&program)
        && (program != "bun"
            || INSTALLS.contains(&subcommand)
            || matches!(subcommand, "run" | "test"))
    {
        let words = if program == "uv" && subcommand == "pip" {
            &arguments[1..]
        } else {
            arguments
        };
        return package_manager(program, words);
    }
    if matches!(program, "make" | "just") {
        return script_target(program, arguments);
    }
    if CHECKS.contains(&program) {
        return Some("build_test");
    }
    if matches!(program, "curl" | "wget" | "http" | "https") {
        return Some(fetch(arguments, local));
    }
    if program == "gh" {
        let action = arguments
            .iter()
            .skip(1)
            .find(|argument| !argument.starts_with('-'))
            .copied()
            .unwrap_or_default();
        if subcommand == "api" {
            let changes = arguments.iter().any(|argument| {
                [
                    "-X",
                    "--method",
                    "-f",
                    "-F",
                    "--field",
                    "--raw-field",
                    "--input",
                ]
                .contains(argument)
            });
            return Some(if changes {
                "external_mutation"
            } else {
                "network_read"
            });
        }
        if ["pr", "issue", "run", "repo", "release", "workflow"].contains(&subcommand) {
            return Some(
                if ["view", "list", "diff", "checks", "status"].contains(&action) {
                    "network_read"
                } else {
                    "external_mutation"
                },
            );
        }
        return None;
    }
    if program == "kubectl" {
        let reads = ["get", "describe", "logs", "top", "version"];
        let changes = [
            "apply", "delete", "exec", "scale", "rollout", "patch", "edit", "create", "replace",
            "drain", "cordon",
        ];
        let action = first_of(arguments, &[&reads, &changes]);
        return (!action.is_empty()).then_some(if reads.contains(&action) {
            "network_read"
        } else {
            "external_mutation"
        });
    }
    if matches!(program, "docker" | "podman" | "nerdctl") {
        let reads = ["ps", "images", "logs", "inspect", "version", "info"];
        let fetches = ["pull", "search"];
        let runs = [
            "build", "run", "exec", "compose", "start", "stop", "restart",
        ];
        let sends = ["push", "login"];
        let action = first_of(arguments, &[&reads, &fetches, &runs, &sends]);
        if reads.contains(&action) {
            return Some("read_workspace");
        }
        if fetches.contains(&action) {
            return Some("network_read");
        }
        if sends.contains(&action) {
            return Some("external_mutation");
        }
        return runs.contains(&action).then_some("local_execution");
    }
    if program == "ssh" {
        return ssh(arguments);
    }
    if program == "direnv" {
        return Some(if subcommand == "allow" {
            "local_execution"
        } else {
            "read_workspace"
        });
    }
    if program == "tar" {
        let mode = arguments
            .first()
            .map_or("", |first| first.trim_start_matches('-'));
        return Some(if mode.starts_with('t') || mode.starts_with("list") {
            "read_workspace"
        } else {
            "write_workspace"
        });
    }
    if DEPLOYERS.contains(&program) {
        if DEPLOY_CHECKS.contains(&subcommand) {
            return Some("build_test");
        }
        return Some(
            if DEPLOY_READS.contains(&subcommand)
                || arguments.contains(&"--check")
                || arguments.contains(&"--dry-run")
            {
                "network_read"
            } else {
                "external_mutation"
            },
        );
    }
    if CLOUDS.contains(&program) {
        return Some(cloud(arguments));
    }
    if matches!(
        (program, subcommand),
        ("sqlx", "migrate") | ("alembic", "upgrade") | ("diesel", "migration")
    ) {
        return Some("local_execution");
    }
    if program == "sqlite3" && arguments.contains(&"-readonly") {
        return Some("read_workspace");
    }
    if program == "crontab" {
        return Some(if arguments == ["-l"] {
            "read_workspace"
        } else {
            "system_privileged"
        });
    }
    if matches!(program, "scp" | "rsync") {
        return Some(if arguments.iter().any(|argument| argument.contains(':')) {
            "external_mutation"
        } else {
            "write_workspace"
        });
    }
    if INTERPRETERS.contains(&program)
        || program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.') == "python"
    {
        if subcommand.is_empty() || arguments.is_empty() || arguments[0].starts_with('-') {
            if matches!(arguments, ["-m", "pytest", ..] | ["-m", "unittest", ..]) {
                return Some("build_test");
            }
            return (arguments.starts_with(&["-m", "json.tool"])).then_some("read_workspace");
        }
        return Some("local_execution");
    }
    if program.starts_with("./")
        || program.starts_with("../")
        || program.contains('/') && !program.starts_with('/')
    {
        return Some("local_execution");
    }
    None
}

pub(super) fn operation(command: &str, local: bool) -> Option<&'static str> {
    let parts = simple_commands(command);
    let mut found = parts
        .iter()
        .map(|part| {
            let words = part.split_whitespace().collect::<Vec<_>>();
            simple_operation(&words, local)
        })
        .collect::<Vec<_>>();
    if command.contains("$(") || command.contains('`') || command.contains("<(") {
        found.push(None);
    }
    let largest = found
        .iter()
        .flatten()
        .min_by_key(|operation| rank(operation))
        .copied()?;
    let known = found.iter().filter(|operation| operation.is_some()).count();
    if known < found.len() && rank(largest) >= 7 {
        None
    } else {
        Some(largest)
    }
}
