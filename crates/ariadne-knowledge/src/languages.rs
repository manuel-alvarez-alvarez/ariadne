//! The language registry: what a file extension is read with.
//!
//! Each language names its grammar, the tags query that picks its
//! definitions out, how its doc comments are written, and what marks a
//! definition as a test. The tags queries are the ones the grammars ship,
//! except Scala's, Kotlin's and Bash's, whose grammars ship none: those
//! three are vendored from Aider (`src/vendor/`, see `NOTICE`). An
//! outline-only format — Markdown and the six formats after it — has no
//! tags query at all; its structure stands in for definitions (see
//! [`crate::parser`]).

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
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    Kotlin,
    Swift,
    Dart,
    Scala,
    Bash,
    Lua,
    Elixir,
    /// Outline-only: no tags query, its headings standing in for
    /// definitions.
    Markdown,
    /// Outline-only: its top-level and depth-2 keys standing in for
    /// definitions.
    Yaml,
    /// Outline-only: its top-level keys and each table's keys standing in
    /// for definitions.
    Toml,
    /// Outline-only: its top-level and depth-2 keys standing in for
    /// definitions.
    Json,
    /// Outline-only: its elements that carry an `id` standing in for
    /// definitions.
    Html,
    /// Outline-only: its selectors standing in for definitions.
    Css,
    /// Outline-only: the object names of its `CREATE` and `ALTER`
    /// statements standing in for definitions.
    Sql,
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
    /// An `@Name` annotation on the definition naming one of these: `@Test`
    /// in Java and Kotlin.
    Annotation(&'static [&'static str]),
    /// A definition whose name starts with this: `test_` in Python, `Test`
    /// in Go, `test` in PHPUnit.
    NamePrefix(&'static str),
    /// A call to one of these, with the test's name as its first argument:
    /// `it(` and `test(` in TypeScript and JavaScript, `describe`/`it` in
    /// Ruby, `test(` in Dart and Scala, `test` in Elixir, a bats `@test` in
    /// Bash.
    Call(&'static [&'static str]),
    /// Either of these marks a test: an `@Name` annotation, or a name
    /// starting with a prefix — Swift's `@Test` and its `test` names in an
    /// `XCTestCase` subclass.
    AnnotationOrNamePrefix(&'static [&'static str], &'static str),
    /// Nothing is a test.
    None,
}

