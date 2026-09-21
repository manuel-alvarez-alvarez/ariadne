//! One file's text to the definitions in it, and to the names it names.
//!
//! A code file is read with its language's tags query: every definition the
//! query captures becomes a [`Symbol`], qualified by the definitions it sits
//! inside, with the signature read off its first lines, the doc comment
//! read by the language's comment syntax where the query captured none, and
//! the test marker rule applied. A Markdown file is read for its headings.
//!
//! Every reference the query captures becomes a [`Reference`], and the
//! import statements of the file become [`Import`]s. Neither is an edge yet:
//! a reference names a name, and [`crate::resolve`] is what finds the
//! definition behind it.

use std::ops::Range;

use tree_sitter::StreamingIterator;
use tree_sitter_tags::TagsContext;

use crate::languages::{DocSyntax, Language, TestRule};

/// One definition of a file: what the index stores per blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// The tags vocabulary: `function`, `method`, `class`, `module`,
    /// `interface`, `type`, `macro`, `constant`; `test` for a test the language
    /// marks by a call; `heading` in Markdown.
    pub kind: String,
    pub name: String,
    /// The name under the scopes it sits in, joined by the language's
    /// separator: `GitManager::add_worktree`.
    pub qualified_name: String,
    /// 1-based, inclusive.
    pub start_line: u32,
    pub end_line: u32,
    /// The definition's opening, on one line, up to its body.
    pub signature: String,
    pub doc: Option<String>,
    pub is_test: bool,
}

/// What one symbol is to another. The whole edge vocabulary: a reference the
/// parser finds is one of these, and so is every row of `edges`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// A call, a macro invocation or a method send.
    Calls,
    /// Any other mention: a type annotation, a constructed class.
    References,
    /// A Rust `impl Trait for Type`, or a Java or PHP
    /// `class A implements I`.
    Implements,
    /// A class or interface that names a base: `class A extends B`,
    /// `class A : B` in C#, `class A(B)` in Python.
    Extends,
    /// A name an import statement brought into the file.
    Imports,
}

impl EdgeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::References => "references",
            EdgeKind::Implements => "implements",
            EdgeKind::Extends => "extends",
            EdgeKind::Imports => "imports",
        }
    }

    /// The edge a tags capture of this syntax type makes. Every capture the
    /// index does not name is a plain mention.
    fn of_capture(name: &str) -> EdgeKind {
        match name {
            "call" | "send" => EdgeKind::Calls,
            "implementation" => EdgeKind::Implements,
            "extends" => EdgeKind::Extends,
            _ => EdgeKind::References,
        }
    }
}

/// One name a blob names, before anything is known about where it is
/// defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub kind: EdgeKind,
    pub name: String,
    /// 1-based.
    pub line: u32,
    /// The index in [`Parsed::symbols`] of the definition the reference sits
    /// in. `None` at file scope.
    pub from: Option<usize>,
}

/// One import statement of a blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    /// The module it reads from: `crate::m`, `./m`, `pkg.m`, `System.Text`.
    pub module: String,
    /// The name it brought in. `None` where the statement brings in the
    /// whole module and names nothing.
    pub name: Option<String>,
    /// 1-based.
    pub line: u32,
}

/// Everything one blob holds: what it defines, what it names, and what it
/// imports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub imports: Vec<Import>,
}

/// How long a signature or a doc is allowed to run, in characters. A
/// signature is what a search answer prints per line, and 200 is a whole
/// parameter list; a doc is what an outline may show, and it is a summary,
/// not a manual.
const SIGNATURE_MAX: usize = 200;
const DOC_MAX: usize = 1000;
pub(crate) const STATEMENT_SCAN_MAX: usize = 600;
const NESTING_DEPTH_MAX: usize = 128;
const PARSE_MAX: std::time::Duration = std::time::Duration::from_secs(1);

struct ParseDeadline {
    at: std::time::Instant,
    id: u64,
    flag: std::sync::Weak<std::sync::atomic::AtomicUsize>,
}

impl PartialEq for ParseDeadline {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ParseDeadline {}

impl PartialOrd for ParseDeadline {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ParseDeadline {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.at.cmp(&self.at).then_with(|| other.id.cmp(&self.id))
    }
}

fn parse_cancellation() -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    static DEADLINES: std::sync::OnceLock<std::sync::mpsc::Sender<ParseDeadline>> =
        std::sync::OnceLock::new();
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sender = DEADLINES.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<ParseDeadline>();
        let _ = std::thread::Builder::new()
            .name("knowledge-parse-deadline".into())
            .spawn(move || parse_deadlines(receiver));
        sender
    });
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let _ = sender.send(ParseDeadline {
        at: std::time::Instant::now() + PARSE_MAX,
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        flag: std::sync::Arc::downgrade(&flag),
    });
    flag
}

