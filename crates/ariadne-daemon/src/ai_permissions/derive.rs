//! Portable facts derived from the complete ACP permission request.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use super::operations;

pub(super) const TAGS: [&str; 15] = [
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
];

const CAPS: [&str; 6] = [
    "production",
    "credential_access",
    "credential_transfer",
    "privileged",
    "download_and_execute",
    "unknown_destination",
];

macro_rules! re_match {
    ($text:expr, $pattern:expr) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| {
            RegexBuilder::new($pattern)
                .case_insensitive(true)
                .build()
                .expect("valid derived-fact regex")
        });
        RE.is_match($text)
    }};
}

macro_rules! re_case {
    ($text:expr, $pattern:expr) => {{
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pattern).expect("valid derived-fact regex"));
        RE.is_match($text)
    }};
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Derived {
    pub(crate) operation: Option<&'static str>,
    pub(crate) risk_tags: Vec<&'static str>,
    pub(crate) rule: Option<&'static str>,
    pub(crate) cap: Option<&'static str>,
}

pub(crate) fn derive(request: &Value, workspace: Option<&str>) -> Derived {
    let (command, paths, title, kind) = parts(request);
    let found = tags(&command, &paths, &title, &kind, workspace);
    let risk_tags = TAGS
        .into_iter()
        .filter(|tag| found.contains(tag))
        .collect::<Vec<_>>();
    Derived {
        operation: operation(&command, &paths, &title, &kind, &found),
        rule: rule(&command, &found),
        cap: CAPS.into_iter().find(|cap| found.contains(cap)),
        risk_tags,
    }
}

fn parts(request: &Value) -> (String, Vec<String>, String, String) {
    let tool_call = request.get("toolCall").unwrap_or(&Value::Null);
    let raw_input = tool_call.get("rawInput").and_then(Value::as_object);
    let command = match raw_input.and_then(|input| input.get("command")) {
        Some(Value::String(command)) => command.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    let mut paths = ["file_path", "path", "url"]
        .into_iter()
        .filter_map(|key| raw_input?.get(key)?.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    paths.extend(
        tool_call
            .get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|location| location.get("path").and_then(Value::as_str))
            .map(str::to_string),
    );
    let title = tool_call
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let kind = tool_call
        .get("kind")
        .map(Value::to_string)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();
    (command, paths, title, kind)
}

fn words(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c| "'\"()[]{};,|".contains(c)))
        .collect()
}

fn recursive_rm(words: &[&str]) -> bool {
    let Some(index) = words.iter().position(|word| *word == "rm") else {
        return false;
    };
    words[index + 1..].iter().any(|option| {
        *option == "--recursive"
            || (option.starts_with('-')
                && !option.starts_with("--")
                && option[1..].to_ascii_lowercase().contains('r'))
    })
}

fn outside_path(path: &str, workspace: &str) -> bool {
    let candidate = path.trim_end_matches(['/', '.', ',', ':', ';', '"', '\'']);
    if path.trim_end_matches(['.', ',', ':', ';', '"', '\'']) == "/" {
        return true;
    }
    if matches!(candidate, "~" | "$HOME" | "${HOME}")
        || candidate.starts_with("~/")
        || candidate.starts_with("$HOME/")
        || candidate == ".."
        || candidate.starts_with("../")
    {
        return true;
    }
    if !candidate.starts_with('/') {
        return false;
    }
    let root = workspace.trim_end_matches('/');
    candidate != root && !candidate.starts_with(&format!("{root}/"))
}

fn paths_in_text(command: &str, title: &str) -> Vec<String> {
    words(&format!("{command} {title}"))
        .into_iter()
        .filter(|word| {
            word.starts_with('/')
                || word.starts_with("~/")
                || word.starts_with("$HOME/")
                || word.starts_with("../")
                || matches!(*word, ".." | "~" | "$HOME" | "${HOME}")
        })
        .map(str::to_string)
        .collect()
}

