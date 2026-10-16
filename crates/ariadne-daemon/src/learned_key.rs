//! The key a learned permission is found by (021, rule 9): the fields of the
//! tool's input that say what it does, with the values that change from one
//! request to the next replaced by placeholders. Two requests that differ only
//! by a commit, a worktree or a description share one key.

use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde_json::{Map, Value};

use crate::ai_permissions::operations::{git_words, simple_commands, wrapper_value_options};

/// The paths and names of one session that a request may carry.
#[derive(Debug, Clone, Default)]
pub(crate) struct Facts {
    /// The repository's checkout.
    pub(crate) repository: String,
    /// The session's working directory: the task's worktree.
    pub(crate) worktree: String,
    /// The session's branch, where it works on a task: the task's.
    pub(crate) branch: Option<String>,
    pub(crate) home: String,
}

/// A learned permission's key, and the command family it falls in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) key: String,
    pub(crate) family: String,
    pub(crate) family_safe: bool,
}

/// The tools whose input is a file and its contents: only the file counts.
const FILE_TOOLS: [&str; 5] = ["Edit", "Write", "Read", "MultiEdit", "NotebookEdit"];
/// The fields of an MCP tool's input that are prose, not what it does.
const MCP_PROSE: [&str; 5] = ["body", "summary", "description", "title", "reason"];
/// The programs of an output filter that a trailing pipe chain may hold.
const FILTERS: [&str; 6] = ["tail", "head", "wc", "cat", "sort", "uniq"];
/// The wrappers a family skips to reach the program.
const WRAPPERS: [&str; 8] = [
    "sudo", "env", "time", "nohup", "timeout", "nice", "command", "exec",
];
/// The options of `time` that take a value; the operation hints have none.
const TIME_VALUES: [&str; 4] = ["-f", "-o", "--format", "--output"];
/// The programs whose family names their first plain argument too.
const SUBCOMMANDS: [&str; 14] = [
    "git", "cargo", "npm", "npx", "pnpm", "yarn", "go", "docker", "gh", "make", "kubectl", "pip",
    "pip3", "brew",
];
/// Options that select a program or change output for each program.
const PROGRAM_SETUP_OPTIONS: [(&str, &[&str]); 6] = [
    ("cargo", &["--config"]),
    ("go", &["-exec", "-toolexec"]),
    ("make", &["--eval", "-f", "--file", "--makefile"]),
    ("npm", &["--script-shell", "--node-options"]),
    ("pnpm", &["--script-shell", "--node-options"]),
    ("yarn", &["--script-shell", "--node-options"]),
];
/// Unsafe Git options before the subcommand.
const GIT_GLOBAL_SETUP_OPTIONS: [&str; 3] = ["-c", "--config-env", "--exec-path"];
/// Unsafe Git options after the named subcommand.
const GIT_SUBCOMMAND_SETUP_OPTIONS: [(&[&str], &[&str]); 7] = [
    (&["grep"], &["-O", "--open-files-in-pager"]),
    (&["diff", "log"], &["--output"]),
    (&["format-patch"], &["-o", "--output"]),
    (
        &["fetch", "clone", "ls-remote"],
        &["--upload-pack", "--receive-pack", "-u"],
    ),
    (&["push"], &["--receive-pack"]),
    (&["rebase"], &["-x", "--exec"]),
    (&["difftool"], &["-x", "--extcmd"]),
];

/// The key and the family of a request for `tool_name` with `raw_input`.
pub(crate) fn normalize(tool_name: &str, raw_input: &Value, facts: &Facts) -> Key {
    let mut kept = fields(tool_name, raw_input);
    if tool_name == "Bash"
        && let Some(Value::String(command)) = kept.get_mut("command")
    {
        *command = bash_command(command, facts);
    }
    let family_safe = tool_name != "Bash"
        || kept
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(is_safe_family_command);
    let kept = placeholders(&kept, facts);
    let family = match (tool_name, kept.get("command")) {
        ("Bash", Some(Value::String(command))) => {
            command_family(command).unwrap_or_else(|| tool_name.to_string())
        }
        _ => tool_name.to_string(),
    };
    Key {
        key: sorted(&kept).to_string(),
        family,
        family_safe,
    }
}

