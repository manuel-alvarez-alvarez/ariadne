//! The language registry: what a file extension is read with.
//!
//! Each language names its grammar, the tags query that picks its
//! definitions out, how its doc comments are written, and what marks a
//! definition as a test. The tags queries are the ones the grammars ship;
//! Markdown ships none and is outline-only, its headings standing in for
//! definitions (see [`crate::parser`]).

use std::sync::OnceLock;

use tree_sitter_tags::TagsConfiguration;

/// One language the index reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    CSharp,
    Python,
    Markdown,
}

/// How a definition's doc comment is written, which is how it is read where
/// the tags query captures none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocSyntax {
    /// The lines right above the definition that start with this marker:
    /// `///` in Rust and C#.
    LinePrefix(&'static str),
    /// A `/** … */` block or `//` lines right above the definition.
    Block,
    /// A string literal as the first statement of the body.
    Docstring,
    /// No doc comments.
    None,
}

/// What marks a definition as a test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestRule {
    /// An attribute on the definition naming one of these: `#[test]` in
    /// Rust, `[Fact]` or `[Test]` in C#.
    Attribute(&'static [&'static str]),
    /// A definition whose name starts with this: `test_` in Python.
    NamePrefix(&'static str),
    /// A call to one of these, with the test's name as its first argument:
    /// `it(` and `test(` in TypeScript and JavaScript.
    Call(&'static [&'static str]),
    /// Nothing is a test.
    None,
}

impl Language {
    /// Every language the registry holds.
    pub const ALL: [Language; 8] = [
        Language::Rust,
        Language::TypeScript,
        Language::Tsx,
        Language::JavaScript,
        Language::Jsx,
        Language::CSharp,
        Language::Python,
        Language::Markdown,
    ];

    /// The language a path is read as, by its extension. `None` is a file
    /// the index skips.
    pub fn of_path(path: &str) -> Option<Language> {
        let extension = path.rsplit_once('.')?.1;
        // The extension is what comes after the last dot of the last
        // segment; a dot in a directory name is not one.
        if extension.contains('/') {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|language| language.extensions().contains(&extension))
    }