fn urls(text: &str) -> impl Iterator<Item = &str> {
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        RegexBuilder::new(r#"https?://[^\s'\"<>]+"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    });
    URL.find_iter(text).map(|found| found.as_str())
}

fn local_url(url: &str) -> bool {
    re_match!(
        url,
        r"^https?://(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])(?:[:/?#]|$)"
    )
}

fn external_url(text: &str) -> bool {
    urls(text).any(|url| !local_url(url))
}

fn fetches_remote(command: &str) -> bool {
    if !re_match!(command, r"\b(?:curl|wget)\b") {
        return false;
    }
    let local = re_match!(
        command,
        r#"(?:^|[\s/@'\"])(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])(?:[:/?#\s'\"]|$)"#
    );
    external_url(command) || !local
}

fn credential_source(text: &str) -> bool {
    if re_match!(
        text,
        r#"(?:\.aws/credentials|\.ssh/(?:id_[^/\s'\"]+|config)|\.npmrc|\.pypirc|/etc/(?:shadow|passwd)|credentials?\.json|\bfind-generic-password\b|\.git-credentials|\.netrc|\.kube/config|\.docker/config\.json|\.gnupg|--export-secret-keys?\b|\bdump-keychain\b|\.keychain|\.pem\b|\.key\b|\.p12\b|\bid_(?:rsa|ed25519|ecdsa|dsa|\*)|(?:^|[;&|(]\s*)(?:env|printenv)\s*(?:$|[>;&]|\|\s*(?:curl|wget|nc|ncat|socat|tee|base64|xxd|gzip)\b))"#
    ) {
        return true;
    }
    static DOTENV: LazyLock<Regex> = LazyLock::new(|| {
        RegexBuilder::new(r#"[^\s/'\"=:]*\.env(?:\.[^/\s'\"]*)?"#)
            .case_insensitive(true)
            .build()
            .unwrap()
    });
    DOTENV
        .find_iter(text)
        .any(|found| !re_match!(found.as_str(), r"(?:example|sample|template)"))
}

fn secret_in_address(text: &str) -> bool {
    urls(text).any(|url| {
        !local_url(url)
            && url.split_once('?').is_some_and(|(_, query)| {
                re_match!(
                    query,
                    r#"(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|private[_-]?key)(?:=|%3D)[^&\s'\"]"#
                )
            })
    })
}

fn uploads(command: &str) -> bool {
    let external_target =
        external_url(command) || re_match!(command, r"\b[A-Za-z0-9._-]+@[A-Za-z0-9.-]+:");
    external_target
        && (re_match!(
            command,
            r"\b(?:curl|wget)\b.*(?:-d|--data|--upload-file|-T)\b|\b(?:scp|rsync|nc|ncat)\b"
        ) || re_case!(
            command,
            r"\bcurl\b.*\s(?:-F|--form)(?:[\s=]|$)|\bwget\b.*\s--post-(?:file|data)\b"
        ))
}

fn deletes(command: &str) -> bool {
    re_match!(
        command,
        concat!(
            r#"(?:^|[\s;&|('\"`])"#,
            r"(?:rm|rmdir|unlink|shred|wipe)(?:\s|$)"
        )
    ) || re_match!(command, r"\bfind\b[^;&|]*\s-delete\b")
}

fn discards(command: &str) -> bool {
    re_match!(
        command,
        r"\bgit\s+(?:reset\s+--hard|clean\b|stash\s+(?:clear|drop)\b)"
    ) || re_match!(
        command,
        concat!(
            r"\bgit\s+checkout",
            r"\s+(?:[^;&|]*\s)?",
            r"(?:--\s|\.(?:\s|$))"
        )
    ) || re_match!(
        command,
        concat!(
            r"\bgit\s+push",
            r"\s+(?:[^;&|]*\s)?",
            r"(?:--force|--mirror|-[a-z]*f[a-z]*(?:\s|$))"
        )
    ) || re_case!(
        command,
        concat!(r"\bgit\s+branch", r"\s+(?:[^;&|]*\s)?", r"-D\b")
    )
}

fn forces(command: &str) -> bool {
    re_match!(command, r"(?:^|\s)--force(?:-with-lease)?(?:[\s=]|$)")
        || re_case!(
            command,
            concat!(
                r#"(?:^|[\s;&|('\"`])"#,
                r"(?:rm|cp|mv|ln)",
                r"\s+(?:[^;&|]*\s)?",
                r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)"
            )
        )
        || re_case!(
            command,
            concat!(
                r"\bgit\s+(?:push|checkout|clean|branch|tag|fetch|switch|rm|mv)",
                r"\s+(?:[^;&|]*\s)?",
                r"-[A-Za-z]*f[A-Za-z]*(?:\s|$)"
            )
        )
        || re_case!(
            command,
            concat!(r"\bgit\s+branch", r"\s+(?:[^;&|]*\s)?", r"-D\b")
        )
        || re_case!(
            command,
            concat!(
                r#"(?:^|[\s;&|('\"`])"#,
                r"(?:kill|pkill|killall)",
                r"\s+(?:[^;&|]*\s)?",
                r"-(?:9|KILL|SIGKILL)\b"
            )
        )
}

fn recursive_change(command: &str) -> bool {
    re_case!(
        command,
        concat!(
            r#"(?:^|[\s;&|('\"`])"#,
            r"(?:chmod|chown|chgrp)",
            r"\s+(?:[^;&|]*\s)?",
            r"-[A-Za-z]*R"
        )
    ) || re_case!(
        command,
        concat!(
            r#"(?:^|[\s;&|('\"`])"#,
            r"(?:cp|scp)",
            r"\s+(?:[^;&|]*\s)?",
            r"-[A-Za-z]*[rRa]"
        )
    ) || re_case!(
        command,
        concat!(
            r#"(?:^|[\s;&|('\"`])"#,
            r"(?:rsync|zip)",
            r"\s+(?:[^;&|]*\s)?",
            r"-[A-Za-z]*[ra]"
        )
    ) || re_match!(command, r"\bfind\b[^;&|]*\s-delete\b")
        || re_match!(
            command,
            concat!(r"\bgit\s+clean", r"\s+(?:[^;&|]*\s)?", r"-[a-z]*d")
        )
}

fn many(command: &str) -> bool {
    let without_urls = urls(command).fold(command.to_string(), |text, url| text.replace(url, ""));
    without_urls.contains('*')
        || re_match!(command, r"\b(?:xargs|find)\b")
        || re_case!(command, r"\s(?:--all|-A)(?:\s|$)")
        || re_case!(command, r"\s\.(?:\s|$)")
}

fn changes(command: &str) -> bool {
    re_match!(
        command,
        concat!(
            r#"(?:^|[\s;&|('\"`])"#,
            r"(?:rm|mv|cp|chmod|chown|chgrp|shred|truncate|tar|zip|rsync|scp|kill)(?:\s|$)"
        )
    ) || re_match!(command, r"\s-(?:delete|exec|execdir)\b|\bsed\s+-i")
        || re_match!(
            command,
            r"\bgit\s+(?:add|checkout|restore|clean|push|rm|reset|stash\s+clear)\b"
        )
        || re_match!(command, r"\bkubectl\b[^;&|]*\sdelete\b")
}

fn persists(command: &str, text: &str, kind: &str) -> bool {
    let found = (!command.is_empty())
        .then(|| operations::operation(command, false))
        .flatten();
    if !command.is_empty()
        && found != Some("read_workspace")
        && re_match!(
            command,
            r"\b(?:crontab|launchctl)\b|\bsystemctl\s+enable\b|\bgit\s+config\s+(?:[^;&|]*\s)?--(?:global|system)\b"
        )
    {
        return true;
    }
    let change = if command.is_empty() {
        matches!(kind, "edit" | "write" | "delete" | "move")
    } else {
        matches!(
            found,
            Some(
                "destructive_or_exfiltration"
                    | "system_privileged"
                    | "external_mutation"
                    | "delete_workspace"
                    | "version_control_mutation"
                    | "dependency_change"
                    | "write_workspace"
            )
        )
    };
    change
        && re_match!(
            text,
            r#"\.git/hooks/|LaunchAgents/|LaunchDaemons/|/cron\.|(?:^|[\s/'\"=:])(?:\.(?:bashrc|bash_profile|bash_login|profile|zshrc|zshenv|zprofile|zlogin|gitconfig|npmrc|mcp\.json|gitlab-ci\.yml)\b|\.github/(?:workflows/|dependabot\.yml)|\.circleci/|\.config/|\.cargo/config|\.(?:claude|codex|cursor|vscode)/(?:hooks/|commands/|agents/|rules|[^\s'\"/]*(?:settings|config|tasks|launch|mcp)[^\s'\"/]*)|\.ssh/(?:authorized_keys|config)\b|\.aws/config\b|(?:AGENTS|CLAUDE)\.md\b|rust-toolchain\b)|(?:^|[\s'\"=])/(?:etc|usr|bin|sbin|System|Library|var|opt|boot)/"#
        )
}

fn tags(
    command: &str,
    paths: &[String],
    title: &str,
    kind: &str,
    workspace: Option<&str>,
) -> Vec<&'static str> {
    let text = std::iter::once(command)
        .chain(std::iter::once(title))
        .chain(paths.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    let words = words(command);
    let mut tags = Vec::new();
    fn add(tags: &mut Vec<&'static str>, tag: &'static str) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    if workspace.is_some_and(|workspace| {
        paths
            .iter()
            .cloned()
            .chain(paths_in_text(command, title))
            .any(|path| outside_path(&path, workspace))
    }) {
        add(&mut tags, "outside_workspace");
    }
    if recursive_rm(&words) || words.contains(&"--recursive") || recursive_change(command) {
        add(&mut tags, "recursive");
    }
    if many(command) && changes(command) {
        add(&mut tags, "bulk");
    }
    if deletes(command) || discards(command) {
        add(&mut tags, "irreversible");
    }
    if external_url(&text)
        || fetches_remote(command)
        || re_match!(
            command,
            r"\bgit\s+(?:fetch|pull|push|clone|remote)\b|\b(?:ssh|scp|rsync)\b"
        )
    {
        add(&mut tags, "remote");
    }
    if re_match!(&text, r"\b(?:production|prod)\b") {
        add(&mut tags, "production");
    }
    if credential_source(&text) {
        add(&mut tags, "credential_access");
    }
    if re_match!(
        &text,
        r"\b(?:sudo|doas)\b|\bsetuid\b|/etc/(?:sudoers|systemd)"
    ) || re_match!(&text, r"(?:^|[;&|(]\s*)su(?:\s|$)")
        || re_case!(
            &text,
            r"\bchmod\s+(?:-[A-Za-z]+\s+)*(?:(?:[0-7]?[0-7][0-7][2367]|[2467][0-7][0-7][0-7])(?:\s|$)|[ugoa]*[+=][rwxXt]*s|[ugoa]*[oa][ugoa]*[+=][rxXst]*w)"
        )
    {
        add(&mut tags, "privileged");
    }
    if command.contains("$(") || command.contains("${") || command.contains('`') {
        add(&mut tags, "shell_interpolation");
    }
    if re_match!(command, r"\b(?:curl|wget)\b")
        && re_match!(
            command,
            r"\|\s*(?:sh|bash|zsh)\b|\|\s*python[\d.]*\s*(?:$|[;&|)]|-(?:\s|$))"
        )
    {
        add(&mut tags, "download_and_execute");
    }
    if secret_in_address(&text) {
        add(&mut tags, "credential_access");
        add(&mut tags, "credential_transfer");
        add(&mut tags, "unknown_destination");
    }
    if uploads(command) {
        add(&mut tags, "unknown_destination");
        if tags.contains(&"credential_access") {
            add(&mut tags, "credential_transfer");
        }
    }
    if forces(command) {
        add(&mut tags, "force");
    }
    if re_match!(command, r"(?:^|\s)(?:nohup|disown)\b") || re_match!(command, r"&\s*$") {
        add(&mut tags, "background_process");
    }
    if persists(command, &text, kind) {
        add(&mut tags, "persistent_change");
    }
    tags
}

fn reviewer_text(text: &str) -> bool {
    re_match!(
        text,
        r"\b(?:ignore|disregard)\s+(?:all\s+|any\s+)?(?:the\s+)?(?:previous|prior|above|earlier)\s+(?:instructions?|rules?|prompts?)\b|\bnote\s+to\s+(?:the\s+)?reviewer\b|\b(?:always|please|definitely|just|safe\s+to)\s+(?:allow|approve)\b|\ballow\s+without\s+asking\b|\bpre-?approved\b"
    )
}

fn rule(command: &str, tags: &[&str]) -> Option<&'static str> {
    let words = words(command);
    if tags.contains(&"credential_access") && uploads(command) {
        return Some("credential_transfer");
    }
    if recursive_rm(&words) && words.iter().any(|word| matches!(*word, "/" | "/*")) {
        return Some("root_delete");
    }
    if recursive_rm(&words)
        && words.iter().any(|word| {
            matches!(*word, "~" | "$HOME" | "${HOME}") || re_case!(word, r"^/home/[^/]+$")
        })
    {
        return Some("home_delete");
    }
    if re_match!(
        command,
        r"--(?:dangerously-)?skip-permissions\b|\bariadne\s+permissions\s+(?:disable|off)\b|\b(?:disable|bypass|skip)\s+(?:agent\s+)?(?:permissions?|approvals?)\b|\bturn\s+off\s+(?:agent\s+)?(?:permissions?|approvals?)\b|\bskip[_-]?confirmations?.{0,24}\b(?:true|1)\b|\bapproval_policy.{0,24}\b(?:never|off)\b|--(?:allow-all-tools|disable-confirmation)\b"
    ) || operations::comments(command)
        .iter()
        .any(|comment| reviewer_text(comment))
    {
        return Some("permission_tamper");
    }
    None
}