/// The fields of the input that the key keeps, by tool.
fn fields(tool_name: &str, raw_input: &Value) -> Value {
    let Value::Object(input) = raw_input else {
        return raw_input.clone();
    };
    let keep = |keep: &dyn Fn(&str) -> bool| {
        Value::Object(
            input
                .iter()
                .filter(|(name, _)| keep(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        )
    };
    match tool_name {
        "Bash" => keep(&|name| name == "command"),
        _ if FILE_TOOLS.contains(&tool_name) => keep(&|name| name == "file_path"),
        _ if tool_name.starts_with("mcp__") => keep(&|name| !MCP_PROSE.contains(&name)),
        "WebFetch" => {
            let host = input
                .get("url")
                .and_then(Value::as_str)
                .and_then(|url| reqwest::Url::parse(url).ok())
                .and_then(|url| url.host_str().map(str::to_string));
            let mut kept = Map::new();
            if let Some(host) = host {
                kept.insert("host".into(), Value::String(host));
            }
            Value::Object(kept)
        }
        "WebSearch" => Value::Object(Map::new()),
        _ => keep(&|name| name != "description"),
    }
}

/// A shell command without what only changes how its output reads: a
/// leading `cd` into the session's own tree, every `2>&1`, and a trailing
/// chain of output filters.
fn bash_command(command: &str, facts: &Facts) -> String {
    static STDERR: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:^|\s+)2>&1\b").expect("valid stderr regex"));
    let command = without_cd(command.trim(), facts);
    let command = STDERR.replace_all(command, "");
    without_filters(command.trim()).trim().to_string()
}

/// The command after a leading `cd <path>` and its `&&`, `;` or newline,
/// where the path is the worktree, the repository or a directory under one.
fn without_cd<'a>(command: &'a str, facts: &Facts) -> &'a str {
    static CD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"^cd\s+('[^']*'|"[^"]*"|[^\s;&|'"]+)[ \t]*(?:&&|;|\n)\s*"#)
            .expect("valid cd regex")
    });
    let Some(found) = CD.captures(command) else {
        return command;
    };
    let path = found[1].trim_matches(|c| c == '\'' || c == '"');
    let path = path
        .strip_suffix('/')
        .filter(|p| !p.is_empty())
        .unwrap_or(path);
    let under = |root: &str| {
        !root.is_empty()
            && (path == root
                || path
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('/')))
    };
    let own = !path.is_empty()
        && !path.split('/').any(|part| part == "..")
        && if path.starts_with('/') {
            under(&facts.worktree) || under(&facts.repository)
        } else {
            // A relative path is under the working directory, the worktree.
            !path.starts_with(['~', '$', '-'])
        };
    if own {
        &command[found[0].len()..]
    } else {
        command
    }
}

/// The command without a trailing pipe chain whose every program is an
/// output filter. A filter that redirects, substitutes or takes a file or
/// program is kept.
fn without_filters(command: &str) -> &str {
    let mut pipes = Vec::new();
    let mut last_other = None;
    let mut quote = None;
    let mut depth = 0usize;
    let mut backtick = false;
    let bytes = command.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(mark) if byte == mark => quote = None,
            Some(b'"') if byte == b'\\' => index += 1,
            Some(_) => {}
            None => match byte {
                b'\\' => index += 1,
                b'$' if bytes
                    .get(index + 1)
                    .is_some_and(|next| matches!(*next, b'\'' | b'"')) =>
                {
                    return command;
                }
                b'\'' | b'"' => quote = Some(byte),
                b'`' => backtick = !backtick,
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                _ if depth > 0 || backtick => {}
                b'|' if bytes.get(index + 1) == Some(&b'|') => {
                    last_other = Some(index + 1);
                    index += 1;
                }
                b'|' => pipes.push(index),
                b';' | b'&' | b'\n' => last_other = Some(index),
                _ => {}
            },
        }
        index += 1;
    }
    if quote.is_some() || depth > 0 || backtick {
        return command;
    }
    let mut cut = command.len();
    for &pipe in pipes.iter().rev() {
        if last_other.is_some_and(|other| other > pipe) {
            break;
        }
        let filter = &command[pipe + 1..cut];
        let plain = !filter.contains(['>', '<', '$', '`', '(', '&', '|']);
        let mut words = filter.split_whitespace();
        let program = words.next().unwrap_or_default();
        let arguments: Vec<_> = words.collect();
        if !plain || !FILTERS.contains(&program) || !filter_arguments_are_inert(program, &arguments)
        {
            break;
        }
        cut = pipe;
    }
    &command[..cut]
}