fn parse_deadlines(receiver: std::sync::mpsc::Receiver<ParseDeadline>) {
    let mut pending = std::collections::BinaryHeap::new();
    loop {
        if pending.is_empty() {
            let Ok(deadline) = receiver.recv() else {
                return;
            };
            pending.push(deadline);
        }
        let next = pending
            .peek()
            .map(|deadline| deadline.at)
            .unwrap_or_else(std::time::Instant::now);
        match receiver.recv_timeout(next.saturating_duration_since(std::time::Instant::now())) {
            Ok(deadline) => pending.push(deadline),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let now = std::time::Instant::now();
                while pending.peek().is_some_and(|deadline| deadline.at <= now) {
                    let Some(deadline) = pending.pop() else {
                        break;
                    };
                    if let Some(flag) = deadline.flag.upgrade() {
                        flag.store(1, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The definitions of `source`, read as `language`.
pub fn parse(language: Language, source: &str) -> Vec<Symbol> {
    read(language, source).symbols
}

/// Everything the index reads off one file: its definitions, its references
/// and its imports.
pub fn read(language: Language, source: &str) -> Parsed {
    // An outline-only language has no tags query, and so no references and no
    // imports: its symbols are read off its own shape.
    let outline = match language {
        Language::Markdown => Some(markdown_outline(source)),
        Language::Yaml => Some(yaml_outline(source)),
        Language::Toml => Some(toml_outline(source)),
        Language::Json => Some(json_outline(source)),
        Language::Html => Some(html_outline(source)),
        Language::Css => Some(css_outline(source)),
        Language::Sql => Some(sql_outline(source)),
        _ => None,
    };
    if let Some(symbols) = outline {
        return Parsed {
            symbols,
            ..Default::default()
        };
    }
    let Some(configuration) = language.tags() else {
        return Parsed::default();
    };
    let mut context = TagsContext::new();
    let cancellation = parse_cancellation();
    let Ok((tags, _)) =
        context.generate_tags(configuration, source.as_bytes(), Some(&cancellation))
    else {
        return Parsed::default();
    };
    let lines = Lines::of(source);
    let mut items: Vec<Item> = tags
        .filter_map(Result::ok)
        .filter_map(|tag| {
            let range = tag.range;
            let name_range = tag.name_range;
            if name_range.start < range.start || name_range.end > range.end {
                return None;
            }
            let name = source.get(name_range.clone())?.to_string();
            source.get(range.clone())?;
            Some(Item {
                kind: configuration
                    .syntax_type_name(tag.syntax_type_id)
                    .to_string(),
                name,
                lines: lines.of_range(&range),
                range,
                name_range,
                docs: tag.docs,
                is_definition: tag.is_definition,
            })
        })
        .collect();
    // Document order, an outer definition before the ones inside it.
    items.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
    });

    let separator = language.separator();
    let doc_syntax = language.doc_syntax();
    let test_rule = language.test_rule();
    // (where the scope ends, its qualified name, the symbol it is)
    let mut scopes: Vec<(usize, String, Option<usize>)> = Vec::new();
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut references: Vec<Reference> = Vec::new();
    // The subject of each reference that names its own source rather than
    // sitting in one: `impl Trait for Type` is an edge from `Type`.
    let mut subjects: Vec<Option<String>> = Vec::new();
    for item in items {
        while scopes
            .last()
            .is_some_and(|(end, _, _)| *end <= item.range.start)
        {
            scopes.pop();
        }
        let parent = scopes.last().map(|(_, qualified, _)| qualified.as_str());
        let qualify = |name: &str| match parent {
            Some(parent) => format!("{parent}{separator}{name}"),
            None => name.to_string(),
        };
        // The definition a reference here sits in: the innermost scope that
        // is one, past an `impl` block, which is no definition of its own.
        let enclosing = scopes.iter().rev().find_map(|(_, _, at)| *at);
        if item.is_definition {
            let qualified_name = qualify(&item.name);
            if is_scope(&item.kind) {
                scopes.push((item.range.end, qualified_name.clone(), Some(symbols.len())));
            }
            let doc = item
                .docs
                .as_deref()
                .map(clean_doc)
                .filter(|doc| !doc.is_empty())
                .or_else(|| doc_of(doc_syntax, source, &lines, &item));
            let is_test = match test_rule {
                TestRule::Attribute(names) => has_attribute(source, &lines, &item, names),
                TestRule::Annotation(names) => has_annotation(source, &lines, &item, names),
                TestRule::NamePrefix(prefix) => item.name.starts_with(prefix),
                TestRule::AnnotationOrNamePrefix(names, prefix) => {
                    has_annotation(source, &lines, &item, names) || item.name.starts_with(prefix)
                }
                TestRule::Call(_) | TestRule::None => false,
            };
            symbols.push(Symbol {
                kind: item.kind.clone(),
                name: item.name.clone(),
                qualified_name,
                start_line: item.lines.0,
                end_line: item.lines.1,
                signature: signature_of(source, &item.range, item.name_range.end),
                doc: doc.map(|doc| cut(&doc, DOC_MAX)),
                is_test,
            });
        } else if item.kind == "implementation" && language == Language::Rust {
            // `impl Type { … }` in Rust: a scope for the methods in it, and
            // no definition of its own. `impl Trait for Type` is also an
            // edge from `Type` to `Trait`. Java and PHP tag the interfaces
            // of an `implements` clause by the same kind: each is a plain
            // edge from the class it sits in, below.
            let header = &source[item.range.clone()];
            let subject = impl_subject(header);
            if let Some(trait_name) = impl_trait(header) {
                references.push(Reference {
                    kind: EdgeKind::Implements,
                    name: trait_name,
                    line: item.lines.0,
                    from: None,
                });
                subjects.push(Some(subject.clone()));
            }
            scopes.push((item.range.end, qualify(&subject), None));
        } else if let TestRule::Call(names) = test_rule
            && item.kind == "call"
            && names.contains(&item.name.as_str())
            && !has_receiver(source, item.name_range.start)
        {
            let name = first_string_argument(&source[item.range.clone()])
                .unwrap_or_else(|| item.name.clone());
            let qualified_name = qualify(&name);
            // A scope of its own, so what the test body calls is the test's.
            scopes.push((item.range.end, qualified_name.clone(), Some(symbols.len())));
            symbols.push(Symbol {
                kind: "test".into(),
                qualified_name,
                name,
                start_line: item.lines.0,
                end_line: item.lines.1,
                signature: signature_of(source, &item.range, item.name_range.end),
                doc: None,
                is_test: true,
            });
        } else {
            references.push(Reference {
                kind: EdgeKind::of_capture(&item.kind),
                name: item.name.clone(),
                line: item.lines.0,
                from: enclosing,
            });
            subjects.push(None);
        }
    }
    // A reference that names its own source points at the definition of that
    // name in this same file, where there is one.
    for (reference, subject) in references.iter_mut().zip(subjects) {
        if let Some(subject) = subject {
            reference.from = symbols.iter().position(|symbol| symbol.name == subject);
        }
    }
    // One definition that names another names it once, at the first line it
    // does: the edge between them is one edge however often it is written.
    let mut named = std::collections::HashSet::new();
    references
        .retain(|reference| named.insert((reference.kind, reference.name.clone(), reference.from)));
    let imports = imports_of(language, source);
    Parsed {
        symbols,
        references,
        imports,
    }
}

/// One tag, with what the parser reads off it.
struct Item {
    kind: String,
    name: String,
    range: Range<usize>,
    name_range: Range<usize>,
    lines: (u32, u32),
    docs: Option<String>,
    is_definition: bool,
}

/// Whether a definition of this kind qualifies the definitions inside it.
fn is_scope(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "module" | "interface" | "function" | "method" | "implementation" | "object"
    )
}

/// Where each line of a file starts, so a byte offset is a line number in
/// one binary search.
pub(crate) struct Lines(Vec<usize>);

impl Lines {
    pub(crate) fn of(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
        Self(starts)
    }

    /// The 1-based line holding byte `at`.
    pub(crate) fn line_of(&self, at: usize) -> u32 {
        self.0.partition_point(|start| *start <= at) as u32
    }

    fn line_start(&self, at: usize) -> usize {
        let line = self
            .0
            .partition_point(|start| *start <= at)
            .saturating_sub(1);
        self.0.get(line).copied().unwrap_or(0)
    }

    /// The 1-based, inclusive first and last line of a byte range. A range
    /// that ends on a newline ends on the line that newline closes.
    fn of_range(&self, range: &Range<usize>) -> (u32, u32) {
        let start = self.line_of(range.start);
        let end = self.line_of(range.end.saturating_sub(1).max(range.start));
        (start, end.max(start))
    }
}

/// The definition's opening, as one line: from its first character to its
/// body, or to the end of the line its name is on once every bracket opened
/// before it has closed.
fn signature_of(source: &str, range: &Range<usize>, name_end: usize) -> String {
    let text = &source[range.clone()];
    let mut depth = 0i32;
    let mut end = text.len();
    for (at, ch) in text.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '{' if depth <= 0 => {
                end = at;
                break;
            }
            '\n' if depth <= 0 && range.start + at >= name_end => {
                end = at;
                break;
            }
            _ => {}
        }
    }
    let one_line = text[..end].split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = one_line
        .trim_end_matches(|c: char| c == '{' || c == ':' || c == ';' || c.is_whitespace())
        .trim_end_matches("=>")
        .trim_end();
    cut(trimmed, SIGNATURE_MAX)
}

/// `text`, cut to `max` characters with an ellipsis where it ran on.
fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", text[..at].trim_end()),
        None => text.to_string(),
    }
}

/// A doc comment as the tags query captured it, without its markers.
fn clean_doc(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(|line| {
            let line = line.trim();
            let line = line
                .strip_prefix("/**")
                .or_else(|| line.strip_prefix("/*"))
                .unwrap_or(line);
            let line = line.strip_suffix("*/").unwrap_or(line);
            let line = line.trim();
            let line = line
                .strip_prefix("///")
                .or_else(|| line.strip_prefix("//"))
                .or_else(|| line.strip_prefix('*'))
                .or_else(|| line.strip_prefix('#'))
                .unwrap_or(line);
            line.trim()
        })
        .collect();
    let first = lines.iter().position(|line| !line.is_empty());
    let last = lines.iter().rposition(|line| !line.is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

/// The doc comment of a definition, read by the language's comment syntax.
fn doc_of(syntax: DocSyntax, source: &str, lines: &Lines, item: &Item) -> Option<String> {
    let doc = match syntax {
        DocSyntax::LinePrefix(prefix) => {
            let doc_lines: Vec<&str> = lines_above(source, lines, item.range.start)
                .filter(|line| !is_attribute_line(line))
                .take_while(|line| line.trim_start().starts_with(prefix))
                .map(|line| line.trim_start()[prefix.len()..].trim())
                .collect();
            doc_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        }
        DocSyntax::Block => {
            let mut doc_lines: Vec<&str> = Vec::new();
            let mut above = lines_above(source, lines, item.range.start).peekable();
            match above.peek().map(|line| line.trim()) {
                // The walk up stops at the line that opens the comment, and
                // that line opens with it: a comment that trails code, or a
                // second comment closing on the way up, is no doc.
                Some(line) if line.ends_with("*/") => {
                    let mut opened = false;
                    for (at, line) in above.enumerate() {
                        doc_lines.push(line);
                        let text = line.trim_start();
                        if let Some(open) = text.find("/*") {
                            opened = open == 0;
                            break;
                        }
                        if at > 0 && text.contains("*/") {
                            break;
                        }
                    }
                    if !opened {
                        doc_lines.clear();
                    }
                }
                _ => doc_lines.extend(above.take_while(|line| line.trim_start().starts_with("//"))),
            }
            clean_doc(&doc_lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
        }
        DocSyntax::Docstring => docstring(
            &source[item.range.clone()],
            item.name_range.end - item.range.start,
        )?,
        DocSyntax::None => return None,
    };
    (!doc.is_empty()).then_some(doc)
}

/// The lines above byte `at`, nearest first.
fn lines_above<'a>(source: &'a str, lines: &Lines, at: usize) -> impl Iterator<Item = &'a str> {
    let line_start = lines.line_start(at);
    source[..line_start].lines().rev()
}

/// A Rust `#[…]` or C# `[…]` attribute on a line of its own.
fn is_attribute_line(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with("#[") || line.starts_with('[')) && line.ends_with(']')
}

/// The docstring of a Python definition: a string literal as the first
/// statement of the body, which starts after the line the signature ends
/// on with a colon, whether that line ends in `\n` or in `\r\n`.
fn docstring(text: &str, after: usize) -> Option<String> {
    let body_at = [":\n", ":\r\n"]
        .into_iter()
        .filter_map(|end| text[after..].find(end).map(|at| after + at + end.len()))
        .min()?;
    let body = text[body_at..].trim_start();
    let quote = ["\"\"\"", "'''"]
        .into_iter()
        .find(|quote| body.starts_with(quote))?;
    let inner = &body[quote.len()..];
    let end = inner.find(quote)?;
    let lines: Vec<&str> = inner[..end].lines().map(str::trim).collect();
    Some(clean_doc(&lines.join("\n")))
}