impl Language {
    /// Every language the registry holds: 20 read by a tags query, and 7
    /// outline-only formats.
    pub const ALL: [Language; 27] = [
        Language::Rust,
        Language::TypeScript,
        Language::Tsx,
        Language::JavaScript,
        Language::Jsx,
        Language::CSharp,
        Language::Python,
        Language::Go,
        Language::Java,
        Language::C,
        Language::Cpp,
        Language::Ruby,
        Language::Php,
        Language::Kotlin,
        Language::Swift,
        Language::Dart,
        Language::Scala,
        Language::Bash,
        Language::Lua,
        Language::Elixir,
        Language::Markdown,
        Language::Yaml,
        Language::Toml,
        Language::Json,
        Language::Html,
        Language::Css,
        Language::Sql,
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
            Language::Go => "go",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Ruby => "ruby",
            Language::Php => "php",
            Language::Kotlin => "kotlin",
            Language::Swift => "swift",
            Language::Dart => "dart",
            Language::Scala => "scala",
            Language::Bash => "bash",
            Language::Lua => "lua",
            Language::Elixir => "elixir",
            Language::Markdown => "markdown",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Json => "json",
            Language::Html => "html",
            Language::Css => "css",
            Language::Sql => "sql",
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
            Language::Go => &["go"],
            Language::Java => &["java"],
            Language::C => &["c", "h"],
            Language::Cpp => &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
            Language::Ruby => &["rb"],
            Language::Php => &["php"],
            Language::Kotlin => &["kt", "kts"],
            Language::Swift => &["swift"],
            Language::Dart => &["dart"],
            Language::Scala => &["scala", "sc"],
            Language::Bash => &["sh", "bash", "bats"],
            Language::Lua => &["lua"],
            Language::Elixir => &["ex", "exs"],
            Language::Markdown => &["md", "markdown"],
            Language::Yaml => &["yaml", "yml"],
            Language::Toml => &["toml"],
            Language::Json => &["json"],
            Language::Html => &["html", "htm"],
            Language::Css => &["css"],
            Language::Sql => &["sql"],
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
            Language::Go => tree_sitter_go::LANGUAGE.into(),
            Language::Java => tree_sitter_java::LANGUAGE.into(),
            Language::C => tree_sitter_c::LANGUAGE.into(),
            Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Language::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Language::Swift => tree_sitter_swift::LANGUAGE.into(),
            Language::Dart => tree_sitter_dart::LANGUAGE.into(),
            Language::Scala => tree_sitter_scala::LANGUAGE.into(),
            Language::Bash => tree_sitter_bash::LANGUAGE.into(),
            Language::Lua => tree_sitter_lua::LANGUAGE.into(),
            Language::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            Language::Markdown => tree_sitter_md::LANGUAGE.into(),
            Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Language::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Language::Json => tree_sitter_json::LANGUAGE.into(),
            Language::Html => tree_sitter_html::LANGUAGE.into(),
            Language::Css => tree_sitter_css::LANGUAGE.into(),
            Language::Sql => tree_sitter_sequel::LANGUAGE.into(),
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
            Language::Go => tree_sitter_go::TAGS_QUERY.to_string(),
            Language::Java => tree_sitter_java::TAGS_QUERY.to_string(),
            Language::C => tree_sitter_c::TAGS_QUERY.to_string(),
            Language::Cpp => tree_sitter_cpp::TAGS_QUERY.to_string(),
            Language::Ruby => tree_sitter_ruby::TAGS_QUERY.to_string(),
            Language::Php => tree_sitter_php::TAGS_QUERY.to_string(),
            // Neither grammar ships a tags query; these are vendored from
            // Aider (see the module doc and `NOTICE`).
            Language::Kotlin => include_str!("vendor/kotlin-tags.scm").to_string(),
            Language::Scala => include_str!("vendor/scala-tags.scm").to_string(),
            Language::Bash => include_str!("vendor/bash-tags.scm").to_string(),
            // Three patterns tag a method, an `init`/`deinit`/subscript or a
            // property by the range of the whole class or protocol around
            // it, not its own: `end_line` would be the enclosing type's.
            // Dropping them leaves every plain function — a method among
            // them — tagged as `function` by the one pattern that already
            // gives it its own range; `init`, `deinit` and a subscript are
            // not tagged at all.
            Language::Swift => tree_sitter_swift::TAGS_QUERY
                .split("\n\n")
                .filter(|block| !block.contains("_body"))
                .collect::<Vec<_>>()
                .join("\n\n"),
            Language::Dart => tree_sitter_dart::TAGS_QUERY.to_string(),
            Language::Lua => tree_sitter_lua::TAGS_QUERY.to_string(),
            Language::Elixir => tree_sitter_elixir::TAGS_QUERY.to_string(),
            Language::Markdown
            | Language::Yaml
            | Language::Toml
            | Language::Json
            | Language::Html
            | Language::Css
            | Language::Sql => return None,
        })
    }

    /// How the language's doc comments are read.
    pub fn doc_syntax(self) -> DocSyntax {
        match self {
            Language::Rust | Language::CSharp => DocSyntax::LinePrefix("///"),
            Language::TypeScript
            | Language::Tsx
            | Language::JavaScript
            | Language::Jsx
            | Language::Go
            | Language::Java
            | Language::C
            | Language::Cpp
            | Language::Php
            | Language::Kotlin
            | Language::Scala => DocSyntax::Block,
            Language::Python => DocSyntax::Docstring,
            Language::Ruby | Language::Bash => DocSyntax::LinePrefix("#"),
            Language::Swift | Language::Dart => DocSyntax::LinePrefix("///"),
            Language::Lua => DocSyntax::LinePrefix("--"),
            // Elixir's doc comment is a `@doc """ … """` attribute above the
            // definition, not a comment; it is not read.
            Language::Elixir => DocSyntax::None,
            Language::Markdown
            | Language::Yaml
            | Language::Toml
            | Language::Json
            | Language::Html
            | Language::Css
            | Language::Sql => DocSyntax::None,
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
            Language::Go => TestRule::NamePrefix("Test"),
            Language::Java | Language::Kotlin => TestRule::Annotation(&["Test"]),
            Language::Ruby => TestRule::Call(&["describe", "it"]),
            Language::Php => TestRule::NamePrefix("test"),
            Language::Swift => TestRule::AnnotationOrNamePrefix(&["Test"], "test"),
            Language::Dart | Language::Scala => TestRule::Call(&["test"]),
            Language::Elixir => TestRule::Call(&["test"]),
            Language::Bash => TestRule::Call(&["@test"]),
            // Neither C, C++ nor Lua has a common test marker.
            Language::C | Language::Cpp | Language::Lua => TestRule::None,
            Language::Markdown
            | Language::Yaml
            | Language::Toml
            | Language::Json
            | Language::Html
            | Language::Css
            | Language::Sql => TestRule::None,
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
        assert_eq!(Language::of_path("cmd/main.go"), Some(Language::Go));
        assert_eq!(Language::of_path("src/Main.java"), Some(Language::Java));
        assert_eq!(Language::of_path("src/lib.c"), Some(Language::C));
        assert_eq!(Language::of_path("include/lib.hpp"), Some(Language::Cpp));
        assert_eq!(Language::of_path("app/model.rb"), Some(Language::Ruby));
        assert_eq!(Language::of_path("src/index.php"), Some(Language::Php));
        assert_eq!(Language::of_path("app/Main.kt"), Some(Language::Kotlin));
        assert_eq!(
            Language::of_path("Sources/App.swift"),
            Some(Language::Swift)
        );
        assert_eq!(Language::of_path("lib/main.dart"), Some(Language::Dart));
        assert_eq!(Language::of_path("src/Main.scala"), Some(Language::Scala));
        assert_eq!(Language::of_path("bin/run.sh"), Some(Language::Bash));
        assert_eq!(Language::of_path("script.bats"), Some(Language::Bash));
        assert_eq!(Language::of_path("src/util.lua"), Some(Language::Lua));
        assert_eq!(Language::of_path("lib/calc.ex"), Some(Language::Elixir));
        assert_eq!(Language::of_path("config/app.yaml"), Some(Language::Yaml));
        assert_eq!(Language::of_path("Cargo.toml"), Some(Language::Toml));
        assert_eq!(Language::of_path("package.json"), Some(Language::Json));
        assert_eq!(Language::of_path("public/index.html"), Some(Language::Html));
        assert_eq!(Language::of_path("styles/app.css"), Some(Language::Css));
        assert_eq!(Language::of_path("db/schema.sql"), Some(Language::Sql));
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