/// Whether the filter arguments change only how it renders its input.
fn filter_arguments_are_inert(program: &str, arguments: &[&str]) -> bool {
    let counts_lines_or_bytes = matches!(program, "tail" | "head");
    let mut needs_number = false;
    for &argument in arguments {
        if needs_number {
            if is_number(argument) {
                needs_number = false;
                continue;
            }
            return false;
        }
        if counts_lines_or_bytes && argument.strip_prefix('-').is_some_and(is_number) {
            continue;
        }
        if counts_lines_or_bytes && matches!(argument, "-n" | "-c" | "--lines" | "--bytes") {
            needs_number = true;
            continue;
        }
        if is_number(argument) || !filter_flag_is_inert(program, argument) {
            return false;
        }
    }
    !needs_number
}

fn is_number(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `argument` is a flag that changes only how a filter renders input.
/// The lists intentionally exclude every option that can name a file or program.
fn filter_flag_is_inert(program: &str, argument: &str) -> bool {
    let flags = match program {
        "tail" | "head" => &[
            "-c",
            "-n",
            "-q",
            "-s",
            "-v",
            "-z",
            "--bytes",
            "--lines",
            "--quiet",
            "--silent",
            "--verbose",
            "--zero-terminated",
        ][..],
        "wc" => &[
            "-c",
            "-m",
            "-l",
            "-w",
            "-L",
            "--bytes",
            "--chars",
            "--lines",
            "--words",
            "--max-line-length",
        ][..],
        "cat" => &[
            "-A",
            "-b",
            "-e",
            "-E",
            "-n",
            "-s",
            "-t",
            "-u",
            "-v",
            "--number",
            "--number-nonblank",
            "--show-all",
            "--show-ends",
            "--show-nonprinting",
            "--show-tabs",
            "--squeeze-blank",
        ][..],
        "sort" => &[
            "-b",
            "-d",
            "-f",
            "-g",
            "-h",
            "-i",
            "-M",
            "-n",
            "-R",
            "-r",
            "-V",
            "-z",
            "--check",
            "--debug",
            "--dictionary-order",
            "--general-numeric-sort",
            "--human-numeric-sort",
            "--ignore-case",
            "--ignore-leading-blanks",
            "--month-sort",
            "--numeric-sort",
            "--random-sort",
            "--reverse",
            "--stable",
            "--unique",
            "--version-sort",
            "--zero-terminated",
        ][..],
        "uniq" => &[
            "-c",
            "-d",
            "-D",
            "-i",
            "-u",
            "-z",
            "--all-repeated",
            "--count",
            "--group",
            "--ignore-case",
            "--repeated",
            "--unique",
            "--zero-terminated",
        ][..],
        _ => return false,
    };
    flags.contains(&argument)
}

/// A family can cover one simple command with no program setup and its
/// stripped output filters.
fn is_safe_family_command(command: &str) -> bool {
    is_single_family_command(command) && first_command_has_no_program_setup(command)
}

/// The first command does not set an environment or use a Git option that can
/// select a program to run.
fn first_command_has_no_program_setup(command: &str) -> bool {
    let command = without_filters(command).trim();
    let Some(first) = simple_commands(command).into_iter().next() else {
        return false;
    };
    let words: Vec<_> = first.split_whitespace().collect();
    let mut words = words.as_slice();
    while let Some(&word) = words.first() {
        if is_assignment(word) {
            return false;
        }
        if WRAPPERS.contains(&word) {
            let values = match word {
                "time" => &TIME_VALUES[..],
                _ => wrapper_value_options(word),
            };
            words = &words[1..];
            while let Some(&option) = words.first().filter(|word| word.starts_with('-')) {
                if word == "env"
                    && (option.starts_with("-S") || option.starts_with("--split-string"))
                {
                    return false;
                }
                let taken = if values.contains(&option) { 2 } else { 1 };
                words = words.get(taken..).unwrap_or(&[]);
            }
            if word == "timeout" {
                words = words.get(1..).unwrap_or(&[]);
            }
            continue;
        }
        return !program_option_selects_a_program(word, &words[1..]);
    }
    false
}

/// An assignment before the program changes the program environment. This
/// accepts the same broad assignment shape that [`program`] skips.
fn is_assignment(word: &str) -> bool {
    word.contains('=') && !word.starts_with(['-', '/', '.'])
}

/// Options that select a program or output file make a family approval unsafe.
fn program_option_selects_a_program(program: &str, arguments: &[&str]) -> bool {
    if program == "git" {
        return git_option_selects_a_program(arguments);
    }
    if program == "make" && arguments.iter().any(|argument| is_assignment(argument)) {
        return true;
    }
    PROGRAM_SETUP_OPTIONS
        .iter()
        .find(|(name, _)| *name == program)
        .is_some_and(|(_, options)| contains_setup_option(arguments, options))
}

/// Git has separate global and subcommand options that can select code.
fn git_option_selects_a_program(arguments: &[&str]) -> bool {
    let subcommand_and_arguments = git_words(arguments);
    let globals = &arguments[..arguments.len() - subcommand_and_arguments.len()];
    if contains_setup_option(globals, &GIT_GLOBAL_SETUP_OPTIONS) {
        return true;
    }
    let Some((&subcommand, arguments)) = subcommand_and_arguments.split_first() else {
        return false;
    };
    GIT_SUBCOMMAND_SETUP_OPTIONS
        .iter()
        .find(|(subcommands, _)| subcommands.contains(&subcommand))
        .is_some_and(|(_, options)| contains_setup_option(arguments, options))
}

/// The option can appear alone, as `--option=value`, or as an attached short
/// option value.
fn contains_setup_option(arguments: &[&str], options: &[&str]) -> bool {
    arguments.iter().any(|argument| {
        options.iter().any(|option| {
            *argument == *option
                || (option.starts_with("--")
                    && argument
                        .strip_prefix(option)
                        .is_some_and(|value| value.starts_with('=')))
                || (!option.starts_with("--")
                    && argument
                        .strip_prefix(option)
                        .is_some_and(|value| !value.is_empty()))
        })
    })
}

/// A command holds one simple command and its stripped output filters.
fn is_single_family_command(command: &str) -> bool {
    let command = without_filters(command).trim();
    let bytes = command.as_bytes();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(b'\'') if byte == b'\'' => quote = None,
            Some(b'\'') => {}
            Some(b'"') if byte == b'"' => quote = None,
            Some(b'"') if byte == b'\\' => index += 1,
            Some(b'"') if byte == b'`' || (byte == b'$' && bytes.get(index + 1) == Some(&b'(')) => {
                return false;
            }
            Some(b'"') => {}
            Some(_) => unreachable!(),
            None => match byte {
                b'\\' => index += 1,
                b'$' if bytes
                    .get(index + 1)
                    .is_some_and(|next| matches!(*next, b'\'' | b'"')) =>
                {
                    return false;
                }
                b'\'' | b'"' => quote = Some(byte),
                b'`' | b'(' | b')' | b';' | b'&' | b'|' | b'\n' => return false,
                b'$' if bytes.get(index + 1) == Some(&b'(') => return false,
                _ => {}
            },
        }
        index += 1;
    }
    quote.is_none()
}