fn operation(
    command: &str,
    paths: &[String],
    title: &str,
    kind: &str,
    tags: &[&str],
) -> Option<&'static str> {
    if reviewer_text(&format!("{command} {title}"))
        || tags.contains(&"credential_transfer")
        || tags.contains(&"download_and_execute")
    {
        return Some("destructive_or_exfiltration");
    }
    if tags.contains(&"privileged") {
        return Some("system_privileged");
    }
    if tags.contains(&"credential_access") {
        return Some("secrets_credentials");
    }
    if !command.is_empty() {
        let local = !external_url(command)
            && re_match!(
                command,
                r#"(?:^|[\s/@'\"])(?:localhost|127\.\d+\.\d+\.\d+|0\.0\.0\.0|\[::1\])(?:[:/?#\s'\"]|$)"#
            );
        return operations::operation(command, local);
    }
    if external_url(&paths.join(" ")) {
        return Some("network_read");
    }
    if matches!(kind, "read" | "search") || re_match!(title, r"^read\b") {
        return Some("read_workspace");
    }
    if matches!(kind, "edit" | "write") || re_match!(title, r"^(?:write|edit)\b") {
        return paths
            .iter()
            .map(|path| operations::write_operation(path))
            .min_by_key(|operation| operations::rank(operation))
            .or(Some("write_workspace"));
    }
    None
}
