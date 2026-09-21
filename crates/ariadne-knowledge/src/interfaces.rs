//! The interfaces a file holds: what one repository offers another, and what
//! it takes from it.
//!
//! None of these is a symbol. A manifest names the package it defines and
//! the packages it depends on; a call registers a route or requests one; a
//! line sets an environment variable or reads one. Each is recorded as an
//! [`Interface`] of the blob, keyed by its kind and its name, and
//! [`crate::resolve::interface_edges`] is what matches them across
//! repositories: a dependency to the package it names, a request to the
//! template it fits, a set to the reads of the same variable.
//!
//! Everything here is read off the text. The shapes are few, and a grammar
//! per manifest would buy little over a scan of its lines.

use crate::languages::Language;
use crate::parser::{Lines, Symbol};

/// What one interface is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InterfaceKind {
    /// A package the manifest defines.
    Package,
    /// A package the manifest depends on, by name.
    Dependency,
    /// A package the manifest depends on, by path: a `path` dependency, a
    /// `file:` or `workspace:` link, a `replace` to a directory, a
    /// `project(':x')`.
    PathDependency,
    /// A route template a call registers: `/v1/items/{id}`.
    Route,
    /// A route a request call uses: `/v1/items/42`.
    RouteUse,
    /// An environment variable the file reads.
    EnvRead,
    /// An environment variable the file sets.
    EnvSet,
}

impl InterfaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InterfaceKind::Package => "package",
            InterfaceKind::Dependency => "dependency",
            InterfaceKind::PathDependency => "path_dependency",
            InterfaceKind::Route => "route",
            InterfaceKind::RouteUse => "route_use",
            InterfaceKind::EnvRead => "env_read",
            InterfaceKind::EnvSet => "env_set",
        }
    }
}

/// One interface of one blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub kind: InterfaceKind,
    /// The package, the route or the variable.
    pub name: String,
    /// 1-based.
    pub line: u32,
    /// The index in the file's symbols of the definition it sits in — for a
    /// route a decorator or an attribute registers, the definition right
    /// under it. `None` at file scope.
    pub symbol: Option<usize>,
    /// The names a route registration passes as its handlers, for the
    /// resolution pass to find. Empty on everything else.
    pub handlers: Vec<String>,
    /// The HTTP method a route call names, uppercased: `GET` for
    /// `client.get("/v1/items")`. `None` where the call names none.
    pub method: Option<String>,
}

/// The manifests read by their file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Manifest {
    Cargo,
    Npm,
    Pub,
    GoMod,
    Pom,
    Gradle,
    GradleSettings,
    /// A `.env` file: every `X=` line sets a variable.
    DotEnv,
    /// A compose file or a workflow: every `X:` or `- X=` line under an
    /// uppercase key sets a variable.
    EnvYaml,
}

/// Whether a path is one the index reads for its interfaces alone, having
/// no language of its own: `go.mod`, `pom.xml`, the Gradle scripts, `.env`.
pub fn is_manifest_only(path: &str) -> bool {
    matches!(
        manifest_of(path),
        Some(
            Manifest::GoMod
                | Manifest::Pom
                | Manifest::Gradle
                | Manifest::GradleSettings
                | Manifest::DotEnv
        )
    )
}

fn manifest_of(path: &str) -> Option<Manifest> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let yaml = name.ends_with(".yml") || name.ends_with(".yaml");
    Some(match name {
        "Cargo.toml" => Manifest::Cargo,
        "package.json" => Manifest::Npm,
        "pubspec.yaml" | "pubspec.yml" => Manifest::Pub,
        "go.mod" => Manifest::GoMod,
        "pom.xml" => Manifest::Pom,
        "build.gradle" | "build.gradle.kts" => Manifest::Gradle,
        "settings.gradle" | "settings.gradle.kts" => Manifest::GradleSettings,
        _ if name.starts_with(".env") => Manifest::DotEnv,
        _ if yaml
            && (name.starts_with("docker-compose")
                || name.starts_with("compose")
                || path.contains(".github/workflows/")) =>
        {
            Manifest::EnvYaml
        }
        _ => return None,
    })
}

/// Every interface of one file: what its manifest says where it is one, and
/// the routes and the environment variables its text names.
pub fn read(path: &str, language: Language, source: &str, symbols: &[Symbol]) -> Vec<Interface> {
    let lines = Lines::of(source);
    let mut found = Vec::new();
    match manifest_of(path) {
        Some(Manifest::Cargo) => cargo(source, &mut found),
        Some(Manifest::Npm) => npm(source, &lines, &mut found),
        Some(Manifest::Pub) => pubspec(source, &mut found),
        Some(Manifest::GoMod) => go_mod(source, &mut found),
        Some(Manifest::Pom) => pom(source, &lines, &mut found),
        Some(Manifest::Gradle) => gradle(source, &mut found),
        Some(Manifest::GradleSettings) => gradle_settings(source, &mut found),
        Some(Manifest::DotEnv) => dot_env(source, &mut found),
        Some(Manifest::EnvYaml) => env_yaml(source, &mut found),
        None => {}
    }
    if language.tags_query().is_some() {
        env_in_code(source, &lines, symbols, &mut found);
        routes(source, &lines, symbols, language, &mut found);
    }
    // One interface per kind, name and line: a pattern that is the tail of
    // another finds the same literal twice.
    let mut seen = std::collections::HashSet::new();
    found.retain(|interface| seen.insert((interface.kind, interface.name.clone(), interface.line)));
    found
}

fn push(found: &mut Vec<Interface>, kind: InterfaceKind, name: &str, line: u32) {
    let name = name.trim();
    if !name.is_empty() {
        found.push(Interface {
            kind,
            name: name.to_string(),
            line,
            symbol: None,
            handlers: Vec::new(),
            method: None,
        });
    }
}

/// The last dependency pushed under `name`, made a path dependency: what a
/// `path =` line under `[dependencies.x]` or a `path:` line under a pubspec
/// dependency says about the entry above it.
fn make_path_dependency(found: &mut [Interface], name: &str) {
    if let Some(dependency) = found
        .iter_mut()
        .rev()
        .find(|i| i.kind == InterfaceKind::Dependency && i.name == name)
    {
        dependency.kind = InterfaceKind::PathDependency;
    }
}

