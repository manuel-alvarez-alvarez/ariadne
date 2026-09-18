//! One file's text to the definitions in it.
//!
//! A code file is read with its language's tags query: every definition the
//! query captures becomes a [`Symbol`], qualified by the definitions it sits
//! inside, with the signature read off its first lines, the doc comment
//! read by the language's comment syntax where the query captured none, and
//! the test marker rule applied. A Markdown file is read for its headings.

use std::ops::Range;

use tree_sitter::StreamingIterator;
use tree_sitter_tags::TagsContext;

use crate::languages::{DocSyntax, Language, TestRule};

/// One definition of a file: what the index stores per blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// The tags vocabulary: `function`, `method`, `class`, `module`,
    /// `interface`, `macro`, `constant`; `test` for a test the language
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

/// How long a signature or a doc is allowed to run, in characters. A
/// signature is what a search answer prints per line, and 200 is a whole
/// parameter list; a doc is what an outline may show, and it is a summary,
/// not a manual.
const SIGNATURE_MAX: usize = 200;
const DOC_MAX: usize = 1000;

/// The definitions of `source`, read as `language`.
pub fn parse(language: Language, source: &str) -> Vec<Symbol> {
    match language {
        Language::Markdown => return markdown_outline(source),
        Language::Yaml => return yaml_outline(source),
        Language::Toml => return toml_outline(source),
        Language::Json => return json_outline(source),
        Language::Html => return html_outline(source),
        Language::Css => return css_outline(source),
        Language::Sql => return sql_outline(source),
        _ => {}
    }
    let Some(configuration) = language.tags() else {
        return Vec::new();
    };
    let mut context = TagsContext::new();
    let Ok((tags, _)) = context.generate_tags(configuration, source.as_bytes(), None) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut items: Vec<Item> = tags
        .filter_map(Result::ok)
        .map(|tag| Item {
            kind: configuration
                .syntax_type_name(tag.syntax_type_id)
                .to_string(),
            name: source[tag.name_range.clone()].to_string(),
            lines: lines.of_range(&tag.range),
            range: tag.range,
            name_range: tag.name_range,
            docs: tag.docs,
            is_definition: tag.is_definition,
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
    let mut scopes: Vec<(usize, String)> = Vec::new();
    let mut symbols = Vec::new();
    for item in items {
        while scopes
            .last()
            .is_some_and(|(end, _)| *end <= item.range.start)
        {
            scopes.pop();
        }
        let parent = scopes.last().map(|(_, qualified)| qualified.as_str());
        let qualify = |name: &str| match parent {
            Some(parent) => format!("{parent}{separator}{name}"),
            None => name.to_string(),
        };
        if item.is_definition {
            let qualified_name = qualify(&item.name);
            if is_scope(&item.kind) {
                scopes.push((item.range.end, qualified_name.clone()));
            }
            let doc = item
                .docs
                .as_deref()
                .map(clean_doc)
                .filter(|doc| !doc.is_empty())
                .or_else(|| doc_of(doc_syntax, source, &item));
            let is_test = match test_rule {
                TestRule::Attribute(names) => has_attribute(source, &item, names),
                TestRule::Annotation(names) => has_annotation(source, &item, names),
                TestRule::NamePrefix(prefix) => item.name.starts_with(prefix),
                TestRule::AnnotationOrNamePrefix(names, prefix) => {
                    has_annotation(source, &item, names) || item.name.starts_with(prefix)
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
        } else if item.kind == "implementation" {
            // `impl Type { … }` in Rust: a scope for the methods in it, and
            // no definition of its own.
            scopes.push((
                item.range.end,
                qualify(&impl_subject(&source[item.range.clone()])),
            ));
        } else if let TestRule::Call(names) = test_rule
            && item.kind == "call"
            && names.contains(&item.name.as_str())
        {
            let name = first_string_argument(&source[item.range.clone()])
                .unwrap_or_else(|| item.name.clone());
            symbols.push(Symbol {
                kind: "test".into(),
                qualified_name: qualify(&name),
                name,
                start_line: item.lines.0,
                end_line: item.lines.1,
                signature: signature_of(source, &item.range, item.name_range.end),
                doc: None,
                is_test: true,
            });
        }
    }
    symbols
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
struct Lines(Vec<usize>);

impl Lines {
    fn of(source: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));
        Self(starts)
    }

    /// The 1-based line holding byte `at`.
    fn line_of(&self, at: usize) -> u32 {
        self.0.partition_point(|start| *start <= at) as u32
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
fn doc_of(syntax: DocSyntax, source: &str, item: &Item) -> Option<String> {
    let doc = match syntax {
        DocSyntax::LinePrefix(prefix) => {
            let lines: Vec<&str> = lines_above(source, item.range.start)
                .filter(|line| !is_attribute_line(line))
                .take_while(|line| line.trim_start().starts_with(prefix))
                .map(|line| line.trim_start()[prefix.len()..].trim())
                .collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        }
        DocSyntax::Block => {
            let mut lines: Vec<&str> = Vec::new();
            let mut above = lines_above(source, item.range.start).peekable();
            match above.peek().map(|line| line.trim()) {
                Some(line) if line.ends_with("*/") => {
                    for line in above {
                        lines.push(line);
                        if line.trim_start().starts_with("/*") {
                            break;
                        }
                    }
                }
                _ => lines.extend(above.take_while(|line| line.trim_start().starts_with("//"))),
            }
            clean_doc(&lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
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
fn lines_above(source: &str, at: usize) -> impl Iterator<Item = &str> {
    let line_start = source[..at].rfind('\n').map_or(0, |nl| nl + 1);
    source[..line_start].lines().rev()
}

/// A Rust `#[…]` or C# `[…]` attribute on a line of its own.
fn is_attribute_line(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with("#[") || line.starts_with('[')) && line.ends_with(']')
}

/// The docstring of a Python definition: a string literal as the first
/// statement of the body, which starts after the line the signature ends
/// on with a colon.
fn docstring(text: &str, after: usize) -> Option<String> {
    let body_at = text[after..].find(":\n").map(|at| after + at + 2)?;
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
fn has_attribute(source: &str, item: &Item, names: &[&str]) -> bool {
    let above = lines_above(source, item.range.start)
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
fn has_annotation(source: &str, item: &Item, names: &[&str]) -> bool {
    let above = lines_above(source, item.range.start)
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

/// `text` split on the commas outside parentheses: the items of
/// `Fact, Trait("a", "b")` are two.
fn split_outside_parentheses(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (at, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
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

/// The first string literal in a call's text: the name of an `it(…)`.
fn first_string_argument(text: &str) -> Option<String> {
    let open = text.find(['\'', '"', '`'])?;
    let quote = text[open..].chars().next()?;
    let inner = &text[open + quote.len_utf8()..];
    let close = inner.find(quote)?;
    let name = inner[..close].trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The headings of a Markdown file, each spanning to the next heading of
/// its level or above.
fn markdown_outline(source: &str) -> Vec<Symbol> {
    let grammar = Language::Markdown.grammar();
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&grammar).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
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
            let text = &source[node.byte_range()];
            let name = node
                .child_by_field_name("heading_content")
                .map(|content| source[content.byte_range()].to_string())
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

/// A parser for `language`, or an empty outline where the grammar refuses
/// to load.
fn outline_parser(language: Language) -> Option<tree_sitter::Parser> {
    let grammar = language.grammar();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&grammar).ok()?;
    Some(parser)
}

/// The top-level keys of a JSON object, and one level of keys nested under
/// an object value: JSON has no comments to read for a doc.
fn json_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Json) else {
        return Vec::new();
    };
    let Some(tree) = parser.parse(source, None) else {
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
            .map(|content| source[content.byte_range()].to_string())
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
            signature: first_line_signature(&source[pair.byte_range()]),
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
    let Some(tree) = parser.parse(source, None) else {
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
    if node.kind() == "block_mapping" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(find_block_mapping)
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
        let name = source[key.byte_range()].trim().to_string();
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
            signature: first_line_signature(&source[pair.byte_range()]),
            doc: None,
            is_test: false,
        });
        if depth < 2
            && let Some(value) = pair.child_by_field_name("value")
            && let Some(nested) = find_block_mapping(value)
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

/// The top-level keys of a TOML document, and the keys of each `[table]`.
fn toml_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Toml) else {
        return Vec::new();
    };
    let Some(tree) = parser.parse(source, None) else {
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
                    signature: first_line_signature(&source[node.byte_range()]),
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
        signature: first_line_signature(&source[pair.byte_range()]),
        doc: None,
        is_test: false,
    })
}

/// A TOML key's text, its quotes stripped where it is a quoted key.
fn toml_key_text(node: tree_sitter::Node, source: &str) -> String {
    source[node.byte_range()]
        .trim_matches(['"', '\''])
        .to_string()
}

/// The elements of an HTML file that carry an `id`, named by its value.
fn html_outline(source: &str) -> Vec<Symbol> {
    let Some(mut parser) = outline_parser(Language::Html) else {
        return Vec::new();
    };
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let lines = Lines::of(source);
    let mut symbols = Vec::new();
    html_elements(tree.root_node(), source, &lines, &mut symbols);
    symbols
}

/// Every element at or under `node` that carries an `id`, depth-first.
fn html_elements(node: tree_sitter::Node, source: &str, lines: &Lines, symbols: &mut Vec<Symbol>) {
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
                signature: first_line_signature(&source[start_tag.byte_range()]),
                doc: None,
                is_test: false,
            });
        }
        html_elements(child, source, lines, symbols);
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
                "attribute_name" => is_id = &source[part.byte_range()] == "id",
                "quoted_attribute_value" => {
                    value = part
                        .named_child(0)
                        .map(|content| source[content.byte_range()].to_string())
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
    let Some(tree) = parser.parse(source, None) else {
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
        let name = source[selectors.byte_range()]
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
    let Some(tree) = parser.parse(source, None) else {
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
            signature: first_line_signature(&source[statement.byte_range()]),
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
                Some(source[name.byte_range()].to_string())
            }
            "identifier" => Some(source[child.byte_range()].to_string()),
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

    #[test]
    fn an_impl_block_is_named_by_the_type_it_is_for() {
        assert_eq!(
            impl_subject("impl<T: Clone> Trait<T> for Type<T> where T: X {"),
            "Type"
        );
        assert_eq!(impl_subject("impl GitManager {"), "GitManager");
        assert_eq!(impl_subject("impl<'a> Iterator for Walk<'a> {"), "Walk");
    }
}