/// Whether one of `names` is an attribute on the definition: on a line of
/// its own above it, or inside it ahead of its name, as C# writes them.
fn has_attribute(source: &str, lines: &Lines, item: &Item, names: &[&str]) -> bool {
    let above = lines_above(source, lines, item.range.start)
        .take_while(|line| is_attribute_line(line))
        .map(str::to_string);
    let inside = source[item.range.start..item.name_range.start]
        .lines()
        .map(str::to_string);
    above.chain(inside).any(|line| {
        attributes_in(&line)
            .iter()
            .any(|attribute| names.contains(&attribute.as_str()))
    })
}

/// The attribute names on a line: `#[tokio::test(flavor = "x")]` names
/// `test`, `[Fact, Trait("a", "b")]` names `Fact` and `Trait`.
fn attributes_in(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let inner = &rest[open + 1..open + close];
        for item in split_outside_parentheses(inner) {
            let path = item.split('(').next().unwrap_or_default().trim();
            let name = path.rsplit([':', '.']).next().unwrap_or_default().trim();
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
        rest = &rest[open + close + 1..];
    }
    names
}

/// Whether one of `names` is an `@Name` annotation on the definition: on a
/// line of its own above it, or inside it ahead of its name, as Java,
/// Kotlin and Swift write them.
fn has_annotation(source: &str, lines: &Lines, item: &Item, names: &[&str]) -> bool {
    let above = lines_above(source, lines, item.range.start)
        .take_while(|line| is_annotation_line(line))
        .map(str::to_string);
    let inside = source[item.range.start..item.name_range.start]
        .lines()
        .map(str::to_string);
    above.chain(inside).any(|line| {
        annotations_in(&line)
            .iter()
            .any(|annotation| names.contains(&annotation.as_str()))
    })
}

/// An `@Name` annotation on a line of its own.
fn is_annotation_line(line: &str) -> bool {
    line.trim_start().starts_with('@')
}

/// The annotation names on a line: `@Test` names `Test`, `@Test(timeout=1)`
/// names `Test`.
fn annotations_in(line: &str) -> Vec<String> {
    line.split('@')
        .skip(1)
        .filter_map(|rest| {
            let name = rest.split(['(', ' ', '\t']).next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// `text` split on the commas outside brackets: the items of
/// `Fact, Trait("a", "b")` are two, and so are the items of
/// `a, b::{c, d}`.
fn split_outside_parentheses(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (at, ch) in text.char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    items.push(&text[start..]);
    items
}

/// The type an `impl` block is for: `impl<T> Trait for Type<T> where …`
/// is `Type`, `impl Type` is `Type`.
fn impl_subject(text: &str) -> String {
    let header = text.split('{').next().unwrap_or(text);
    let header = header.trim_start().strip_prefix("impl").unwrap_or(header);
    let header = header.split(" where ").next().unwrap_or(header);
    let subject = match header.rsplit_once(" for ") {
        Some((_, subject)) => subject,
        None => strip_generics_prefix(header),
    };
    subject
        .trim()
        .split(['<', ' ', '('])
        .next()
        .unwrap_or_default()
        .trim_start_matches('&')
        .to_string()
}

/// The trait an `impl` block is for, by its last path segment:
/// `impl<T> a::Trait<T> for Type` is `Trait`. `None` for an inherent
/// `impl Type`.
fn impl_trait(text: &str) -> Option<String> {
    let header = text.split('{').next().unwrap_or(text);
    let header = header.trim_start().strip_prefix("impl")?;
    let header = header.split(" where ").next().unwrap_or(header);
    let (before, _) = header.rsplit_once(" for ")?;
    let path = strip_generics_prefix(before).trim();
    let name = path
        .split('<')
        .next()
        .unwrap_or(path)
        .rsplit("::")
        .next()
        .unwrap_or_default()
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `<T: Bound> Type` without its leading generics.
fn strip_generics_prefix(header: &str) -> &str {
    let header = header.trim_start();
    if !header.starts_with('<') {
        return header;
    }
    let mut depth = 0;
    for (at, ch) in header.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return &header[at + 1..];
                }
            }
            _ => {}
        }
    }
    header
}

/// Whether the name at byte `at` is called on a receiver: `pattern.test(s)`,
/// `a?.test(s)`, `Mod.test "x"`. A test call names no receiver.
fn has_receiver(source: &str, at: usize) -> bool {
    source[..at].trim_end().ends_with('.')
}

/// The first string literal in a call's text: the name of an `it(…)`.
fn first_string_argument(text: &str) -> Option<String> {
    let open = text.find(['\'', '"', '`'])?;
    let quote = text[open..].chars().next()?;
    let inner = &text[open + quote.len_utf8()..];
    let close = inner.find(quote)?;
    let name = inner[..close].trim();
    (!name.is_empty()).then(|| name.to_string())
}

// -- imports -----------------------------------------------------------------

/// The import statements of a file, read by the syntax of its language:
/// `use` in Rust, `import` and `from … import` in Python, `import` and
/// `require` in TypeScript and JavaScript, `using` in C#. Every other
/// language names none, and its references resolve on the three steps that
/// are left.
///
/// Read off the text rather than off the tree: no tags query captures an
/// import, and the shapes are few enough to name.
fn imports_of(language: Language, source: &str) -> Vec<Import> {
    let lines = Lines::of(source);
    let mut imports = Vec::new();
    match language {
        Language::Rust => {
            for (at, statement) in statements(source, &["use "], ';', "//") {
                for path in expand_braces(statement, "::") {
                    push_import(&mut imports, &path, "::", lines.line_of(at));
                }
            }
        }
        Language::CSharp => {
            for (at, statement) in statements(source, &["using "], ';', "//") {
                let statement = statement.trim();
                let statement = statement.strip_prefix("static ").unwrap_or(statement);
                // `using var file = File.Open(…)` is a statement, not a
                // directive: a directive is a dotted path and nothing else.
                if statement.is_empty()
                    || !statement
                        .chars()
                        .all(|c| c.is_alphanumeric() || "._ =".contains(c))
                {
                    continue;
                }
                // `using Alias = A.B;` imports `A.B`, whatever it is called
                // here. A directive names a namespace, never one definition
                // in it, so it brings in no name of its own.
                let module = statement.rsplit('=').next().unwrap_or(statement).trim();
                imports.push(Import {
                    module: module.to_string(),
                    name: None,
                    line: lines.line_of(at),
                });
            }
        }
        Language::Python => {
            for (at, statement) in statements(source, &["import ", "from "], '\n', "#") {
                python_import(&mut imports, statement, lines.line_of(at));
            }
        }
        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
            for (at, statement) in statements(source, &["import "], '\n', "//") {
                javascript_import(&mut imports, statement, lines.line_of(at));
            }
            // `require` is a call, not a statement: it sits wherever the
            // binding that holds its answer does.
            for (at, args) in call_arguments(source, "require(") {
                if let Some(module) = quoted(args) {
                    imports.push(Import {
                        module,
                        name: None,
                        line: lines.line_of(at),
                    });
                }
            }
        }
        Language::Dart => {
            for (at, statement) in statements(source, &["import ", "export "], ';', "//") {
                dart_import(&mut imports, statement, lines.line_of(at));
            }
        }
        _ => {}
    }
    imports
}

fn call_arguments<'a>(source: &'a str, pattern: &str) -> Vec<(usize, &'a str)> {
    let calls: Vec<(usize, usize)> = source
        .match_indices(pattern)
        .map(|(at, _)| (at, at + pattern.len() - 1))
        .collect();
    let mut ends = vec![None; calls.len()];
    let mut next_call = 0;
    let mut stack = Vec::new();
    for (at, ch) in source.char_indices() {
        match ch {
            '(' | '{' | '[' => {
                let call = calls
                    .get(next_call)
                    .filter(|(_, open)| *open == at)
                    .map(|_| next_call);
                if call.is_some() {
                    next_call += 1;
                }
                stack.push(call);
            }
            ')' | '}' | ']' => {
                if let Some(Some(call)) = stack.pop()
                    && ch == ')'
                {
                    ends[call] = Some(at);
                }
            }
            _ => {}
        }
    }
    calls
        .iter()
        .enumerate()
        .map(|(index, &(at, open))| {
            let start = open + 1;
            let stop = ends[index]
                .or_else(|| calls.get(index + 1).map(|(next, _)| *next))
                .unwrap_or(source.len());
            (at, &source[start..stop])
        })
        .collect()
}

/// Every statement of `source` that opens with one of `keywords` at the
/// start of a line, each with the byte it opens at and its text past the
/// keyword, up to `end` — or up to the closing bracket where one is open.
/// A bracket after the language's `comment` marker opens nothing.
fn statements<'a>(
    source: &'a str,
    keywords: &[&str],
    end: char,
    comment: &str,
) -> Vec<(usize, &'a str)> {
    let lines = Lines::of(source);
    let mut starts = Vec::new();
    for keyword in keywords {
        for (at, _) in source.match_indices(keyword) {
            let line_start = lines.line_start(at);
            // Only a statement of its own, past the visibility it may carry.
            let before = source[line_start..at].trim();
            if !is_visibility(before) {
                continue;
            }
            starts.push((at, keyword.len()));
        }
    }
    // Document order, whatever order the keywords were given in.
    starts.sort_by_key(|(at, _)| *at);
    let mut found = Vec::with_capacity(starts.len());
    for (index, &(at, keyword_len)) in starts.iter().enumerate() {
        let start = at + keyword_len;
        let stop = starts
            .get(index + 1)
            .map_or(source.len(), |(next, _)| *next);
        let rest = &source[start..stop];
        found.push((at, &rest[..import_statement_end(rest, end, comment)]));
    }
    found
}