// -- manifests ---------------------------------------------------------------

/// Whether a Cargo table holds dependencies: `dependencies`,
/// `dev-dependencies`, `build-dependencies`, `workspace.dependencies`,
/// `target.'cfg(unix)'.dependencies`.
fn is_dependency_table(table: &str) -> bool {
    table == "dependencies" || table.ends_with("-dependencies") || table.ends_with(".dependencies")
}

/// `[package] name`, and the keys of every dependency table. A `path`
/// dependency is one by path; `package = "x"` renames what is depended on.
fn cargo(source: &str, found: &mut Vec<Interface>) {
    let mut section = String::new();
    // The dependency a `[dependencies.x]` table is about, whose `path` key
    // may follow on a line of its own.
    let mut table_dependency: Option<String> = None;
    for (at, line) in source.lines().enumerate() {
        let line_no = at as u32 + 1;
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        if let Some(header) = text.strip_prefix('[') {
            section = header
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            table_dependency = None;
            if let Some((table, name)) = section.rsplit_once('.')
                && is_dependency_table(table)
            {
                let name = name.trim_matches('"');
                push(found, InterfaceKind::Dependency, name, line_no);
                table_dependency = Some(name.to_string());
            }
            continue;
        }
        let Some((key, value)) = text.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = value.trim();
        if section == "package" && key == "name" {
            push(found, InterfaceKind::Package, &unquote(value), line_no);
        } else if let Some(name) = &table_dependency {
            if key == "path" {
                make_path_dependency(found, name);
            }
        } else if is_dependency_table(&section) {
            let renamed = inline_value(value, "package");
            let name = renamed.as_deref().unwrap_or(key);
            let kind = match inline_value(value, "path").is_some() {
                true => InterfaceKind::PathDependency,
                false => InterfaceKind::Dependency,
            };
            push(found, kind, name, line_no);
        }
    }
}

/// The value of `key` in an inline table: `{ path = "../x", version = "1" }`.
fn inline_value(value: &str, key: &str) -> Option<String> {
    let inner = value.strip_prefix('{')?;
    inner.split(',').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k.trim() == key).then(|| unquote(v.trim().trim_end_matches('}')))
    })
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(',')
        .trim()
        .trim_matches(['"', '\''])
        .to_string()
}

/// `name`, and every key of `dependencies`, `devDependencies`,
/// `peerDependencies` and `optionalDependencies`; a `file:`, `link:` or
/// `workspace:` value is a path dependency. A `workspaces` entry that names a
/// directory is a path dependency on the package that directory holds.
fn npm(source: &str, lines: &Lines, found: &mut Vec<Interface>) {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&Language::Json.grammar()).is_err() {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };
    let Some(object) = tree
        .root_node()
        .named_child(0)
        .filter(|node| node.kind() == "object")
    else {
        return;
    };
    let text = |node: tree_sitter::Node| {
        source
            .get(node.byte_range())
            .unwrap_or_default()
            .trim_matches('"')
            .to_string()
    };
    let mut cursor = object.walk();
    for pair in object.named_children(&mut cursor) {
        let (Some(key), Some(value)) = (
            pair.child_by_field_name("key"),
            pair.child_by_field_name("value"),
        ) else {
            continue;
        };
        match text(key).as_str() {
            "name" if value.kind() == "string" => {
                push(
                    found,
                    InterfaceKind::Package,
                    &text(value),
                    lines.line_of(pair.start_byte()),
                );
            }
            "dependencies" | "devDependencies" | "peerDependencies" | "optionalDependencies"
                if value.kind() == "object" =>
            {
                let mut inner = value.walk();
                for dependency in value.named_children(&mut inner) {
                    let (Some(name), Some(version)) = (
                        dependency.child_by_field_name("key"),
                        dependency.child_by_field_name("value"),
                    ) else {
                        continue;
                    };
                    let version = text(version);
                    let by_path = ["file:", "link:", "workspace:", "portal:"]
                        .iter()
                        .any(|prefix| version.starts_with(prefix));
                    let kind = match by_path {
                        true => InterfaceKind::PathDependency,
                        false => InterfaceKind::Dependency,
                    };
                    push(
                        found,
                        kind,
                        &text(name),
                        lines.line_of(dependency.start_byte()),
                    );
                }
            }
            "workspaces" => {
                let entries = match value.kind() {
                    "array" => Some(value),
                    "object" => {
                        let mut inner = value.walk();
                        value
                            .named_children(&mut inner)
                            .find(|p| {
                                p.child_by_field_name("key")
                                    .is_some_and(|k| text(k) == "packages")
                            })
                            .and_then(|p| p.child_by_field_name("value"))
                            .filter(|v| v.kind() == "array")
                    }
                    _ => None,
                };
                let Some(entries) = entries else {
                    continue;
                };
                let mut inner = entries.walk();
                for entry in entries.named_children(&mut inner) {
                    let directory = text(entry);
                    // A glob names no one package.
                    if entry.kind() != "string" || directory.contains('*') {
                        continue;
                    }
                    let name = directory
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("");
                    push(
                        found,
                        InterfaceKind::PathDependency,
                        name,
                        lines.line_of(entry.start_byte()),
                    );
                }
            }
            _ => {}
        }
    }
}

/// `name:`, and the keys nested under `dependencies:`,
/// `dev_dependencies:` and `dependency_overrides:`; a dependency with a
/// `path:` key under it is one by path.
fn pubspec(source: &str, found: &mut Vec<Interface>) {
    let mut section = String::new();
    let mut dependency: Option<String> = None;
    for (at, line) in source.lines().enumerate() {
        let line_no = at as u32 + 1;
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let Some((key, value)) = text.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(['"', '\'']);
        if indent == 0 {
            section = key.to_string();
            dependency = None;
            if key == "name" {
                push(found, InterfaceKind::Package, &unquote(value), line_no);
            }
            continue;
        }
        let in_dependencies = matches!(
            section.as_str(),
            "dependencies" | "dev_dependencies" | "dependency_overrides"
        );
        if !in_dependencies {
            continue;
        }
        match (indent, key) {
            (1..=2, _) => {
                push(found, InterfaceKind::Dependency, key, line_no);
                dependency = Some(key.to_string());
            }
            (_, "path") => {
                if let Some(name) = &dependency {
                    make_path_dependency(found, name);
                }
            }
            _ => {}
        }
    }
}