    /// How the language is named in the store and in the status.
    pub fn name(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
            Language::Jsx => "jsx",
            Language::CSharp => "csharp",
            Language::Python => "python",
            Language::Markdown => "markdown",
        }
    }

    /// The file extensions read as this language.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Language::Rust => &["rs"],
            Language::TypeScript => &["ts", "mts", "cts"],
            Language::Tsx => &["tsx"],
            Language::JavaScript => &["js", "mjs", "cjs"],
            Language::Jsx => &["jsx"],
            Language::CSharp => &["cs"],
            Language::Python => &["py", "pyi"],
            Language::Markdown => &["md", "markdown"],
        }
    }

    /// The grammar the language is parsed with.
    pub fn grammar(self) -> tree_sitter::Language {
        match self {
            Language::Rust => tree_sitter_rust::LANGUAGE.into(),
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Language::JavaScript | Language::Jsx => tree_sitter_javascript::LANGUAGE.into(),
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Language::Python => tree_sitter_python::LANGUAGE.into(),
            Language::Markdown => tree_sitter_md::LANGUAGE.into(),
        }
    }

    /// The tags query that names the language's definitions and references:
    /// the one its grammar ships. `None` for Markdown, whose outline is its
    /// headings rather than tags.
    pub fn tags_query(self) -> Option<String> {
        Some(match self {
            // The upstream query names an `impl` block only where its trait
            // is a bare identifier: `impl fmt::Display for T` names nothing,
            // and the methods in it would go unqualified. One pattern more
            // names every `impl` by its type; where an upstream pattern
            // matches the same block, its tag is the one kept.
            Language::Rust => format!(
                "{}\n(impl_item type: (_) @name) @reference.implementation\n",
                tree_sitter_rust::TAGS_QUERY
            ),
            // The TypeScript query holds only what TypeScript adds over
            // JavaScript, whose node names the two grammars share.
            Language::TypeScript | Language::Tsx => format!(
                "{}\n{}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Language::JavaScript | Language::Jsx => tree_sitter_javascript::TAGS_QUERY.to_string(),
            // The C# query ends in a bare `@module` capture, which is no
            // tags capture at all and is refused by the tags crate; the
            // `@definition.module` on the line above it is what names a
            // namespace.
            Language::CSharp => tree_sitter_c_sharp::TAGS_QUERY
                .lines()
                .filter(|line| !line.trim_end().ends_with("@module"))
                .collect::<Vec<_>>()
                .join("\n"),
            Language::Python => tree_sitter_python::TAGS_QUERY.to_string(),
            Language::Markdown => return None,
        })
    }

    /// How the language's doc comments are read.
    pub fn doc_syntax(self) -> DocSyntax {
        match self {
            Language::Rust | Language::CSharp => DocSyntax::LinePrefix("///"),
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
                DocSyntax::Block
            }
            Language::Python => DocSyntax::Docstring,
            Language::Markdown => DocSyntax::None,
        }
    }

    /// What marks a definition as a test.
    pub fn test_rule(self) -> TestRule {
        match self {
            Language::Rust => TestRule::Attribute(&["test"]),
            Language::CSharp => TestRule::Attribute(&["Fact", "Theory", "Test", "TestMethod"]),
            Language::Python => TestRule::NamePrefix("test_"),
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
                TestRule::Call(&["it", "test"])
            }
            Language::Markdown => TestRule::None,
        }
    }

    /// What joins a definition's name to the scope it sits in, in the
    /// qualified name.
    pub fn separator(self) -> &'static str {
        match self {
            Language::Rust => "::",
            Language::Markdown => " > ",
            _ => ".",
        }
    }

    /// The compiled tags query, built once per language for the life of the
    /// process. `None` for a language with no tags query.
    pub(crate) fn tags(self) -> Option<&'static TagsConfiguration> {
        static CONFIGURATIONS: OnceLock<Vec<Option<TagsConfiguration>>> = OnceLock::new();
        CONFIGURATIONS
            .get_or_init(|| {
                Self::ALL
                    .into_iter()
                    .map(|language| {
                        let query = language.tags_query()?;
                        let mut configuration = TagsConfiguration::new(
                            language.grammar(),
                            &query,
                            "",
                        )
                        .unwrap_or_else(|e| {
                            panic!("the {} tags query does not compile: {e}", language.name())
                        });
                        language.disable_unread_patterns(&mut configuration);
                        Some(configuration)
                    })
                    .collect()
            })
            .get(self as usize)
            .and_then(Option::as_ref)
    }

    /// Turn off the patterns of the tags query whose tags the index never
    /// reads: every reference but the ones a rule of this language needs.
    ///
    /// A reference is most of what a query captures — every call, every
    /// type mention — and the tags crate computes docs, line and column
    /// information for each one. Skipping them at the cursor is what keeps
    /// a whole repository under a few seconds.
    fn disable_unread_patterns(self, configuration: &mut TagsConfiguration) {
        let query = &mut configuration.query;
        let names: Vec<String> = query
            .capture_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        for pattern in 0..query.pattern_count() {
            let read = query
                .capture_quantifiers(pattern)
                .iter()
                .zip(&names)
                .filter(|(quantifier, _)| **quantifier != tree_sitter::CaptureQuantifier::Zero)
                .any(|(_, name)| self.reads_capture(name));
            if !read {
                query.disable_pattern(pattern);
            }
        }
    }

    /// Whether the index reads tags of this capture: every definition, the
    /// `impl` blocks that qualify a Rust method, and the calls a test rule
    /// looks at.
    fn reads_capture(self, name: &str) -> bool {
        name.starts_with("definition.")
            || (name == "reference.implementation" && self == Language::Rust)
            || (name == "reference.call" && matches!(self.test_rule(), TestRule::Call(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_read_by_its_extension() {
        assert_eq!(
            Language::of_path("crates/a/src/lib.rs"),
            Some(Language::Rust)
        );
        assert_eq!(Language::of_path("ui/src/app.tsx"), Some(Language::Tsx));
        assert_eq!(
            Language::of_path("ui/src/app.ts"),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::of_path("web/index.mjs"),
            Some(Language::JavaScript)
        );
        assert_eq!(Language::of_path("web/App.jsx"), Some(Language::Jsx));
        assert_eq!(Language::of_path("Api/Program.cs"), Some(Language::CSharp));
        assert_eq!(Language::of_path("tools/build.py"), Some(Language::Python));
        assert_eq!(
            Language::of_path("docs/README.md"),
            Some(Language::Markdown)
        );
        assert_eq!(Language::of_path("Cargo.lock"), None);
        assert_eq!(Language::of_path("a.dir/Makefile"), None);
        assert_eq!(Language::of_path("LICENSE"), None);
    }

    /// Every tags query the registry ships compiles against its grammar.
    #[test]
    fn every_tags_query_compiles() {
        for language in Language::ALL {
            assert_eq!(
                language.tags().is_some(),
                language.tags_query().is_some(),
                "{}",
                language.name()
            );
        }
    }
}