/// Whether the text before an import keyword is no more than a visibility:
/// nothing, `pub`, `pub(crate)`, `pub(in a::b)` or `export`.
fn is_visibility(before: &str) -> bool {
    before.is_empty()
        || before == "pub"
        || before == "export"
        || (before.starts_with("pub(") && before.ends_with(')') && before.matches('(').count() == 1)
}

/// Where a statement ends: at the first `end` outside a bracket, so a
/// `from a import (\n b,\n)` and a `use a::{\n b,\n};` are read whole.
pub(crate) fn statement_end(rest: &str, end: char) -> usize {
    let mut scan_end = rest.len().min(STATEMENT_SCAN_MAX);
    while !rest.is_char_boundary(scan_end) {
        scan_end -= 1;
    }
    balanced_statement_end(&rest[..scan_end], end)
}

/// Where an import statement ends: at the first `end` outside a bracket, a
/// string and a line comment, so a bracket in `# noqa (legacy` opens
/// nothing.
fn import_statement_end(rest: &str, end: char, comment: &str) -> usize {
    let mut depth = 0i32;
    let mut quote = None;
    let mut in_comment = false;
    for (at, ch) in rest.char_indices() {
        if in_comment && ch != '\n' {
            continue;
        }
        in_comment = false;
        if let Some(open) = quote {
            if ch == open || ch == '\n' {
                quote = None;
            }
            if ch != '\n' {
                continue;
            }
        }
        if ch == end && depth <= 0 {
            return at;
        }
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            '\'' | '"' | '`' => quote = Some(ch),
            _ if rest[at..].starts_with(comment) => in_comment = true,
            _ => {}
        }
    }
    rest.len()
}

fn balanced_statement_end(rest: &str, end: char) -> usize {
    let mut depth = 0i32;
    for (at, ch) in rest.char_indices() {
        if ch == end && depth <= 0 {
            return at;
        }
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    rest.len()
}

/// `a::b::{c, d::e}` as `a::b::c` and `a::b::d::e`.
fn expand_braces(path: &str, separator: &str) -> Vec<String> {
    expand_braces_at(path, separator, 0)
}

fn expand_braces_at(path: &str, separator: &str, depth: usize) -> Vec<String> {
    let path = path.trim();
    if depth >= NESTING_DEPTH_MAX {
        return vec![path.to_string()];
    }
    let (Some(open), Some(close)) = (path.find('{'), path.rfind('}')) else {
        return vec![path.to_string()];
    };
    if close < open {
        return vec![path.to_string()];
    }
    let prefix = path[..open].trim().trim_end_matches(separator);
    let mut paths = Vec::new();
    for item in split_outside_parentheses(&path[open + 1..close]) {
        for tail in expand_braces_at(item, separator, depth + 1) {
            if tail.is_empty() {
                continue;
            }
            paths.push(match prefix.is_empty() {
                true => tail,
                false => format!("{prefix}{separator}{tail}"),
            });
        }
    }
    paths
}

/// Record one imported path: everything but its last segment is the module,
/// and the last segment is the name — unless it is a glob, which names
/// nothing.
///
/// `use a::b as c` keeps `b`: the alias is not what the definition is
/// called, and the definition is what the edge points at.
fn push_import(imports: &mut Vec<Import>, path: &str, separator: &str, line: u32) {
    let path = path.trim();
    let path = path.split(" as ").next().unwrap_or(path).trim();
    if path.is_empty() {
        return;
    }
    let (module, name) = match path.rsplit_once(separator) {
        Some((module, last)) => (module.trim().to_string(), last.trim()),
        None => (String::new(), path),
    };
    let named = !name.is_empty() && name != "*" && name != "self";
    imports.push(Import {
        module: match named {
            true => module,
            // `use a::b::*` and `using System.Text` name a whole module.
            false => match module.is_empty() {
                true => path.to_string(),
                false => format!("{module}{separator}{name}"),
            },
        },
        name: named.then(|| name.to_string()),
        line,
    });
}

/// `import a.b`, `import a.b as c`, `from a.b import c, d as e`, and
/// `from . import x`.
fn python_import(imports: &mut Vec<Import>, statement: &str, line: u32) {
    // A `# comment` names nothing, on any line of the statement.
    let statement = statement
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    match statement.split_once(" import ") {
        // `from a.b import c, d`: one module, every name in it.
        Some((module, names)) => {
            let module = module.trim().to_string();
            for name in split_outside_parentheses(names.trim().trim_matches(['(', ')'])) {
                let name = name.trim();
                let name = name.split(" as ").next().unwrap_or(name).trim();
                if name.is_empty() || name == "*" {
                    continue;
                }
                imports.push(Import {
                    module: module.clone(),
                    name: Some(name.to_string()),
                    line,
                });
            }
        }
        // `import a.b, c`: whole modules, naming nothing in them.
        None => {
            for module in split_outside_parentheses(&statement) {
                let module = module.trim();
                let module = module.split(" as ").next().unwrap_or(module).trim();
                if !module.is_empty() {
                    imports.push(Import {
                        module: module.to_string(),
                        name: None,
                        line,
                    });
                }
            }
        }
    }
}

/// `import { x, y as z } from './m'`, `import d from 'm'`,
/// `import * as ns from 'm'` and `import 'm'`.
fn javascript_import(imports: &mut Vec<Import>, statement: &str, line: u32) {
    let Some(module) = quoted(statement) else {
        return;
    };
    let clause = match statement.rsplit_once(" from ") {
        Some((clause, _)) => clause.trim(),
        // `import './m'`: a module and no names.
        None => "",
    };
    let clause = clause.strip_prefix("type ").unwrap_or(clause);
    // The braces of `d, { x, y }` only group; what matters is the names.
    let clause = clause.replace(['{', '}'], " ");
    let mut names: Vec<String> = Vec::new();
    for part in clause.split(',') {
        let part = part.trim();
        let part = part.strip_prefix("type ").unwrap_or(part);
        // `* as ns` names the module, not a definition in it.
        if part.is_empty() || part.starts_with('*') {
            continue;
        }
        let name = part.split(" as ").next().unwrap_or(part).trim();
        if !name.is_empty() {
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        imports.push(Import {
            module,
            name: None,
            line,
        });
        return;
    }
    for name in names {
        imports.push(Import {
            module: module.clone(),
            name: Some(name),
            line,
        });
    }
}

/// `import 'package:app/m.dart' show x, y`, `export './m.dart'`, and their
/// `as` and `hide` variants. A prefix is local to the importing file and a
/// hidden name is unavailable, so neither names a definition here.
fn dart_import(imports: &mut Vec<Import>, statement: &str, line: u32) {
    let Some(module) = quoted(statement) else {
        return;
    };
    // The Dart SDK is outside the repository.
    if module.starts_with("dart:") {
        return;
    }
    let Some((_, shown)) = statement.split_once(" show ") else {
        imports.push(Import {
            module,
            name: None,
            line,
        });
        return;
    };
    let shown = shown.split_once(" hide ").map_or(shown, |(shown, _)| shown);
    for name in shown
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        imports.push(Import {
            module: module.clone(),
            name: Some(name.to_string()),
            line,
        });
    }
}

/// The first quoted string of a statement: the module a JavaScript import
/// reads from.
fn quoted(text: &str) -> Option<String> {
    let open = text.find(['\'', '"', '`'])?;
    let quote = text[open..].chars().next()?;
    let inner = &text[open + quote.len_utf8()..];
    let close = inner.find(quote)?;
    Some(inner[..close].to_string())
}

/// The headings of a Markdown file, each spanning to the next heading of
/// its level or above.
fn markdown_outline(source: &str) -> Vec<Symbol> {
    let grammar = Language::Markdown.grammar();
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&grammar).is_err() {
        return Vec::new();
    }
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let Ok(query) = tree_sitter::Query::new(&grammar, "[(atx_heading) (setext_heading)] @heading")
    else {
        return Vec::new();
    };
    struct Heading {
        level: usize,
        name: String,
        signature: String,
        start_line: u32,
    }
    let mut headings: Vec<Heading> = Vec::new();
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            let node = capture.node;
            let mut level = 0;
            let mut walk = node.walk();
            for child in node.children(&mut walk) {
                level = match child.kind() {
                    "atx_h1_marker" | "setext_h1_underline" => 1,
                    "atx_h2_marker" | "setext_h2_underline" => 2,
                    "atx_h3_marker" => 3,
                    "atx_h4_marker" => 4,
                    "atx_h5_marker" => 5,
                    "atx_h6_marker" => 6,
                    _ => continue,
                };
            }
            let text = node_text(source, node);
            let name = node
                .child_by_field_name("heading_content")
                .map(|content| node_text(source, content).to_string())
                .unwrap_or_else(|| text.lines().next().unwrap_or_default().to_string());
            let name = name
                .trim()
                .trim_end_matches('#')
                .trim()
                .trim_start_matches('#')
                .trim()
                .to_string();
            if name.is_empty() || level == 0 {
                continue;
            }
            headings.push(Heading {
                level,
                name,
                signature: cut(
                    text.lines().next().unwrap_or_default().trim(),
                    SIGNATURE_MAX,
                ),
                start_line: node.start_position().row as u32 + 1,
            });
        }
    }
    headings.sort_by_key(|heading| heading.start_line);
    let last_line = source.lines().count().max(1) as u32;
    let mut scopes: Vec<(usize, String)> = Vec::new();
    let mut symbols = Vec::new();
    for (at, heading) in headings.iter().enumerate() {
        let end_line = headings[at + 1..]
            .iter()
            .find(|next| next.level <= heading.level)
            .map_or(last_line, |next| next.start_line - 1);
        while scopes
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            scopes.pop();
        }
        let qualified_name = match scopes.last() {
            Some((_, parent)) => {
                format!("{parent}{}{}", Language::Markdown.separator(), heading.name)
            }
            None => heading.name.clone(),
        };
        scopes.push((heading.level, qualified_name.clone()));
        symbols.push(Symbol {
            kind: "heading".into(),
            name: heading.name.clone(),
            qualified_name,
            start_line: heading.start_line,
            end_line: end_line.max(heading.start_line),
            signature: heading.signature.clone(),
            doc: None,
            is_test: false,
        });
    }
    symbols
}