/// `module`, every `require`, and every `replace` whose right-hand side is a
/// directory.
fn go_mod(source: &str, found: &mut Vec<Interface>) {
    let mut block: Option<&str> = None;
    for (at, line) in source.lines().enumerate() {
        let line_no = at as u32 + 1;
        let text = line.split("//").next().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        if let Some(name) = text.strip_prefix("module ") {
            push(found, InterfaceKind::Package, name, line_no);
            continue;
        }
        if text == ")" {
            block = None;
            continue;
        }
        let (directive, rest) = match block {
            Some(directive) => (directive, text),
            None => match text.split_once(' ') {
                Some((directive, rest)) if rest.trim() == "(" => {
                    block = Some(match directive {
                        "require" => "require",
                        "replace" => "replace",
                        _ => "",
                    });
                    continue;
                }
                Some((directive, rest)) => (directive, rest.trim()),
                None => continue,
            },
        };
        match directive {
            "require" => {
                if let Some(name) = rest.split_whitespace().next() {
                    push(found, InterfaceKind::Dependency, name, line_no);
                }
            }
            "replace" => {
                if let Some((name, target)) = rest.split_once("=>") {
                    let name = name.split_whitespace().next().unwrap_or("");
                    let target = target.trim();
                    if target.starts_with('.') || target.starts_with('/') {
                        push(found, InterfaceKind::PathDependency, name, line_no);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The project's own `groupId:artifactId`, and one per `<dependency>`. The
/// elements are walked in order, so a coordinate is read for the element it
/// sits in: `<parent>`, `<dependency>`, `<plugin>` or the project itself.
fn pom(source: &str, lines: &Lines, found: &mut Vec<Interface>) {
    let mut stack: Vec<&str> = Vec::new();
    // The group read in the element on top of the stack, waiting for its
    // artifact.
    let mut group: Vec<Option<String>> = Vec::new();
    let mut at = 0;
    let mut project_named = false;
    while let Some(open) = source[at..].find('<') {
        let open = at + open;
        let Some(close) = source[open..].find('>') else {
            break;
        };
        let close = open + close;
        let tag = &source[open + 1..close];
        at = close + 1;
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if stack.last() == Some(&name.trim()) {
                stack.pop();
                group.pop();
            }
            continue;
        }
        if tag.ends_with('/') {
            continue;
        }
        let name = tag.split_whitespace().next().unwrap_or("");
        match name {
            "groupId" | "artifactId" => {
                let Some(end) = source[at..].find('<') else {
                    break;
                };
                let value = source[at..at + end].trim().to_string();
                let element = stack.last().copied().unwrap_or("");
                if name == "groupId" {
                    if let Some(slot) = group.last_mut() {
                        *slot = Some(value);
                    }
                    continue;
                }
                let coordinate = match group.last().and_then(Option::as_deref) {
                    Some(group) => format!("{group}:{value}"),
                    None => value,
                };
                match element {
                    "dependency" => {
                        push(
                            found,
                            InterfaceKind::Dependency,
                            &coordinate,
                            lines.line_of(open),
                        );
                    }
                    "project" if !project_named => {
                        project_named = true;
                        push(
                            found,
                            InterfaceKind::Package,
                            &coordinate,
                            lines.line_of(open),
                        );
                    }
                    _ => {}
                }
            }
            _ => {
                stack.push(name);
                group.push(None);
            }
        }
    }
}

/// The configurations a Gradle script declares a dependency under.
const GRADLE_CONFIGURATIONS: &[&str] = &[
    "implementation",
    "api",
    "compileOnly",
    "runtimeOnly",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "annotationProcessor",
    "kapt",
    "classpath",
];

/// `implementation 'g:a:v'`, `implementation("g:a:v")` and
/// `implementation(project(":x"))`, under every configuration named above.
fn gradle(source: &str, found: &mut Vec<Interface>) {
    for (at, line) in source.lines().enumerate() {
        let line_no = at as u32 + 1;
        let text = line.trim();
        let Some(configuration) = GRADLE_CONFIGURATIONS.iter().find(|c| {
            text.strip_prefix(*c)
                .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('('))
        }) else {
            continue;
        };
        let rest = text[configuration.len()..].trim_start();
        let rest = rest.strip_prefix('(').unwrap_or(rest);
        if let Some(project) = rest.find("project(") {
            if let Some(path) = quoted(&rest[project + "project(".len()..]) {
                let name = path.rsplit(':').next().unwrap_or(&path);
                push(found, InterfaceKind::PathDependency, name, line_no);
            }
            continue;
        }
        if let Some(coordinate) = quoted(rest) {
            let mut parts = coordinate.split(':');
            if let (Some(group), Some(artifact)) = (parts.next(), parts.next()) {
                push(
                    found,
                    InterfaceKind::Dependency,
                    &format!("{group}:{artifact}"),
                    line_no,
                );
            }
        }
    }
}

/// `rootProject.name = "x"`.
fn gradle_settings(source: &str, found: &mut Vec<Interface>) {
    for (at, line) in source.lines().enumerate() {
        let text = line.trim();
        if let Some(rest) = text.strip_prefix("rootProject.name")
            && let Some(name) = quoted(rest.trim_start().trim_start_matches('='))
        {
            push(found, InterfaceKind::Package, &name, at as u32 + 1);
        }
    }
}

/// `X=value`, with or without `export`.
fn dot_env(source: &str, found: &mut Vec<Interface>) {
    for (at, line) in source.lines().enumerate() {
        let text = line.trim();
        let text = text.strip_prefix("export ").unwrap_or(text).trim_start();
        if let Some((name, _)) = text.split_once('=')
            && is_identifier(name.trim())
        {
            push(found, InterfaceKind::EnvSet, name.trim(), at as u32 + 1);
        }
    }
}

/// `X: value` and `- X=value`, for an uppercase `X`: the environment entries
/// of a compose service or a workflow.
fn env_yaml(source: &str, found: &mut Vec<Interface>) {
    for (at, line) in source.lines().enumerate() {
        let text = line.trim();
        let text = text.strip_prefix('-').unwrap_or(text).trim_start();
        let text = text.trim_matches(['"', '\'']);
        let Some(end) = text.find([':', '=']) else {
            continue;
        };
        let name = text[..end].trim().trim_matches(['"', '\'']);
        if name.len() >= 2
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            && name.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
        {
            push(found, InterfaceKind::EnvSet, name, at as u32 + 1);
        }
    }
}

// -- environment variables in code -------------------------------------------

/// How the variable's name follows a pattern.
#[derive(Clone, Copy)]
enum Follow {
    /// A string literal: `env::var("X")`.
    Quoted,
    /// A bare identifier: `process.env.X`.
    Identifier,
}

/// What reads an environment variable, per language, and how the name
/// follows.
const ENV_READS: &[(&str, Follow)] = &[
    ("env::var(", Follow::Quoted),
    ("env::var_os(", Follow::Quoted),
    ("option_env!(", Follow::Quoted),
    ("env!(", Follow::Quoted),
    ("process.env.", Follow::Identifier),
    ("process.env[", Follow::Quoted),
    ("import.meta.env.", Follow::Identifier),
    ("Deno.env.get(", Follow::Quoted),
    ("os.environ[", Follow::Quoted),
    ("os.environ.get(", Follow::Quoted),
    ("os.getenv(", Follow::Quoted),
    ("GetEnvironmentVariable(", Follow::Quoted),
    ("Platform.environment[", Follow::Quoted),
    ("String.fromEnvironment(", Follow::Quoted),
    ("os.Getenv(", Follow::Quoted),
    ("os.LookupEnv(", Follow::Quoted),
    ("System.getenv(", Follow::Quoted),
    ("System.get_env(", Follow::Quoted),
    ("ENV[", Follow::Quoted),
    ("ENV.fetch(", Follow::Quoted),
    ("getenv(", Follow::Quoted),
];

/// What sets one.
const ENV_SETS: &[(&str, Follow)] = &[("env::set_var(", Follow::Quoted)];

fn env_in_code(source: &str, lines: &Lines, symbols: &[Symbol], found: &mut Vec<Interface>) {
    for (kind, patterns) in [
        (InterfaceKind::EnvRead, ENV_READS),
        (InterfaceKind::EnvSet, ENV_SETS),
    ] {
        for (pattern, follow) in patterns {
            for (at, _) in source.match_indices(pattern) {
                let rest = &source[at + pattern.len()..];
                let name = match follow {
                    Follow::Quoted => quoted(rest).filter(|name| is_identifier(name)),
                    Follow::Identifier => {
                        let end = rest
                            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                            .unwrap_or(rest.len());
                        Some(rest[..end].to_string()).filter(|name| is_identifier(name))
                    }
                };
                let Some(name) = name else {
                    continue;
                };
                let line = lines.line_of(at);
                found.push(Interface {
                    kind,
                    name,
                    line,
                    symbol: enclosing(symbols, line),
                    handlers: Vec::new(),
                    method: None,
                });
            }
        }
    }
}

// -- routes ------------------------------------------------------------------

/// The calls that register a route, by name.
const REGISTER: &[&str] = &[
    "route",
    "nest",
    "route_service",
    "handle",
    "Handle",
    "HandleFunc",
    "handleFunc",
    "resource",
    "service",
    "mount",
    "MapGet",
    "MapPost",
    "MapPut",
    "MapDelete",
    "MapPatch",
    "MapMethods",
    "RequestMapping",
    "GetMapping",
    "PostMapping",
    "PutMapping",
    "DeleteMapping",
    "PatchMapping",
    "Path",
];

/// The HTTP verbs, which register a route on a router and request one on
/// anything else.
const VERBS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "head", "options", "all", "any", "use", "GET", "POST",
    "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "Get", "Post", "Put", "Patch", "Delete", "Head",
    "Options",
];

/// The receivers a verb registers a route on.
const ROUTERS: &[&str] = &[
    "app", "router", "routes", "r", "mux", "server", "srv", "group", "g", "route", "sub",
];

/// The verbs that name an HTTP method, in lowercase: `all`, `any` and `use`
/// name none.
const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// The methods a Dart client calls a route with.
const DART_METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// The calls that request a route, by name.
const REQUEST: &[&str] = &[
    "fetch",
    "request",
    "Request",
    "NewRequest",
    "NewRequestWithContext",
    "open",
    "ajax",
    "getJSON",
    "apiFetch",
    "daemonFetch",
    "$http",
];

/// The names in a registration call that are never its handler.
const NOT_A_HANDLER: &[&str] = &[
    "to",
    "web",
    "axum",
    "routing",
    "self",
    "this",
    "req",
    "res",
    "next",
    "ctx",
    "true",
    "false",
    "null",
    "None",
    "new",
    "Router",
    "router",
    "route",
    "async",
    "await",
    "move",
    "fn",
    "function",
    "lambda",
    "def",
    "handler",
    "handlers",
    "with_state",
    "layer",
    "http",
    "HttpResponse",
    "Ok",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Register,
    Request,
}

/// Every route literal a registration or a request call holds.
fn routes(
    source: &str,
    lines: &Lines,
    symbols: &[Symbol],
    language: Language,
    found: &mut Vec<Interface>,
) {
    let mut characters = source.char_indices().peekable();
    let mut last_angle = None;
    let mut quotes = source
        .match_indices(['"', '\'', '`'])
        .map(|(at, _)| at)
        .peekable();
    for (open, _) in source.match_indices('(') {
        while quotes.peek().is_some_and(|at| *at <= open) {
            quotes.next();
        }
        let scan_end = open
            .saturating_add(1)
            .saturating_add(crate::parser::STATEMENT_SCAN_MAX);
        if quotes.peek().is_none_or(|at| *at >= scan_end) {
            continue;
        }
        while let Some(&(at, ch)) = characters.peek() {
            if at >= open {
                break;
            }
            if ch == '<' {
                last_angle = Some(at);
            }
            characters.next();
        }
        let head = &source[..open];
        // A generic between the callee and the paren: `get<T>(`.
        let head = match head.ends_with('>') {
            true => match last_angle {
                Some(at) => &head[..at],
                None => head,
            },
            false => head,
        };
        let chain_start = head
            .trim_end_matches(|c: char| {
                c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '$' | '!')
            })
            .len();
        let chain = &head[chain_start..];
        let mut segments = chain
            .split(['.', ':'])
            .map(|s| s.trim_end_matches('!'))
            .filter(|s| !s.is_empty());
        let Some(callee) = segments.next_back() else {
            continue;
        };
        let receiver = segments.next_back();
        let before = head[..chain_start].trim_end();
        let decorated = before.ends_with('@') || before.ends_with("#[");
        // A decorator or an attribute always registers; a verb registers on
        // a router and requests on anything else.
        let on_router = receiver.is_some_and(|receiver| ROUTERS.contains(&receiver));
        let role = if REGISTER.contains(&callee) || (decorated && callee.ends_with("Mapping")) {
            Role::Register
        } else if VERBS.contains(&callee) {
            match decorated || on_router {
                true => Role::Register,
                false => Role::Request,
            }
        } else if REQUEST.contains(&callee) {
            Role::Request
        } else {
            continue;
        };
        let rest = &source[open + 1..];
        let end = crate::parser::statement_end(rest, ')');
        let args = &rest[..end];
        // A Dart client is handed the path below the prefix its base URL
        // holds, so its first argument is a route with or without a leading
        // `/`, and an interpolation in it stands for a segment.
        let dart_request =
            language == Language::Dart && role == Role::Request && DART_METHODS.contains(&callee);
        let found_literal = match dart_request {
            true => dart_route_use(args).map(|path| (path, 0)),
            false => route_literal(args),
        };
        let Some((literal, after)) = found_literal else {
            continue;
        };
        let line = lines.line_of(open);
        let (kind, symbol, handlers) = match role {
            Role::Request => (
                InterfaceKind::RouteUse,
                enclosing(symbols, line),
                Vec::new(),
            ),
            Role::Register => {
                let symbol = match decorated {
                    true => next_after(symbols, line).or_else(|| enclosing(symbols, line)),
                    false => enclosing(symbols, line),
                };
                (InterfaceKind::Route, symbol, handler_names(&args[after..]))
            }
        };
        found.push(Interface {
            kind,
            name: literal,
            line,
            symbol,
            handlers,
            method: http_method(callee),
        });
    }
}

/// The HTTP method a callee names, uppercased: `Get` and `get` are both
/// `GET`, and a call named `route` or `use` names none.
fn http_method(callee: &str) -> Option<String> {
    let lowercase = callee.to_ascii_lowercase();
    METHODS
        .contains(&lowercase.as_str())
        .then(|| lowercase.to_ascii_uppercase())
}

/// The route a Dart request call takes as its first argument, with every
/// interpolation in the form a template literal gives. The leading `/` is
/// optional: the client holds the prefix the path hangs under.
fn dart_route_use(args: &str) -> Option<String> {
    let literal = quoted(args)?;
    let path = without_query(&literal).trim_end_matches('/');
    let template = dart_parameters(path);
    if route_segments(&template).is_empty() || !is_route_template(&template) {
        return None;
    }
    Some(template)
}

/// A Dart literal up to its query string. A `?` inside an interpolation is
/// part of a Dart expression, such as `${user?.id}`, and starts no query.
fn without_query(literal: &str) -> &str {
    let mut at = 0;
    while let Some(next) = literal[at..].find(['?', '$']) {
        let next = at + next;
        if literal.as_bytes()[next] == b'?' {
            return &literal[..next];
        }
        at = next + 1;
        // Past `$` an interpolation is `{…}` or a name; a name holds no `?`,
        // so only a braced one is stepped over.
        let Some(inner) = literal[at..].strip_prefix('{') else {
            continue;
        };
        match inner.find('}') {
            Some(close) => at += 1 + close + 1,
            // An interpolation nothing closes is text like any other.
            None => break,
        }
    }
    literal
}

/// Whether a template reads as a path. An interpolation holds a Dart
/// expression, which is any text, so only what is outside one is a path.
fn is_route_template(template: &str) -> bool {
    let is_path = |c: char| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/');
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        if !rest[..at].chars().all(is_path) {
            return false;
        }
        // Past `$` stands `{expression}`: `dart_parameters` writes every
        // interpolation in that form, and a `$` on its own is not a path.
        let Some((_, left)) = rest[at + 1..]
            .strip_prefix('{')
            .and_then(|inner| inner.split_once('}'))
        else {
            return false;
        };
        rest = left;
    }
    rest.chars().all(is_path)
}

/// Every `$name` of a Dart string written as `${name}`, which is the form a
/// template literal gives and the form a wildcard segment is read in. A
/// `${expr}` is already in that form.
fn dart_parameters(path: &str) -> String {
    let mut template = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(at) = rest.find('$') {
        template.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (parameter, left) = match after.strip_prefix('{') {
            Some(inner) => match inner.split_once('}') {
                Some((parameter, left)) => (parameter, left),
                // An interpolation nothing closes is text like any other.
                None => {
                    template.push_str(&rest[at..]);
                    return template;
                }
            },
            None => {
                let end = after
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..end], &after[end..])
            }
        };
        if parameter.is_empty() {
            template.push('$');
            rest = after;
            continue;
        }
        template.push_str("${");
        template.push_str(parameter);
        template.push('}');
        rest = left;
    }
    template.push_str(rest);
    template
}