/// Every string of the value with this session's paths and names, then the
/// one-time values, replaced by placeholders.
fn placeholders(value: &Value, facts: &Facts) -> Value {
    match value {
        Value::String(text) => Value::String(replace(text, facts)),
        Value::Array(items) => Value::Array(items.iter().map(|v| placeholders(v, facts)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(name, v)| (name.clone(), placeholders(v, facts)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn replace(text: &str, facts: &Facts) -> String {
    static TMP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(^|[^\w./~-])(?:/private)?/tmp(?:/[^\s'"`;|&<>(){}]*|\b)"#)
            .expect("valid tmp regex")
    });
    static HASH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b[0-9a-fA-F]{7,64}\b").expect("valid hash regex"));
    static ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b0[0-9a-z]{25}\b").expect("valid id regex"));
    let mut named = [
        (facts.repository.as_str(), "<REPO>"),
        (facts.worktree.as_str(), "<WORKTREE>"),
        (facts.branch.as_deref().unwrap_or_default(), "<BRANCH>"),
    ];
    named.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
    let mut text = text.to_string();
    for (value, placeholder) in named.into_iter().chain([(facts.home.as_str(), "<HOME>")]) {
        if !value.is_empty() {
            text = text.replace(value, placeholder);
        }
    }
    let text = TMP.replace_all(&text, "${1}<TMP>");
    let text = HASH.replace_all(&text, |found: &Captures| {
        let word = &found[0];
        if word.bytes().any(|b| b.is_ascii_digit()) && word.bytes().any(|b| b.is_ascii_alphabetic())
        {
            "<HASH>".to_string()
        } else {
            word.to_string()
        }
    });
    ID.replace_all(&text, "<ID>").into_owned()
}

/// The value with the keys of every object sorted.
fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(name, value)| (name.clone(), sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The program of the first simple command, after its wrappers and
/// assignments, with its first plain argument for a program that has
/// subcommands.
fn command_family(command: &str) -> Option<String> {
    let first = simple_commands(command).into_iter().next()?;
    let words: Vec<&str> = first.split_whitespace().collect();
    let words = program(&words);
    let (&program, arguments) = words.split_first()?;
    if !SUBCOMMANDS.contains(&program) {
        return Some(program.to_string());
    }
    let arguments = if program == "git" {
        git_words(arguments)
    } else {
        arguments
    };
    Some(match arguments.iter().find(|word| !word.starts_with('-')) {
        Some(subcommand) => format!("{program} {subcommand}"),
        None => program.to_string(),
    })
}

/// The words from the program on: `NAME=value` assignments and the
/// [`WRAPPERS`] before it are dropped, with each wrapper's options, the value
/// of an option that takes one, and the duration of `timeout`.
fn program<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let mut words = words;
    while let Some(&first) = words.first() {
        if first.contains('=') && !first.starts_with(['-', '/', '.']) {
            words = &words[1..];
        } else if WRAPPERS.contains(&first) {
            let values = match first {
                "time" => &TIME_VALUES[..],
                _ => wrapper_value_options(first),
            };
            words = &words[1..];
            while let Some(&option) = words.first().filter(|word| word.starts_with('-')) {
                let taken = if values.contains(&option) { 2 } else { 1 };
                words = words.get(taken..).unwrap_or(&[]);
            }
            if first == "timeout" {
                words = words.get(1..).unwrap_or(&[]);
            }
        } else {
            break;
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::{Facts, normalize};

    use serde_json::{Value, json};

    const REPO: &str = "/Users/dev/workspace/ariadne";
    const WORKTREE: &str = "/Users/dev/.ariadne/worktrees/ezy1mfrv/pd35xzry-eng";
    const BRANCH: &str = "normalize-the-learned-permission-key-and-35xzry";

    fn facts() -> Facts {
        Facts {
            repository: REPO.into(),
            worktree: WORKTREE.into(),
            branch: Some(BRANCH.into()),
            home: "/Users/dev".into(),
        }
    }

    fn key(tool: &str, input: Value) -> String {
        normalize(tool, &input, &facts()).key
    }

    fn family(tool: &str, input: Value) -> String {
        normalize(tool, &input, &facts()).family
    }

    /// A stderr merge, an output filter and a description leave the key of
    /// a command as it was.
    #[test]
    fn the_four_forms_of_one_rebase_give_one_key() {
        let keys: Vec<String> = [
            json!({"command": "git rebase main"}),
            json!({"command": "git rebase main 2>&1"}),
            json!({"command": "git rebase main 2>&1 | tail -40", "description": "Rebase"}),
            json!({"description": "Rebase onto main", "command": "git rebase main | head -5"}),
        ]
        .into_iter()
        .map(|input| key("Bash", input))
        .collect();
        assert!(
            keys.iter().all(|k| k == r#"{"command":"git rebase main"}"#),
            "{keys:?}"
        );
    }

    /// A commit is a placeholder, so two merges of two commits share a key.
    #[test]
    fn two_complete_step_inputs_with_different_shas_give_one_key() {
        let tool = "mcp__ariadne__complete_step";
        let first = key(
            tool,
            json!({"merge_commit": "94f07c0b878adfa965c6b6438dad1dedb4578e9f"}),
        );
        let second = key(tool, json!({"merge_commit": "ff3c04a5"}));
        assert_eq!(first, r#"{"merge_commit":"<HASH>"}"#);
        assert_eq!(second, first);
    }

    /// An edit keys on its file, not on what it changes.
    #[test]
    fn two_edits_of_one_file_give_one_key() {
        let path = format!("{WORKTREE}/src/lib.rs");
        let first = key(
            "Edit",
            json!({"file_path": path, "old_string": "a", "new_string": "b"}),
        );
        let second = key(
            "Edit",
            json!({"file_path": path, "old_string": "c", "new_string": "d", "replace_all": true}),
        );
        let notebook = key(
            "NotebookEdit",
            json!({"file_path": path, "notebook_path": "a.ipynb", "new_source": "x"}),
        );
        assert_eq!(first, r#"{"file_path":"<WORKTREE>/src/lib.rs"}"#);
        assert_eq!(second, first);
        assert_eq!(notebook, first, "only `file_path` is kept");
    }

    /// The prose of an MCP call is left out of its key.
    #[test]
    fn two_step_returns_with_different_bodies_give_one_key() {
        let tool = "mcp__ariadne__fail_step";
        let first = key(
            tool,
            json!({"verdict": "approve", "body": "Looks good.", "summary": "ok"}),
        );
        let second = key(
            tool,
            json!({"verdict": "approve", "body": "Fine.", "reason": "tests pass"}),
        );
        assert_eq!(first, r#"{"verdict":"approve"}"#);
        assert_eq!(second, first);
    }

    /// A `cd` into the worktree, a stderr merge and a `tail` all go.
    #[test]
    fn a_cd_into_the_worktree_and_an_output_filter_are_dropped() {
        let command = format!("cd {WORKTREE}/ui && npm test 2>&1 | tail -20");
        assert_eq!(
            key("Bash", json!({"command": command})),
            r#"{"command":"npm test"}"#
        );
    }

    /// A `cd` elsewhere, and a filter that writes a file, stay in the key.
    #[test]
    fn a_cd_elsewhere_and_a_writing_filter_are_kept() {
        assert_eq!(
            key("Bash", json!({"command": "cd /etc && ls"})),
            r#"{"command":"cd /etc && ls"}"#
        );
        assert_eq!(
            key(
                "Bash",
                json!({"command": format!("cd {WORKTREE}/../x && ls")})
            ),
            r#"{"command":"cd <WORKTREE>/../x && ls"}"#
        );
        assert_eq!(
            key("Bash", json!({"command": "echo x | cat > notes.md"})),
            r#"{"command":"echo x | cat > notes.md"}"#
        );
        assert_eq!(
            key("Bash", json!({"command": "ls | tail -1 && rm a"})),
            r#"{"command":"ls | tail -1 && rm a"}"#
        );
    }

    /// A trailing filter stays in the key when an option can name a file or
    /// execute another program.
    #[test]
    fn a_filter_with_an_output_or_program_option_stays_in_the_key() {
        assert_eq!(
            key(
                "Bash",
                json!({"command": "git status | sort -o status.txt"})
            ),
            r#"{"command":"git status | sort -o status.txt"}"#
        );
        assert_eq!(
            key(
                "Bash",
                json!({"command": "git status | sort -S1 --compress-program=payload"})
            ),
            r#"{"command":"git status | sort -S1 --compress-program=payload"}"#
        );
        assert_eq!(
            key("Bash", json!({"command": "git status | cat 1"})),
            r#"{"command":"git status | cat 1"}"#
        );
        assert_eq!(
            key("Bash", json!({"command": "git status | head -n 5 1"})),
            r#"{"command":"git status | head -n 5 1"}"#
        );
    }

    /// A family allows a simple command with safe output filters, but rejects
    /// unsafe shell or program setup.
    #[test]
    fn a_family_covers_one_command_and_safe_output_filters() {
        for command in [
            "git status",
            "git status 2>&1 | head -n 20",
            "git commit -c HEAD",
        ] {
            assert!(
                normalize("Bash", &json!({"command": command}), &facts()).family_safe,
                "{command}"
            );
        }
        for command in [
            "git status && ./payload",
            "git status; rm payload",
            "git status\nrm payload",
            "git status $(./payload)",
            "git status <(./payload)",
            "git status `./payload`",
            "git status x$'\\'' ; ./payload ; : x$'\\''",
            "git status | grep clean",
            "git -c core.fsmonitor=./payload status",
            "git -ccore.fsmonitor=./payload status",
            "git --config-env core.fsmonitor=PAYLOAD status",
            "git --exec-path=./payload status",
            "LD_PRELOAD=./payload.so git status",
            "GIT_EXTERNAL_DIFF=./payload git diff",
            "GIT_PAGER=./payload git -p status",
            "env GIT_EXTERNAL_DIFF=./payload git diff",
            "env -S 'GIT_PAGER=./payload git -p status'",
            "git grep -O./payload TODO",
            "git grep --open-files-in-pager=./payload TODO",
            "git diff --output=src/lib.rs",
            "git log --output=src/lib.rs",
            "git ls-remote --upload-pack=./payload .",
            "git fetch --upload-pack=./payload .",
            "git rebase -x ./payload main",
            "git rebase --exec ./payload main",
            "git difftool -x ./payload",
            "git difftool --extcmd=./payload",
            "cargo build --config build.rustc-wrapper=./payload",
            "cargo --config=build.rustc-wrapper=./payload build",
            "go test -exec ./payload",
            "go build -toolexec=./payload",
            "make SHELL=./payload build",
            "make --eval=./payload",
            "npm test --script-shell=./payload",
            "pnpm test --script-shell=./payload",
            "yarn test --node-options=./payload",
        ] {
            assert!(
                !normalize("Bash", &json!({"command": command}), &facts()).family_safe,
                "{command}"
            );
        }
    }

    /// The longest of the repository, the worktree and the branch is
    /// replaced first, and the home last: a worktree under the home is
    /// `<WORKTREE>`, not `<HOME>/...`.
    #[test]
    fn the_worktree_the_repository_the_branch_and_the_home_are_replaced_in_order() {
        let command = format!(
            "ls {WORKTREE}/src {REPO}/docs && git push origin {BRANCH} && cat /Users/dev/.zshrc"
        );
        assert_eq!(
            key("Bash", json!({"command": command})),
            r#"{"command":"ls <WORKTREE>/src <REPO>/docs && git push origin <BRANCH> && cat <HOME>/.zshrc"}"#
        );
    }

    /// A session without a task branch replaces no branch.
    #[test]
    fn a_session_without_a_branch_keeps_the_branch_name() {
        let facts = Facts {
            branch: None,
            ..facts()
        };
        let command = format!("git push origin {BRANCH}");
        assert_eq!(
            normalize("Bash", &json!({"command": command}), &facts).key,
            format!(r#"{{"command":"git push origin {BRANCH}"}}"#)
        );
    }

    /// Temporary paths, hashes and Ariadne ids are placeholders; numbers and
    /// plain words are kept.
    #[test]
    fn one_time_values_are_placeholders_and_numbers_are_kept() {
        assert_eq!(
            key(
                "mcp__ariadne__get_task",
                json!({"task_id": "01m3z5ms6y9vayba2ppd35xzry", "limit": 40,
                       "log": "/private/tmp/run.log", "out": "/tmp/x/y.txt"})
            ),
            r#"{"limit":40,"log":"<TMP>","out":"<TMP>","task_id":"<ID>"}"#
        );
        assert_eq!(
            key(
                "Bash",
                json!({"command": "head -n 1234567 deadbeef 1a2b3c4 /Users/x/tmp/y"})
            ),
            r#"{"command":"head -n 1234567 deadbeef <HASH> /Users/x/tmp/y"}"#
        );
    }

    /// A hash in capitals is a hash too.
    #[test]
    fn an_uppercase_hash_is_a_placeholder() {
        assert_eq!(
            key("Bash", json!({"command": "git show 94F07C0B 94f07C0b"})),
            r#"{"command":"git show <HASH> <HASH>"}"#
        );
    }

    /// Only the session's own branch is `<BRANCH>`: a session names its own
    /// branch so and leaves another task's branch apart from its own.
    #[test]
    fn only_the_sessions_own_branch_is_the_branch_placeholder() {
        let second = Facts {
            branch: Some(format!("{BRANCH}-a2")),
            ..facts()
        };
        let command = format!("git push origin {BRANCH}-a2 && git log {BRANCH}");
        assert_eq!(
            normalize("Bash", &json!({"command": command}), &second).key,
            format!(r#"{{"command":"git push origin <BRANCH> && git log {BRANCH}"}}"#)
        );
        assert_eq!(
            key(
                "Bash",
                json!({"command": format!("git push origin {BRANCH}-a2")})
            ),
            r#"{"command":"git push origin <BRANCH>-a2"}"#
        );
    }

    /// A fetch keys on its host alone, and a search on nothing.
    #[test]
    fn a_fetch_keys_on_its_host_and_a_search_on_nothing() {
        assert_eq!(
            key(
                "WebFetch",
                json!({"url": "https://docs.rs/serde/latest", "prompt": "Summarize"})
            ),
            r#"{"host":"docs.rs"}"#
        );
        assert_eq!(key("WebSearch", json!({"query": "serde"})), "{}");
        assert_eq!(
            key("Glob", json!({"pattern": "*.rs", "description": "find"})),
            r#"{"pattern":"*.rs"}"#
        );
    }

    /// The family is the program, with the subcommand of a program that has
    /// them, after a `cd`, the global options of git and any wrapper.
    #[test]
    fn the_family_is_the_program_and_its_subcommand() {
        let command = format!("cd {WORKTREE} && cargo nextest run -p x");
        assert_eq!(family("Bash", json!({"command": command})), "cargo nextest");
        let command = format!("git -C {REPO} push origin main");
        assert_eq!(family("Bash", json!({"command": command})), "git push");
        assert_eq!(family("Bash", json!({"command": "ls -la"})), "ls");
        assert_eq!(
            family("Bash", json!({"command": "FOO=1 timeout 60 python3 x.py"})),
            "python3"
        );
        assert_eq!(
            family(
                "Bash",
                json!({"command": "sudo -u root nice -n 5 env A=b make build"})
            ),
            "make build"
        );
        assert_eq!(
            family(
                "Bash",
                json!({"command": "time -f '%E' -o t.log cargo test"})
            ),
            "cargo test"
        );
        assert_eq!(family("Bash", json!({"command": "xargs rm"})), "xargs");
        assert_eq!(family("Bash", json!({"command": "doas rm a"})), "doas");
        assert_eq!(
            family("Edit", json!({"file_path": "a", "old_string": "b"})),
            "Edit"
        );
    }
}