/// `text`'s first line, cut to `SIGNATURE_MAX`: what an outline-only
/// format's symbol shows in place of a code signature.
fn first_line_signature(text: &str) -> String {
    cut(
        text.lines().next().unwrap_or_default().trim(),
        SIGNATURE_MAX,
    )
}

fn node_text<'a>(source: &'a str, node: tree_sitter::Node) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}

/// A parser for `language`, or an empty outline where the grammar refuses
/// to load.
fn outline_parser(language: Language) -> Option<tree_sitter::Parser> {
    let grammar = language.grammar();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&grammar).ok()?;
    Some(parser)
}

fn outline_tree(parser: &mut tree_sitter::Parser, source: &str) -> Option<tree_sitter::Tree> {
    let started = std::time::Instant::now();
    let mut progress = |_: &tree_sitter::ParseState| match started.elapsed() < PARSE_MAX {
        true => std::ops::ControlFlow::Continue(()),
        false => std::ops::ControlFlow::Break(()),
    };
    parser.parse_with_options(
        &mut |at, _| source.as_bytes().get(at..).unwrap_or_default(),
        None,
        Some(tree_sitter::ParseOptions::new().progress_callback(&mut progress)),
    )
}

/// The top-level keys of a JSON object, and one level of keys nested under
/// an object value: JSON has no comments to read for a doc.
fn json_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Json) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let Some(object) = tree
        .root_node()
        .named_child(0)
        .filter(|node| node.kind() == "object")
    else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    json_pairs(object, None, 1, source, &lines, &mut symbols);
    symbols
}

fn json_pairs(
    object: tree_sitter::Node,
    parent: Option<&str>,
    depth: u32,
    source: &str,
    lines: &Lines,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = object.walk();
    for pair in object.named_children(&mut cursor) {
        if pair.kind() != "pair" {
            continue;
        }
        let Some(key) = pair.child_by_field_name("key") else {
            continue;
        };
        let name = key
            .named_child(0)
            .map(|content| node_text(source, content).to_string())
            .unwrap_or_default();
        let qualified_name = match parent {
            Some(parent) => format!("{parent}.{name}"),
            None => name.clone(),
        };
        let (start_line, end_line) = lines.of_range(&pair.byte_range());
        symbols.push(Symbol {
            kind: "key".into(),
            name,
            qualified_name: qualified_name.clone(),
            start_line,
            end_line,
            signature: first_line_signature(node_text(source, pair)),
            doc: None,
            is_test: false,
        });
        if depth < 2
            && let Some(value) = pair.child_by_field_name("value")
            && value.kind() == "object"
        {
            json_pairs(
                value,
                Some(&qualified_name),
                depth + 1,
                source,
                lines,
                symbols,
            );
        }
    }
}

/// The top-level keys of a YAML mapping, and one level of keys nested under
/// a mapping value.
fn yaml_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Yaml) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let Some(mapping) = find_block_mapping(tree.root_node()) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    yaml_pairs(mapping, None, 1, source, &lines, &mut symbols);
    symbols
}

/// The first `block_mapping` at or under `node`.
fn find_block_mapping(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    find_block_mapping_at(node, 0)
}

fn find_block_mapping_at(node: tree_sitter::Node, depth: usize) -> Option<tree_sitter::Node> {
    if node.kind() == "block_mapping" {
        return Some(node);
    }
    if depth >= NESTING_DEPTH_MAX {
        return None;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_block_mapping_at(child, depth + 1))
}

fn yaml_pairs(
    mapping: tree_sitter::Node,
    parent: Option<&str>,
    depth: u32,
    source: &str,
    lines: &Lines,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = mapping.walk();
    for pair in mapping.named_children(&mut cursor) {
        if pair.kind() != "block_mapping_pair" {
            continue;
        }
        let Some(key) = pair.child_by_field_name("key") else {
            continue;
        };
        let name = node_text(source, key).trim().to_string();
        let qualified_name = match parent {
            Some(parent) => format!("{parent}.{name}"),
            None => name.clone(),
        };
        let (start_line, end_line) = lines.of_range(&pair.byte_range());
        symbols.push(Symbol {
            kind: "key".into(),
            name,
            qualified_name: qualified_name.clone(),
            start_line,
            end_line,
            signature: first_line_signature(node_text(source, pair)),
            doc: None,
            is_test: false,
        });
        if depth < 2
            && let Some(value) = pair.child_by_field_name("value")
            && let Some(nested) = find_block_mapping(value)
            && !in_sequence(nested, value)
        {
            yaml_pairs(
                nested,
                Some(&qualified_name),
                depth + 1,
                source,
                lines,
                symbols,
            );
        }
    }
}

/// Whether `node` sits in a list under `value`: the keys of a list's first
/// item are no keys of the list.
fn in_sequence(node: tree_sitter::Node, value: tree_sitter::Node) -> bool {
    let mut at = node.parent();
    while let Some(parent) = at
        && parent != value
    {
        if matches!(parent.kind(), "block_sequence" | "flow_sequence") {
            return true;
        }
        at = parent.parent();
    }
    false
}

/// The top-level keys of a TOML document, and the keys of each `[table]`.
fn toml_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Toml) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    let mut cursor = tree.root_node().walk();
    for node in tree.root_node().named_children(&mut cursor) {
        match node.kind() {
            "pair" => symbols.extend(toml_pair(node, None, source, &lines)),
            "table" => {
                let Some(key) = node.named_child(0) else {
                    continue;
                };
                let name = toml_key_text(key, source);
                let (start_line, end_line) = lines.of_range(&node.byte_range());
                symbols.push(Symbol {
                    kind: "table".into(),
                    name: name.clone(),
                    qualified_name: name.clone(),
                    start_line,
                    end_line,
                    signature: first_line_signature(node_text(source, node)),
                    doc: None,
                    is_test: false,
                });
                let mut pairs = node.walk();
                for pair in node.named_children(&mut pairs) {
                    if pair.kind() == "pair" {
                        symbols.extend(toml_pair(pair, Some(&name), source, &lines));
                    }
                }
            }
            _ => {}
        }
    }
    symbols
}

fn toml_pair(
    pair: tree_sitter::Node,
    parent: Option<&str>,
    source: &str,
    lines: &Lines,
) -> Option<Symbol> {
    let key = pair.named_child(0)?;
    let name = toml_key_text(key, source);
    let qualified_name = match parent {
        Some(parent) => format!("{parent}.{name}"),
        None => name.clone(),
    };
    let (start_line, end_line) = lines.of_range(&pair.byte_range());
    Some(Symbol {
        kind: "key".into(),
        name,
        qualified_name,
        start_line,
        end_line,
        signature: first_line_signature(node_text(source, pair)),
        doc: None,
        is_test: false,
    })
}

/// A TOML key's text, its quotes stripped where it is a quoted key.
fn toml_key_text(node: tree_sitter::Node, source: &str) -> String {
    node_text(source, node)
        .trim_matches(['"', '\''])
        .to_string()
}

/// The elements of an HTML file that carry an `id`, named by its value.
fn html_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Html) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    html_elements(tree.root_node(), source, &lines, &mut symbols);
    symbols
}

/// Every element at or under `node` that carries an `id`, depth-first.
fn html_elements(node: tree_sitter::Node, source: &str, lines: &Lines, symbols: &mut Vec<Symbol>) {
    html_elements_at(node, source, lines, symbols, 0);
}