/// The first string literal of a call's arguments that reads as a route: it
/// starts with `/` and has two or more segments. Answers the route and where
/// the literal ends.
fn route_literal(args: &str) -> Option<(String, usize)> {
    let mut at = 0;
    while let Some(open) = args[at..].find(['"', '\'', '`']) {
        let open = at + open;
        let quote = args[open..].chars().next()?;
        let inner = &args[open + quote.len_utf8()..];
        let close = inner.find(quote)?;
        let content = &inner[..close];
        at = open + quote.len_utf8() + close + quote.len_utf8();
        let route = content.split('?').next().unwrap_or("");
        if route.starts_with('/') && route_segments(route).len() >= 2 {
            let route = route.trim_end_matches('/');
            return Some((route.to_string(), at));
        }
    }
    None
}

/// The identifiers a registration passes after its literal, up to the body
/// of a closure: the handler is among them.
fn handler_names(after: &str) -> Vec<String> {
    let end = ["=>", "{", "->"]
        .iter()
        .filter_map(|stop| after.find(stop))
        .min()
        .unwrap_or(after.len());
    let mut names = Vec::new();
    for token in
        after[..end].split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | ':')))
    {
        let name = token.rsplit(['.', ':']).next().unwrap_or(token);
        if name.is_empty()
            || name.starts_with(|c: char| c.is_ascii_digit())
            || VERBS.contains(&name)
            || NOT_A_HANDLER.contains(&name)
            || names.iter().any(|seen| seen == name)
        {
            continue;
        }
        names.push(name.to_string());
        if names.len() == 4 {
            break;
        }
    }
    names
}