fn html_elements_at(
    node: tree_sitter::Node,
    source: &str,
    lines: &Lines,
    symbols: &mut Vec<Symbol>,
    depth: usize,
) {
    if depth >= NESTING_DEPTH_MAX {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "element"
            && let Some(start_tag) = child
                .named_child(0)
                .filter(|tag| matches!(tag.kind(), "start_tag" | "self_closing_tag"))
            && let Some(id) = html_id_attribute(start_tag, source)
        {
            let (start_line, end_line) = lines.of_range(&child.byte_range());
            symbols.push(Symbol {
                kind: "element".into(),
                name: id.clone(),
                qualified_name: id,
                start_line,
                end_line,
                signature: first_line_signature(node_text(source, start_tag)),
                doc: None,
                is_test: false,
            });
        }
        html_elements_at(child, source, lines, symbols, depth + 1);
    }
}

/// The value of a `start_tag` or `self_closing_tag`'s `id` attribute, if it
/// has one.
fn html_id_attribute(start_tag: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = start_tag.walk();
    for attribute in start_tag.named_children(&mut cursor) {
        if attribute.kind() != "attribute" {
            continue;
        }
        let mut parts = attribute.walk();
        let mut is_id = false;
        let mut value = None;
        for part in attribute.named_children(&mut parts) {
            match part.kind() {
                "attribute_name" => is_id = node_text(source, part) == "id",
                "quoted_attribute_value" => {
                    value = part
                        .named_child(0)
                        .map(|content| node_text(source, content).to_string())
                }
                _ => {}
            }
        }
        if is_id {
            return value;
        }
    }
    None
}

/// The selectors of a CSS file's rule sets.
fn css_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Css) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let grammar = Language::Css.grammar();
    let Ok(query) = tree_sitter::Query::new(&grammar, "(rule_set (selectors) @selectors) @rule")
    else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(matched) = matches.next() {
        let mut rule = None;
        let mut selectors = None;
        for capture in matched.captures() {
            match query.capture_names()[capture.index as usize] {
                "rule" => rule = Some(capture.node),
                "selectors" => selectors = Some(capture.node),
                _ => {}
            }
        }
        let (Some(rule), Some(selectors)) = (rule, selectors) else {
            continue;
        };
        let name = node_text(source, selectors)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let (start_line, end_line) = lines.of_range(&rule.byte_range());
        symbols.push(Symbol {
            kind: "selector".into(),
            name: name.clone(),
            qualified_name: name.clone(),
            start_line,
            end_line,
            signature: cut(&name, SIGNATURE_MAX),
            doc: None,
            is_test: false,
        });
    }
    symbols
}

/// The object name of every `CREATE` and `ALTER` statement in a SQL file:
/// a table, a view, an index, a function, a sequence, a type, a trigger, a
/// materialized view or a schema.
fn sql_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Sql) else {
        return Vec::new();
    };
    let Some(tree) = outline_tree(&mut parser, source) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    let mut cursor = tree.root_node().walk();
    for statement in tree.root_node().named_children(&mut cursor) {
        let Some(node) = statement.named_child(0) else {
            continue;
        };
        let kind = node.kind();
        let Some(kind) = kind
            .strip_prefix("create_")
            .or_else(|| kind.strip_prefix("alter_"))
        else {
            continue;
        };
        let Some(name) = sql_object_name(node, source) else {
            continue;
        };
        let (start_line, end_line) = lines.of_range(&statement.byte_range());
        symbols.push(Symbol {
            kind: kind.to_string(),
            name: name.clone(),
            qualified_name: name,
            start_line,
            end_line,
            signature: first_line_signature(node_text(source, statement)),
            doc: None,
            is_test: false,
        });
    }
    symbols
}

/// The name of the object a `create_*`/`alter_*` node names: its first
/// `object_reference` or bare `identifier` child, before any table, column
/// or target the rest of the statement goes on to name.
fn sql_object_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| match child.kind() {
            "object_reference" => {
                let name = child.child_by_field_name("name")?;
                Some(node_text(source, name).to_string())
            }
            "identifier" => Some(node_text(source, child).to_string()),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("no symbol {name} in {symbols:#?}"))
    }

    /// A Rust method is qualified by the type its `impl` block is for, not
    /// by the trait, and its doc comment is the `///` lines above it, past
    /// the attributes between them.
    #[test]
    fn a_rust_method_is_qualified_by_its_impl_type_and_carries_its_doc() {
        let source = "\
/// A manager.
pub struct GitManager;

impl std::fmt::Display for GitManager {
    /// Writes it.
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        Ok(())
    }
}

/// Free.
pub async fn add_worktree(
    repo: &Path,
    branch: &str,
) -> Result<()> {
    todo!()
}
";
        let symbols = parse(Language::Rust, source);
        let fmt = find(&symbols, "fmt");
        assert_eq!(fmt.qualified_name, "GitManager::fmt");
        assert_eq!(fmt.kind, "method");
        assert_eq!(fmt.doc.as_deref(), Some("Writes it."));
        assert_eq!((fmt.start_line, fmt.end_line), (7, 9));
        let free = find(&symbols, "add_worktree");
        assert_eq!(free.qualified_name, "add_worktree");
        assert_eq!(
            free.signature,
            "pub async fn add_worktree( repo: &Path, branch: &str, ) -> Result<()>"
        );
        assert_eq!(free.doc.as_deref(), Some("Free."));
        assert_eq!((free.start_line, free.end_line), (13, 18));
        assert_eq!(
            find(&symbols, "GitManager").doc.as_deref(),
            Some("A manager.")
        );
    }

    #[test]
    fn a_test_is_marked_by_the_rule_of_its_language() {
        let rust = parse(
            Language::Rust,
            "#[test]\nfn holds() {}\n#[tokio::test(flavor = \"x\")]\nasync fn waits() {}\nfn plain() {}\n",
        );
        assert!(find(&rust, "holds").is_test);
        assert!(find(&rust, "waits").is_test);
        assert!(!find(&rust, "plain").is_test);

        let python = parse(
            Language::Python,
            "def test_it():\n    pass\ndef it():\n    pass\n",
        );
        assert!(find(&python, "test_it").is_test);
        assert!(!find(&python, "it").is_test);

        let typescript = parse(
            Language::TypeScript,
            "it('adds numbers', () => {});\ntest(\"subtracts\", () => {});\nfunction helper() {}\n",
        );
        let adds = find(&typescript, "adds numbers");
        assert!(adds.is_test);
        assert_eq!(adds.kind, "test");
        assert!(find(&typescript, "subtracts").is_test);
        assert!(!find(&typescript, "helper").is_test);

        let csharp = parse(
            Language::CSharp,
            "public class T {\n    [Fact]\n    public void Holds() {}\n    [Test, Category(\"slow\")]\n    public void Waits() {}\n    public void Plain() {}\n}\n",
        );
        assert!(find(&csharp, "Holds").is_test);
        assert!(find(&csharp, "Waits").is_test);
        assert!(!find(&csharp, "Plain").is_test);
    }

    #[test]
    fn a_test_is_marked_by_the_rule_of_the_new_languages() {
        let go = parse(Language::Go, "func TestHolds(t *T) {}\nfunc Plain() {}\n");
        assert!(find(&go, "TestHolds").is_test);
        assert!(!find(&go, "Plain").is_test);

        let java = parse(
            Language::Java,
            "class T {\n    @Test\n    void holds() {}\n    void plain() {}\n}\n",
        );
        assert!(find(&java, "holds").is_test);
        assert!(!find(&java, "plain").is_test);

        let kotlin = parse(
            Language::Kotlin,
            "class T {\n    @Test\n    fun holds() {}\n    fun plain() {}\n}\n",
        );
        assert!(find(&kotlin, "holds").is_test);
        assert!(!find(&kotlin, "plain").is_test);

        let ruby = parse(
            Language::Ruby,
            "describe 'a thing' do\n  it 'holds' do\n  end\nend\ndef plain\nend\n",
        );
        assert!(find(&ruby, "holds").is_test);
        assert!(!find(&ruby, "plain").is_test);

        let php = parse(
            Language::Php,
            "<?php\nclass T {\n    public function testHolds() {}\n    public function plain() {}\n}\n",
        );
        assert!(find(&php, "testHolds").is_test);
        assert!(!find(&php, "plain").is_test);

        let swift = parse(
            Language::Swift,
            "class T {\n    @Test\n    func attributed() {}\n    func testNamed() {}\n    func plain() {}\n}\n",
        );
        assert!(find(&swift, "attributed").is_test);
        assert!(find(&swift, "testNamed").is_test);
        assert!(!find(&swift, "plain").is_test);

        let dart = parse(
            Language::Dart,
            "void main() {\n  test('holds', () {});\n}\nvoid plain() {}\n",
        );
        assert!(find(&dart, "holds").is_test);
        assert!(!find(&dart, "plain").is_test);

        let scala = parse(
            Language::Scala,
            "class T {\n  test(\"holds\") {}\n  def plain() = {}\n}\n",
        );
        assert!(find(&scala, "holds").is_test);
        assert!(!find(&scala, "plain").is_test);

        let elixir = parse(
            Language::Elixir,
            "defmodule T do\n  test \"holds\" do\n  end\n  def plain do\n  end\nend\n",
        );
        assert!(find(&elixir, "holds").is_test);
        assert!(!find(&elixir, "plain").is_test);

        let bash = parse(
            Language::Bash,
            "@test \"holds\" {\n  true\n}\nplain() {\n  true\n}\n",
        );
        assert!(find(&bash, "holds").is_test);
        assert!(!find(&bash, "plain").is_test);
    }

    #[test]
    fn a_python_docstring_and_a_javascript_block_comment_are_docs() {
        let python = parse(
            Language::Python,
            "class Parser:\n    \"\"\"Reads files.\n\n    Slowly.\n    \"\"\"\n    def read(self, path):\n        '''One file.'''\n        return path\n",
        );
        assert_eq!(
            find(&python, "Parser").doc.as_deref(),
            Some("Reads files.\n\nSlowly.")
        );
        let read = find(&python, "read");
        assert_eq!(read.doc.as_deref(), Some("One file."));
        assert_eq!(read.qualified_name, "Parser.read");
        assert_eq!(read.signature, "def read(self, path)");

        let javascript = parse(
            Language::JavaScript,
            "/**\n * Adds two numbers.\n */\nexport function add(a, b) { return a + b; }\n// Doubles.\nconst twice = (x) => x * 2;\n",
        );
        assert_eq!(
            find(&javascript, "add").doc.as_deref(),
            Some("Adds two numbers.")
        );
        assert_eq!(find(&javascript, "twice").doc.as_deref(), Some("Doubles."));
    }

    #[test]
    fn markdown_headings_span_to_the_next_heading_of_their_level() {
        let source =
            "# Title\n\nText\n\n## Sub\n\nmore\n\n### Deep ###\n\nlast\n\n## Other\n\nend\n";
        let symbols = parse(Language::Markdown, source);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Title", "Sub", "Deep", "Other"]);
        let sub = find(&symbols, "Sub");
        assert_eq!((sub.start_line, sub.end_line), (5, 12));
        assert_eq!(sub.qualified_name, "Title > Sub");
        assert_eq!(sub.signature, "## Sub");
        let deep = find(&symbols, "Deep");
        assert_eq!((deep.start_line, deep.end_line), (9, 12));
        assert_eq!(deep.qualified_name, "Title > Sub > Deep");
        let title = find(&symbols, "Title");
        assert_eq!((title.start_line, title.end_line), (1, 15));
        assert!(symbols.iter().all(|s| s.kind == "heading"));
    }

    #[test]
    fn attributes_are_read_by_their_last_path_segment() {
        assert_eq!(attributes_in("#[tokio::test(flavor = \"x\")]"), ["test"]);
        assert_eq!(
            attributes_in("[Fact, Trait(\"a\", \"b\")]"),
            ["Fact", "Trait"]
        );
        assert_eq!(attributes_in("#[cfg(test)]"), ["cfg"]);
        assert!(attributes_in("fn plain() {}").is_empty());
    }

    /// Every import shape of every language the index reads, as the module
    /// it reads from and the name it brings in.
    #[test]
    fn an_import_is_read_by_the_syntax_of_its_language() {
        let read = |language, source| {
            imports_of(language, source)
                .into_iter()
                .map(|import| (import.module, import.name.unwrap_or_default()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            read(
                Language::Rust,
                "use crate::m::b;\npub use a::{c, d::e};\nuse x::*;\nuse y as z;\n// use n::o;\n",
            ),
            [
                ("crate::m".into(), "b".to_string()),
                ("a".into(), "c".into()),
                ("a::d".into(), "e".into()),
                ("x::*".into(), String::new()),
                ("".into(), "y".into()),
            ]
        );
        assert_eq!(
            read(
                Language::Python,
                "from pkg.m import x, y as z\nimport os.path\nfrom . import q\nfrom a import (\n    b,\n    c,\n)\n",
            ),
            [
                ("pkg.m".into(), "x".to_string()),
                // `y as z` keeps `y`: the alias is not what the definition
                // is called.
                ("pkg.m".into(), "y".into()),
                ("os.path".into(), String::new()),
                (".".into(), "q".into()),
                ("a".into(), "b".into()),
                ("a".into(), "c".into()),
            ]
        );
        assert_eq!(
            read(
                Language::TypeScript,
                "import { x, y as z } from './m';\nimport d from 'lib';\nimport * as ns from 'all';\nimport './side';\n",
            ),
            [
                ("./m".into(), "x".to_string()),
                ("./m".into(), "y".into()),
                ("lib".into(), "d".into()),
                ("all".into(), String::new()),
                ("./side".into(), String::new()),
            ]
        );
        assert_eq!(
            read(
                Language::JavaScript,
                "const { halve } = require('./util');\nconst lib = require('lib');\n",
            ),
            [
                ("./util".into(), String::new()),
                ("lib".into(), String::new()),
            ]
        );
        assert_eq!(
            read(
                Language::CSharp,
                "using System.Text;\nusing static Lib.Helper;\nusing Alias = Lib.Deep;\nusing var file = File.Open(path);\n",
            ),
            [
                ("System.Text".into(), String::new()),
                ("Lib.Helper".into(), String::new()),
                ("Lib.Deep".into(), String::new()),
            ]
        );
    }

    #[test]
    fn a_dart_directive_reads_its_module_names_and_line() {
        assert_eq!(
            imports_of(
                Language::Dart,
                "import 'package:app/src/x.dart';\nimport '../lib/y.dart' as y;\nimport 'package:app/src/shown.dart' show One, Two;\nexport './reexport.dart' hide Hidden;\nexport 'package:app/src/public.dart' show Exported;\nimport 'dart:async' show Future;\n",
            ),
            [
                Import {
                    module: "package:app/src/x.dart".into(),
                    name: None,
                    line: 1,
                },
                Import {
                    module: "../lib/y.dart".into(),
                    name: None,
                    line: 2,
                },
                Import {
                    module: "package:app/src/shown.dart".into(),
                    name: Some("One".into()),
                    line: 3,
                },
                Import {
                    module: "package:app/src/shown.dart".into(),
                    name: Some("Two".into()),
                    line: 3,
                },
                Import {
                    module: "./reexport.dart".into(),
                    name: None,
                    line: 4,
                },
                Import {
                    module: "package:app/src/public.dart".into(),
                    name: Some("Exported".into()),
                    line: 5,
                },
            ]
        );
    }

    /// A call is an edge from the definition it sits in, a Rust
    /// `impl Trait for Type` is one from the type to the trait, and a base
    /// class is an `extends`.
    #[test]
    fn a_reference_belongs_to_the_definition_it_sits_in() {
        let rust = references_of(
            Language::Rust,
            "struct Widget;\n\
             impl std::fmt::Display for Widget {\n    \
                 fn fmt(&self) {\n        \
                     helper();\n    \
                 }\n\
             }\n",
        );
        assert!(
            rust.contains(&(EdgeKind::Calls, "helper".into(), Some("fmt".into()))),
            "{rust:?}"
        );
        assert!(
            rust.contains(&(
                EdgeKind::Implements,
                "Display".into(),
                Some("Widget".into())
            )),
            "{rust:?}"
        );

        for (language, source, base) in [
            (
                Language::JavaScript,
                "class Button extends Widget {}\n",
                "Widget",
            ),
            (
                Language::TypeScript,
                "class Button extends Widget {}\n",
                "Widget",
            ),
            (
                Language::Python,
                "class Button(Widget):\n    pass\n",
                "Widget",
            ),
            (
                Language::CSharp,
                "public class Button : Widget {}\n",
                "Widget",
            ),
        ] {
            let found = references_of(language, source);
            assert!(
                found.contains(&(EdgeKind::Extends, base.into(), Some("Button".into()))),
                "{}: {found:?}",
                language.name()
            );
        }
    }

    /// A type alias is a definition of kind `type` and an enum one of kind
    /// `class`, in TypeScript and in TSX alike.
    #[test]
    fn a_type_alias_and_an_enum_are_definitions_in_typescript_and_tsx() {
        for language in [Language::TypeScript, Language::Tsx] {
            let symbols = parse(
                language,
                "export type A = { x: number }\nexport enum B {\n  One,\n}\n",
            );
            let alias = find(&symbols, "A");
            assert_eq!(
                (
                    alias.kind.as_str(),
                    alias.start_line,
                    alias.end_line,
                    alias.signature.as_str()
                ),
                ("type", 1, 1, "type A ="),
                "{}",
                language.name()
            );
            let enumeration = find(&symbols, "B");
            assert_eq!(
                (
                    enumeration.kind.as_str(),
                    enumeration.start_line,
                    enumeration.end_line,
                    enumeration.signature.as_str()
                ),
                ("class", 2, 4, "enum B"),
                "{}",
                language.name()
            );
        }
    }

    /// Every reference of a source, as `(kind, name, the definition it sits
    /// in)`.
    fn references_of(language: Language, source: &str) -> Vec<(EdgeKind, String, Option<String>)> {
        let parsed = read(language, source);
        parsed
            .references
            .iter()
            .map(|reference| {
                (
                    reference.kind,
                    reference.name.clone(),
                    reference.from.map(|at| parsed.symbols[at].name.clone()),
                )
            })
            .collect()
    }

    /// A method call on a receiver shares its name with a test call, but it
    /// is no test: `/^a/.test(s)` is a regular expression match.
    #[test]
    fn a_method_call_on_a_receiver_is_no_test() {
        for (language, source) in [
            (Language::JavaScript, "if (/^a/.test(s)) {}\n"),
            (Language::TypeScript, "if (/^a/.test(s)) {}\n"),
            (Language::Dart, "void main() {\n  pattern.test('a');\n}\n"),
            (Language::Scala, "class T {\n  pattern.test(\"a\")\n}\n"),
            (
                Language::Elixir,
                "defmodule T do\n  def f do\n    Pattern.test(\"a\")\n  end\nend\n",
            ),
            (Language::Ruby, "def f\n  spec.it 'a'\nend\n"),
        ] {
            let symbols = parse(language, source);
            assert!(
                symbols
                    .iter()
                    .all(|symbol| !symbol.is_test && symbol.kind != "test"),
                "{}: {symbols:#?}",
                language.name()
            );
        }
    }

    /// A Java or PHP `implements` clause is an edge from the class to the
    /// interface, and the methods of the class stay in the class's scope.
    #[test]
    fn an_implements_clause_is_an_edge_from_its_class() {
        for (language, source) in [
            (
                Language::Java,
                "class A implements Runnable, Closeable {\n    void run() {}\n}\n",
            ),
            (
                Language::Php,
                "<?php\nclass A implements Runnable, Closeable {\n    public function run() {}\n}\n",
            ),
        ] {
            let found = references_of(language, source);
            for interface in ["Runnable", "Closeable"] {
                assert!(
                    found.contains(&(EdgeKind::Implements, interface.into(), Some("A".into()))),
                    "{}: {found:?}",
                    language.name()
                );
            }
            let symbols = parse(language, source);
            assert_eq!(
                find(&symbols, "run").qualified_name,
                "A.run",
                "{}",
                language.name()
            );
        }
    }

    /// A block comment that closes at the end of a line of code is no doc of
    /// the definition below it, and the walk up never reaches another
    /// comment.
    #[test]
    fn a_trailing_block_comment_is_no_doc() {
        let symbols = parse(
            Language::C,
            "/* Licence header. */\n#include <x.h>\nstatic int n; /* count */\nint f(void) { return n; }\n/* Adds. */\nint g(void) { return 1; }\nstatic int m; /* a\n   b */\nint h(void) { return m; }\n",
        );
        assert_eq!(find(&symbols, "f").doc, None);
        assert_eq!(find(&symbols, "g").doc.as_deref(), Some("Adds."));
        assert_eq!(find(&symbols, "h").doc, None);
    }

    #[test]
    fn a_python_docstring_is_read_with_windows_line_endings() {
        let symbols = parse(
            Language::Python,
            "def f():\r\n    \"\"\"Adds.\"\"\"\r\n    return 1\r\n",
        );
        assert_eq!(find(&symbols, "f").doc.as_deref(), Some("Adds."));
    }

    /// A keyword inside a string on a line that starts with `pub` or
    /// `export` opens no import; a visibility before the keyword does.
    #[test]
    fn an_import_keyword_after_other_code_opens_no_import() {
        let read = |language, source| {
            imports_of(language, source)
                .into_iter()
                .map(|import| (import.module, import.name.unwrap_or_default()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            read(
                Language::Rust,
                "pub const H: &str = \"use the --force\";\npub(crate) use a::b;\npub(in crate::m) use c::d;\n",
            ),
            [("a".into(), "b".to_string()), ("c".into(), "d".into())]
        );
        assert_eq!(
            read(
                Language::TypeScript,
                "export const h = \"import the 'x' file\";\nimport { y } from './y';\n",
            ),
            [("./y".into(), "y".to_string())]
        );
    }

    /// A bracket in a comment on an import line leaves the statement on its
    /// line, and the comment is no name.
    #[test]
    fn a_comment_on_an_import_line_is_no_part_of_it() {
        assert_eq!(
            imports_of(
                Language::Python,
                "from x import y  # noqa (legacy\nimport os\n\ndef f():\n    pass\n",
            ),
            [
                Import {
                    module: "x".into(),
                    name: Some("y".into()),
                    line: 1,
                },
                Import {
                    module: "os".into(),
                    name: None,
                    line: 2,
                },
            ]
        );
    }

    /// A key whose value is a list of mappings has no nested keys: the keys
    /// of the list's first item are no keys of the list.
    #[test]
    fn a_yaml_list_of_mappings_gives_no_nested_keys() {
        let symbols = parse(
            Language::Yaml,
            "items:\n  - kind: a\n    name: x\n  - kind: b\nserver:\n  host: h\n",
        );
        let names: Vec<&str> = symbols.iter().map(|s| s.qualified_name.as_str()).collect();
        assert_eq!(names, ["items", "server", "server.host"]);
    }

    #[test]
    fn an_impl_block_is_named_by_the_type_it_is_for() {
        assert_eq!(
            impl_subject("impl<T: Clone> Trait<T> for Type<T> where T: X {"),
            "Type"
        );
        assert_eq!(impl_subject("impl GitManager {"), "GitManager");
        assert_eq!(impl_subject("impl<'a> Iterator for Walk<'a> {"), "Walk");
    }

    /// A valid import keeps every name when its text exceeds the route scan
    /// limit.
    #[test]
    fn a_long_valid_import_keeps_every_name() {
        let names = (0..100)
            .map(|at| format!("item_{at:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("use crate::items::{{{names}}};");
        assert!(source.len() > STATEMENT_SCAN_MAX);

        let imports = imports_of(Language::Rust, &source);

        assert_eq!(imports.len(), 100);
        assert_eq!(
            imports.first().and_then(|item| item.name.as_deref()),
            Some("item_000")
        );
        assert_eq!(
            imports.last().and_then(|item| item.name.as_deref()),
            Some("item_099")
        );
        assert!(imports.iter().all(|item| item.module == "crate::items"));
    }

    /// Every registered language returns from hostile input within the file
    /// parser's time budget.
    ///
    /// The limit is on the CPU time of the calling thread, which is the work
    /// the readers do. On a loaded machine the thread waits for a core, and
    /// wall-clock time counts that wait as well.
    #[test]
    fn hostile_input_returns_for_every_language_within_two_cpu_seconds() {
        use std::time::Duration;

        const MIB: usize = 1024 * 1024;
        const LIMIT: Duration = Duration::from_secs(2);

        fn thread_cpu_time() -> Duration {
            let now = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
            Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
        }

        fn assert_returns(language: Language, name: &str, source: &str) {
            let started = thread_cpu_time();
            let parsed = read(language, source);
            assert!(
                thread_cpu_time() - started < LIMIT,
                "{} parser exceeded two CPU seconds on {name}",
                language.name()
            );

            let started = thread_cpu_time();
            crate::interfaces::read("hostile.txt", language, source, &parsed.symbols);
            assert!(
                thread_cpu_time() - started < LIMIT,
                "{} interface reader exceeded two CPU seconds on {name}",
                language.name()
            );
        }

        let multibyte_clamps = format!(
            "/// {}─\nfn f({}─: usize) {{ client.get(\"/v1/{}─\") }}",
            "a".repeat(DOC_MAX - 1),
            "a".repeat(SIGNATURE_MAX - 1),
            "a".repeat(594),
        );
        let nested_braces = format!("use {}item{};", "{".repeat(20_000), "}".repeat(20_000));
        let nested_html = format!("{}{}", "<div>".repeat(20_000), "</div>".repeat(20_000));
        let yaml_flow_lists = format!("{}value{}", "[".repeat(20_000), "]".repeat(20_000));
        let repeated_calls = "get(".repeat(MIB / 4);
        let minified_tail = format!("{}{}", "fn a(){}".repeat(5_000), "use a;".repeat(5_000));
        let generic_call = "x>(\"\")";
        let generic_calls = generic_call.repeat((MIB - minified_tail.len()) / generic_call.len());
        let mut one_line = format!("{minified_tail}{generic_calls}");
        one_line.push_str(&"x".repeat(MIB - one_line.len()));

        for language in Language::ALL {
            let parsed = read(language, "");
            crate::interfaces::read("hostile.txt", language, "", &parsed.symbols);
        }
        for language in Language::ALL {
            assert_returns(language, "registry text", "─");
        }
        for (language, name, source) in [
            (
                Language::Rust,
                "multibyte clamps",
                multibyte_clamps.as_str(),
            ),
            (
                Language::TypeScript,
                "multibyte route clamp",
                multibyte_clamps.as_str(),
            ),
            (Language::Rust, "nested braces", nested_braces.as_str()),
            (Language::Html, "nested HTML", nested_html.as_str()),
            (Language::Yaml, "YAML flow lists", yaml_flow_lists.as_str()),
            (
                Language::TypeScript,
                "repeated calls",
                repeated_calls.as_str(),
            ),
            (Language::Rust, "one line", one_line.as_str()),
            (Language::TypeScript, "one line", one_line.as_str()),
        ] {
            assert_returns(language, name, source);
        }
    }
}