/// The segments of a route: `/v1/items/{id}` is `v1`, `items` and `{id}`.
pub fn route_segments(route: &str) -> Vec<&str> {
    route
        .split('?')
        .next()
        .unwrap_or("")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// Whether a segment stands for any value: `{id}`, `:id`, `<id>`, `*rest`,
/// `${id}`, `$id`, `{}`.
pub fn is_wildcard(segment: &str) -> bool {
    segment.starts_with([':', '{', '<', '*', '$']) || segment.contains('$') || segment.contains('{')
}

/// Whether a route use fits a template, and how well: `exact` where every
/// segment is the same, `heuristic` where a wildcard stood in for one.
///
/// A use with no leading `/` is a path below a prefix the client holds —
/// `users/1` under `/api` — so it fits the last segments of a template, and
/// the prefix it does not name makes the match `heuristic`.
pub fn route_match(used: &str, template: &str) -> Option<&'static str> {
    let below_a_prefix = !used.trim_start().starts_with('/');
    let used = route_segments(used);
    let all = route_segments(template);
    let template = match below_a_prefix && used.len() < all.len() {
        true => &all[all.len() - used.len()..],
        false => &all[..],
    };
    if used.is_empty() || used.len() != template.len() {
        return None;
    }
    let mut exact = !below_a_prefix;
    for (a, b) in used.iter().zip(template) {
        if a == b {
            continue;
        }
        if is_wildcard(a) || is_wildcard(b) {
            exact = false;
            continue;
        }
        return None;
    }
    Some(match exact {
        true => "exact",
        false => "heuristic",
    })
}

// -- helpers -----------------------------------------------------------------

/// The innermost definition whose lines hold `line`.
fn enclosing(symbols: &[Symbol], line: u32) -> Option<usize> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start_line <= line && line <= s.end_line)
        .max_by_key(|(_, s)| (s.start_line, std::cmp::Reverse(s.end_line)))
        .map(|(at, _)| at)
}

/// The first definition that starts within a few lines after `line`: what a
/// decorator or an attribute on `line` is on.
fn next_after(symbols: &[Symbol], line: u32) -> Option<usize> {
    symbols
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start_line > line && s.start_line <= line + 20)
        .min_by_key(|(_, s)| s.start_line)
        .map(|(at, _)| at)
}

/// The first quoted string of `text`, past any whitespace.
fn quoted(text: &str) -> Option<String> {
    let text = text.trim_start();
    let quote = text
        .chars()
        .next()
        .filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let inner = &text[quote.len_utf8()..];
    let close = inner.find(quote)?;
    Some(inner[..close].to_string())
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(|c: char| c.is_ascii_digit())
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(found: &[Interface]) -> Vec<(InterfaceKind, &str, u32)> {
        found
            .iter()
            .map(|i| (i.kind, i.name.as_str(), i.line))
            .collect()
    }

    fn symbol(name: &str, start: u32, end: u32) -> Symbol {
        Symbol {
            kind: "function".into(),
            name: name.into(),
            qualified_name: name.into(),
            start_line: start,
            end_line: end,
            signature: String::new(),
            doc: None,
            is_test: false,
        }
    }

    /// Each manifest names its package and its dependencies, a path
    /// dependency told from one by name.
    #[test]
    fn every_manifest_names_its_package_and_its_dependencies() {
        let cargo = "[package]\nname = \"api-types\"\n\n[dependencies]\nserde = \"1\"\nweb = { path = \"../web\" }\nrenamed = { package = \"real\", version = \"1\" }\n\n[dev-dependencies.tools]\npath = \"../tools\"\n";
        assert_eq!(
            kinds(&read("Cargo.toml", Language::Toml, cargo, &[])),
            [
                (InterfaceKind::Package, "api-types", 2),
                (InterfaceKind::Dependency, "serde", 5),
                (InterfaceKind::PathDependency, "web", 6),
                (InterfaceKind::Dependency, "real", 7),
                (InterfaceKind::PathDependency, "tools", 9),
            ]
        );

        let npm = "{\n  \"name\": \"web\",\n  \"dependencies\": {\n    \"api-types\": \"^1.0.0\",\n    \"shared\": \"file:../shared\"\n  },\n  \"devDependencies\": { \"vitest\": \"3\" },\n  \"workspaces\": [\"packages/ui\", \"packages/*\"]\n}\n";
        assert_eq!(
            kinds(&read("package.json", Language::Json, npm, &[])),
            [
                (InterfaceKind::Package, "web", 2),
                (InterfaceKind::Dependency, "api-types", 4),
                (InterfaceKind::PathDependency, "shared", 5),
                (InterfaceKind::Dependency, "vitest", 7),
                (InterfaceKind::PathDependency, "ui", 8),
            ]
        );

        let pubspec = "name: app\ndependencies:\n  http: ^1.0\n  api_types:\n    path: ../api_types\ndev_dependencies:\n  test: any\n";
        assert_eq!(
            kinds(&read("pubspec.yaml", Language::Yaml, pubspec, &[])),
            [
                (InterfaceKind::Package, "app", 1),
                (InterfaceKind::Dependency, "http", 3),
                (InterfaceKind::PathDependency, "api_types", 4),
                (InterfaceKind::Dependency, "test", 7),
            ]
        );

        let go = "module example.com/api\n\ngo 1.22\n\nrequire (\n\tgithub.com/a/b v1.2.3 // indirect\n)\nrequire github.com/c/d v1\nreplace github.com/a/b => ../b\n";
        assert_eq!(
            kinds(&read("go.mod", Language::Manifest, go, &[])),
            [
                (InterfaceKind::Package, "example.com/api", 1),
                (InterfaceKind::Dependency, "github.com/a/b", 6),
                (InterfaceKind::Dependency, "github.com/c/d", 8),
                (InterfaceKind::PathDependency, "github.com/a/b", 9),
            ]
        );

        let pom = "<project>\n  <parent>\n    <groupId>org.parent</groupId>\n    <artifactId>parent</artifactId>\n  </parent>\n  <groupId>com.example</groupId>\n  <artifactId>api</artifactId>\n  <dependencies>\n    <dependency>\n      <groupId>com.example</groupId>\n      <artifactId>types</artifactId>\n    </dependency>\n  </dependencies>\n</project>\n";
        assert_eq!(
            kinds(&read("pom.xml", Language::Manifest, pom, &[])),
            [
                (InterfaceKind::Package, "com.example:api", 7),
                (InterfaceKind::Dependency, "com.example:types", 11),
            ]
        );

        let gradle = "dependencies {\n    implementation 'com.example:types:1.0'\n    testImplementation(\"org.junit:junit:5\")\n    implementation(project(\":shared\"))\n}\n";
        assert_eq!(
            kinds(&read("build.gradle", Language::Manifest, gradle, &[])),
            [
                (InterfaceKind::Dependency, "com.example:types", 2),
                (InterfaceKind::Dependency, "org.junit:junit", 3),
                (InterfaceKind::PathDependency, "shared", 4),
            ]
        );
        assert_eq!(
            kinds(&read(
                "settings.gradle.kts",
                Language::Kotlin,
                "rootProject.name = \"api\"\n",
                &[]
            )),
            [(InterfaceKind::Package, "api", 1)]
        );
    }

    /// A variable is set by a `.env` line, a compose or workflow entry and
    /// `set_var`, and read by each language's own call.
    #[test]
    fn an_environment_variable_is_set_and_read_by_each_syntax() {
        assert_eq!(
            kinds(&read(
                ".env",
                Language::Manifest,
                "# comment\nAPI_TOKEN=secret\nexport PORT=8080\n",
                &[]
            )),
            [
                (InterfaceKind::EnvSet, "API_TOKEN", 2),
                (InterfaceKind::EnvSet, "PORT", 3),
            ]
        );
        assert_eq!(
            kinds(&read(
                "docker-compose.yml",
                Language::Yaml,
                "services:\n  api:\n    environment:\n      - API_TOKEN=x\n      PORT: 80\n    image: api\n",
                &[]
            )),
            [
                (InterfaceKind::EnvSet, "API_TOKEN", 4),
                (InterfaceKind::EnvSet, "PORT", 5),
            ]
        );
        assert_eq!(
            kinds(&read(
                ".github/workflows/ci.yml",
                Language::Yaml,
                "env:\n  CARGO_TERM_COLOR: always\njobs:\n  test:\n    runs-on: ubuntu\n",
                &[]
            )),
            [(InterfaceKind::EnvSet, "CARGO_TERM_COLOR", 2)]
        );

        let symbols = [symbol("main", 1, 4)];
        let rust = "fn main() {\n    let token = std::env::var(\"API_TOKEN\");\n    env::set_var(\"RUST_LOG\", \"info\");\n}\n";
        let found = read("src/main.rs", Language::Rust, rust, &symbols);
        assert_eq!(
            kinds(&found),
            [
                (InterfaceKind::EnvRead, "API_TOKEN", 2),
                (InterfaceKind::EnvSet, "RUST_LOG", 3),
            ]
        );
        assert_eq!(found[0].symbol, Some(0), "the read sits in main");

        for (path, language, text) in [
            (
                "a.ts",
                Language::TypeScript,
                "const t = process.env.API_TOKEN;",
            ),
            ("a.py", Language::Python, "t = os.environ[\"API_TOKEN\"]"),
            (
                "A.cs",
                Language::CSharp,
                "var t = Environment.GetEnvironmentVariable(\"API_TOKEN\");",
            ),
            (
                "a.dart",
                Language::Dart,
                "final t = Platform.environment[\"API_TOKEN\"];",
            ),
            ("a.go", Language::Go, "t := os.Getenv(\"API_TOKEN\")"),
        ] {
            assert_eq!(
                kinds(&read(path, language, text, &[])),
                [(InterfaceKind::EnvRead, "API_TOKEN", 1)],
                "{path}"
            );
        }
    }

    /// A literal in a registration call is a route template, with the
    /// handler the call names; a literal in a request call is a route use;
    /// a literal with one segment is neither.
    #[test]
    fn a_route_is_registered_by_a_router_and_used_by_a_request() {
        let symbols = [symbol("router", 1, 5), symbol("get_item", 7, 9)];
        let rust = "pub fn router() -> Router {\n    Router::new()\n        .route(\"/v1/items/{id}\", get(get_item).delete(drop_item))\n        .nest(\"/v1\", other())\n}\n\npub async fn get_item() {\n    client.get(\"/v1/items/42\").await;\n}\n";
        let found = read("src/lib.rs", Language::Rust, rust, &symbols);
        assert_eq!(
            kinds(&found),
            [
                (InterfaceKind::Route, "/v1/items/{id}", 3),
                (InterfaceKind::RouteUse, "/v1/items/42", 8),
            ]
        );
        assert_eq!(found[0].symbol, Some(0));
        assert_eq!(found[0].handlers, ["get_item", "drop_item"]);
        assert_eq!(found[1].symbol, Some(1));

        let symbols = [symbol("fetchItem", 1, 4), symbol("index", 7, 9)];
        let ts = "export async function fetchItem(id: string) {\n  await fetch(`/v1/items/${id}?full=1`);\n  await axios.get('/v1/items/all/');\n}\napp.get('/v1/items/:id', (req, res) => res.json(index()));\n\nfunction index() {\n  return [];\n}\n";
        let found = read("src/client.ts", Language::TypeScript, ts, &symbols);
        assert_eq!(
            kinds(&found),
            [
                (InterfaceKind::RouteUse, "/v1/items/${id}", 2),
                (InterfaceKind::RouteUse, "/v1/items/all", 3),
                (InterfaceKind::Route, "/v1/items/:id", 5),
            ]
        );
        assert_eq!(found[2].symbol, None, "registered at file scope");
        assert!(found[2].handlers.is_empty(), "{:?}", found[2].handlers);

        // A decorator registers the definition under it.
        let symbols = [symbol("show", 2, 3)];
        let python = "@app.route(\"/v1/items/<id>\")\ndef show(id):\n    return id\n";
        let found = read("app.py", Language::Python, python, &symbols);
        assert_eq!(kinds(&found), [(InterfaceKind::Route, "/v1/items/<id>", 1)]);
        assert_eq!(found[0].symbol, Some(0));
    }

    /// A bounded route scan never cuts through a multi-byte character.
    #[test]
    fn a_route_scan_cuts_only_at_a_character_boundary() {
        let source = format!("client.get(\"/v1/{}─\")", "a".repeat(594));

        assert!(read("client.ts", Language::TypeScript, &source, &[]).is_empty());
    }

    /// Each of the five methods a Dart client calls is a route use: the
    /// method it names, the path it asks for, and the line of the call.
    #[test]
    fn a_dart_call_of_each_method_is_a_route_use_with_its_method_and_line() {
        let dart = "class Repositories {\n  Future<void> load(String id) async {\n    await _api.get('auth/me');\n    await _api.post('users', {'name': 'x'});\n    await _api.put('users/$id/role', {'role': 'admin'});\n    await _api.patch('members/$id/active', {'active': true});\n    await _api.delete('teams/$id');\n  }\n}\n";
        let found = read("lib/api.dart", Language::Dart, dart, &[]);
        let calls: Vec<String> = found
            .iter()
            .map(|i| {
                format!(
                    "{} {} {} {}",
                    i.kind.as_str(),
                    i.method.as_deref().unwrap_or("-"),
                    i.name,
                    i.line
                )
            })
            .collect();
        assert_eq!(
            calls,
            [
                "route_use GET auth/me 3",
                "route_use POST users 4",
                "route_use PUT users/${id}/role 5",
                "route_use PATCH members/${id}/active 6",
                "route_use DELETE teams/${id} 7",
            ]
        );
    }

    /// An interpolation of a Dart string is one path parameter, written the
    /// way a template literal gives it.
    #[test]
    fn a_dart_interpolation_is_one_path_parameter() {
        let dart = "Future<void> f(String id) async {\n  await _api.put('users/$id/role', body);\n  await _api.get('members/${user.id}');\n  await _api.get('teams/${team.id.toString()}/members');\n  await _api.get('members/${user?.id}/cards');\n  await _api.get('users?page=1');\n}\n";
        let found = read("lib/api.dart", Language::Dart, dart, &[]);
        assert_eq!(
            kinds(&found),
            [
                (InterfaceKind::RouteUse, "users/${id}/role", 2),
                (InterfaceKind::RouteUse, "members/${user.id}", 3),
                (
                    InterfaceKind::RouteUse,
                    "teams/${team.id.toString()}/members",
                    4
                ),
                (InterfaceKind::RouteUse, "members/${user?.id}/cards", 5),
                (InterfaceKind::RouteUse, "users", 6),
            ]
        );
        for use_of in found.iter().filter(|i| i.name.contains('$')) {
            assert_eq!(
                use_of.name.matches("${").count(),
                1,
                "one parameter in {}",
                use_of.name
            );
        }
    }

    /// A use fits a template segment by segment: the same segments are an
    /// exact match, a wildcard on either side a heuristic one, and a
    /// different count no match.
    #[test]
    fn a_route_use_fits_a_template_segment_by_segment() {
        assert_eq!(route_match("/v1/items/42", "/v1/items/42"), Some("exact"));
        assert_eq!(
            route_match("/v1/items/42", "/v1/items/{id}"),
            Some("heuristic")
        );
        assert_eq!(
            route_match("/v1/items/${id}", "/v1/items/:id"),
            Some("heuristic")
        );
        assert_eq!(route_match("/v1/items", "/v1/items/{id}"), None);
        assert_eq!(route_match("/v1/goals/42", "/v1/items/{id}"), None);
        assert_eq!(
            route_match("/v1/items/42?x=1", "/v1/items/42/"),
            Some("exact")
        );
    }

    /// A use with no leading `/` names the path below a prefix the client
    /// holds, so it fits the last segments of a template, as a guess.
    #[test]
    fn a_route_use_below_a_prefix_fits_the_end_of_a_template() {
        assert_eq!(
            route_match("users/${id}/role", "/api/users/{id}/role"),
            Some("heuristic")
        );
        assert_eq!(route_match("users", "/api/v1/users"), Some("heuristic"));
        assert_eq!(route_match("auth/me", "/api/auth/me"), Some("heuristic"));
        assert_eq!(route_match("users/1", "/api/teams/1"), None);
        assert_eq!(
            route_match("/users/1", "/api/users/1"),
            None,
            "a path that starts at the root names the whole route"
        );
    }
}
