//! The names a blob names, to the definitions behind them.
//!
//! One reference is one name and nothing more: the parser saw `b()` and does
//! not know which `b`. This is where a name becomes an edge, by looking for
//! its definition in four places, nearest first:
//!
//! 1. the same file,
//! 2. the same module or directory,
//! 3. the modules the file imports,
//! 4. any definition of that name in the repository.
//!
//! The first of those that holds a definition is the one that answers, and
//! the edge says which one did. One definition there is `exact`; several are
//! `heuristic`, the count is kept, and an edge is written to each — an author
//! asking who calls a name is better served by a list that holds the answer
//! than by a guess.
//!
//! The second half is what joins two repositories, or two files of one: the
//! interfaces of [`crate::interfaces`], matched by name and by route, in
//! [`interface_edges`].

use std::collections::{HashMap, HashSet};

use crate::interfaces::{InterfaceKind, route_match};
use crate::languages::Language;
use crate::parser::{EdgeKind, Import};

/// How many definitions a name may match and still be resolved. Past this,
/// the name says nothing about which definition was meant — `new` in a large
/// repository — and the edges would be noise measured in thousands.
pub const MAX_CANDIDATES: usize = 20;

/// The shortest name a reference across repositories is looked up by: a
/// shorter one — `get`, `run`, `new` — is defined everywhere and means
/// nothing in particular.
pub const MIN_FOREIGN_NAME: usize = 4;

/// One definition a name could mean, at one ref of one repository.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub id: i64,
    pub blob: String,
    pub path: String,
    pub language: String,
    pub qualified_name: String,
    pub start_line: i64,
}

/// One reference of one blob, as the store holds it.
#[derive(Clone, Debug)]
pub(crate) struct Mention {
    pub kind: String,
    pub name: String,
    pub line: i64,
    pub from_symbol: Option<i64>,
}

/// Everything one blob names, gathered for the resolution pass.
#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    pub path: String,
    pub mentions: Vec<Mention>,
    pub imports: Vec<Import>,
}

/// One interface of one blob at a ref, as the store holds it.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct InterfaceRow {
    pub blob: String,
    pub path: String,
    pub kind: String,
    pub name: String,
    pub line: i64,
    pub symbol: Option<i64>,
    /// The handler names a route registration passes, space-separated.
    pub handlers: Option<String>,
}

/// One row of `edges`, without the repositories and refs of its two ends:
/// those are the pair the edge is committed under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub kind: String,
    pub from_blob: String,
    pub from_symbol: Option<i64>,
    pub from_line: i64,
    pub to_blob: String,
    pub to_symbol: Option<i64>,
    pub to_line: i64,
    /// What an interface edge is about, and the name a foreign reference
    /// named. `None` on a symbol edge within one repository.
    pub name: Option<String>,
    pub confidence: &'static str,
    /// Which step answered the name: one of the four of rule 9 for a symbol
    /// edge, and what joined the two ends for an interface edge.
    pub step: &'static str,
    pub candidates: i64,
}

/// What a name resolved to: the definitions of the step that answered, which
/// step that was, and whether one of them matched or several.
#[derive(Clone, Debug)]
pub(crate) struct Resolved<'a> {
    pub candidates: Vec<&'a Candidate>,
    pub step: &'static str,
    pub confidence: &'static str,
}

/// The kinds an interface edge can be: the edges [`interface_edges`]
/// derives, and the ones the link pass replaces as a pair.
pub const INTERFACE_KINDS: [&str; 3] = ["depends_on", "calls_route", "sets_env"];

/// The interface edges from every row of `from` to every row of `to`: a
/// dependency to the package it names, a route use to the template it fits,
/// a set variable to every read of it. `handlers` holds the definitions the
/// route registrations of `to` name, which is where a route edge points
/// when one of them resolves; the registration itself is where it points
/// otherwise.
pub(crate) fn interface_edges(
    from: &[InterfaceRow],
    to: &[InterfaceRow],
    handlers: &HashMap<String, Vec<Candidate>>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen: HashSet<(String, String, i64, String, Option<i64>, i64)> = HashSet::new();
    let mut push = |kind: &str,
                    a: &InterfaceRow,
                    to_blob: &str,
                    to_symbol: Option<i64>,
                    to_line: i64,
                    name: &str,
                    confidence: &'static str,
                    step: &'static str,
                    candidates: i64| {
        let key = (
            kind.to_string(),
            a.blob.clone(),
            a.line,
            to_blob.to_string(),
            to_symbol,
            to_line,
        );
        if seen.insert(key) {
            edges.push(Edge {
                kind: kind.to_string(),
                from_blob: a.blob.clone(),
                from_symbol: a.symbol,
                from_line: a.line,
                to_blob: to_blob.to_string(),
                to_symbol,
                to_line,
                name: Some(name.to_string()),
                confidence,
                step,
                candidates,
            });
        }
    };
    fn of(rows: &[InterfaceRow], kind: InterfaceKind) -> Vec<&InterfaceRow> {
        rows.iter()
            .filter(|row| row.kind == kind.as_str())
            .collect()
    }
    // A dependency to the package it names.
    let packages = of(to, InterfaceKind::Package);
    for dependency in from
        .iter()
        .filter(|row| row.kind == "dependency" || row.kind == "path_dependency")
    {
        let (confidence, step) = match dependency.kind.as_str() {
            "path_dependency" => ("exact", "path"),
            _ => ("heuristic", "name"),
        };
        for package in packages.iter().filter(|p| p.name == dependency.name) {
            push(
                "depends_on",
                dependency,
                &package.blob,
                package.symbol,
                package.line,
                &package.name,
                confidence,
                step,
                1,
            );
        }
    }
    // A request to the template it fits. The edge points at the handler
    // where the registration named one the ref defines — in the same file
    // first, then wherever it is — and at the registration itself otherwise.
    let templates = of(to, InterfaceKind::Route);
    for used in of(from, InterfaceKind::RouteUse) {
        for template in &templates {
            let Some(confidence) = route_match(&used.name, &template.name) else {
                continue;
            };
            let resolved: Vec<&Candidate> = template
                .handlers
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|name| handlers.get(name))
                .filter_map(|matched| narrow(matched, &template.blob, &template.path, &[]))
                .flat_map(|resolved| resolved.candidates)
                .collect();
            match resolved.is_empty() {
                true => push(
                    "calls_route",
                    used,
                    &template.blob,
                    template.symbol,
                    template.line,
                    &template.name,
                    confidence,
                    "route",
                    1,
                ),
                false => {
                    let count = resolved.len() as i64;
                    for handler in resolved {
                        push(
                            "calls_route",
                            used,
                            &handler.blob,
                            Some(handler.id),
                            handler.start_line,
                            &template.name,
                            confidence,
                            "route",
                            count,
                        );
                    }
                }
            }
        }
    }
    // A set variable to every read of it.
    let reads = of(to, InterfaceKind::EnvRead);
    for set in of(from, InterfaceKind::EnvSet) {
        for read in reads.iter().filter(|read| read.name == set.name) {
            push(
                "sets_env",
                set,
                &read.blob,
                read.symbol,
                read.line,
                &read.name,
                "exact",
                "name",
                1,
            );
        }
    }
    edges
}

/// The edges of every blob in `named`, resolved against `candidates`: the
/// definitions of every name they mention, at the ref being indexed.
pub(crate) fn edges_of(
    named: &HashMap<String, Names>,
    candidates: &HashMap<String, Vec<Candidate>>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    for (blob, names) in named {
        // The modules the file imports, read once for the file rather than
        // once for every name it names.
        let modules: Vec<Vec<&str>> = names
            .imports
            .iter()
            .map(|import| module_segments(&import.module))
            .filter(|segments| !segments.is_empty())
            .collect();
        // An import is a reference of the file itself, standing outside
        // every definition in it.
        let imported = names.imports.iter().filter_map(|import| {
            Some(Mention {
                kind: EdgeKind::Imports.as_str().to_string(),
                name: import.name.clone()?,
                line: import.line as i64,
                from_symbol: None,
            })
        });
        // A name resolves the same way wherever in the file it is named.
        let mut narrowed: HashMap<String, Option<Resolved>> = HashMap::new();
        // One edge per pair, within this blob. Per blob and not per run: a
        // mention at file scope has no definition to be from, so two files
        // that import the same name would share a key across the run and only
        // the first would get its edge.
        let mut seen: HashSet<(String, Option<i64>, i64)> = HashSet::new();
        for mention in names.mentions.iter().cloned().chain(imported) {
            let resolved = narrowed.entry(mention.name.clone()).or_insert_with(|| {
                let matched = candidates.get(&mention.name)?;
                narrow(matched, blob, &names.path, &modules)
            });
            let Some(resolved) = resolved else {
                continue;
            };
            for candidate in resolved.candidates.iter() {
                if Some(candidate.id) == mention.from_symbol {
                    continue;
                }
                let key = (mention.kind.clone(), mention.from_symbol, candidate.id);
                if !seen.insert(key) {
                    continue;
                }
                edges.push(Edge {
                    kind: mention.kind.clone(),
                    from_blob: blob.clone(),
                    from_symbol: mention.from_symbol,
                    from_line: mention.line,
                    to_blob: candidate.blob.clone(),
                    to_symbol: Some(candidate.id),
                    to_line: candidate.start_line,
                    name: None,
                    confidence: resolved.confidence,
                    step: resolved.step,
                    candidates: resolved.candidates.len() as i64,
                });
            }
        }
    }
    edges
}

/// The foreign references of one ref: every mention of a name the ref
/// defines nowhere, pointed at the definitions of that name in another
/// repository. Each is `heuristic` — the name is all that joins them — and
/// carries how many definitions it matched.
///
/// `mentions` are the ones whose names `defined` holds, keyed by name;
/// `defined` is what the other repository defines under each name. A name
/// past [`MAX_CANDIDATES`] says nothing about which definition was meant
/// and makes no edge.
pub(crate) fn foreign_edges(
    mentions: &HashMap<String, Vec<(String, Mention)>>,
    defined: &HashMap<String, Vec<Candidate>>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    let mut seen: HashSet<(String, Option<i64>, i64)> = HashSet::new();
    for (name, candidates) in defined {
        let candidates: Vec<&Candidate> = candidates
            .iter()
            .filter(|candidate| !is_outline_only(candidate))
            .collect();
        if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
            continue;
        }
        let Some(named) = mentions.get(name) else {
            continue;
        };
        for (blob, mention) in named {
            for candidate in &candidates {
                if !seen.insert((blob.clone(), mention.from_symbol, candidate.id)) {
                    continue;
                }
                edges.push(Edge {
                    kind: "references".into(),
                    from_blob: blob.clone(),
                    from_symbol: mention.from_symbol,
                    from_line: mention.line,
                    to_blob: candidate.blob.clone(),
                    to_symbol: Some(candidate.id),
                    to_line: candidate.start_line,
                    name: Some(name.clone()),
                    confidence: "heuristic",
                    step: "name",
                    candidates: candidates.len() as i64,
                });
            }
        }
    }
    edges
}

/// The definitions a name resolves to, which step answered and how sure that
/// is: the nearest step that holds any, unless it holds more than
/// [`MAX_CANDIDATES`].
fn narrow<'a>(
    matched: &'a [Candidate],
    blob: &str,
    path: &str,
    modules: &[Vec<&str>],
) -> Option<Resolved<'a>> {
    let directory = directory_of(path);
    let held = |step: &'static str, found: Vec<&'a Candidate>| {
        (!found.is_empty()).then_some((step, found))
    };
    let matched: Vec<&Candidate> = matched
        .iter()
        .filter(|candidate| !is_outline_only(candidate))
        .collect();
    let (step, candidates) = held(
        "file",
        matched
            .iter()
            .copied()
            .filter(|candidate| candidate.blob == blob)
            .collect(),
    )
    // The same module or directory.
    .or_else(|| {
        held(
            "directory",
            matched
                .iter()
                .copied()
                .filter(|candidate| directory_of(&candidate.path) == directory)
                .collect(),
        )
    })
    // What the file imports.
    .or_else(|| {
        held(
            "import",
            matched
                .iter()
                .copied()
                .filter(|candidate| modules.iter().any(|module| module_holds(module, candidate)))
                .collect(),
        )
    })
    // Anywhere in the repository.
    .or_else(|| held("repository", matched))?;
    if candidates.len() > MAX_CANDIDATES {
        return None;
    }
    let confidence = match candidates.len() {
        1 => "exact",
        _ => "heuristic",
    };
    Some(Resolved {
        candidates,
        step,
        confidence,
    })
}

/// Whether a candidate is a definition from an outline-only format.
fn is_outline_only(candidate: &Candidate) -> bool {
    Language::OUTLINE_ONLY
        .into_iter()
        .any(|language| language.name() == candidate.language)
}

/// Whether a definition could be what an import statement named: its path or
/// its qualified name carries the module's segments.
fn module_holds(wanted: &[&str], candidate: &Candidate) -> bool {
    if wanted.is_empty() {
        return false;
    }
    let path = path_segments(&candidate.path);
    if holds(&path, wanted) {
        return true;
    }
    let qualified: Vec<&str> = candidate
        .qualified_name
        .split(['.', ':', '>'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    holds(&qualified, wanted)
}

/// Whether `wanted` runs through `segments` in order, one after another.
fn holds(segments: &[&str], wanted: &[&str]) -> bool {
    if wanted.is_empty() || wanted.len() > segments.len() {
        return false;
    }
    segments
        .windows(wanted.len())
        .any(|window| window == wanted)
}

/// The segments a module path names, without what says only where to start:
/// `crate::m`, `./m` and `.m` are all `m`; a Dart package URI loses its
/// package name and `.dart` extension.
fn module_segments(module: &str) -> Vec<&str> {
    let module = module
        .strip_prefix("package:")
        .and_then(|module| module.split_once('/').map(|(_, path)| path))
        .unwrap_or(module);
    let module = module.strip_suffix(".dart").unwrap_or(module);
    module
        .split(['.', ':', '/'])
        .map(str::trim)
        .filter(|segment| {
            !segment.is_empty() && !matches!(*segment, "crate" | "self" | "super" | "*")
        })
        .collect()
}

/// The segments of a path, without the file extension: `pkg/m.py` is `pkg`
/// and `m`.
fn path_segments(path: &str) -> Vec<&str> {
    let path = match path.rsplit_once('.') {
        Some((stem, extension)) if !extension.contains('/') => stem,
        _ => path,
    };
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// The directory a path sits in, `""` at the repository root.
fn directory_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(directory, _)| directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: i64, blob: &str, path: &str, qualified_name: &str) -> Candidate {
        Candidate {
            id,
            blob: blob.into(),
            path: path.into(),
            language: "rust".into(),
            qualified_name: qualified_name.into(),
            start_line: 1,
        }
    }

    fn row(blob: &str, kind: InterfaceKind, name: &str, line: i64) -> InterfaceRow {
        InterfaceRow {
            blob: blob.into(),
            path: format!("{blob}.rs"),
            kind: kind.as_str().into(),
            name: name.into(),
            line,
            symbol: None,
            handlers: None,
        }
    }

    /// A dependency joins the package it names, exactly by path and as a
    /// guess by name; a route use joins the template it fits, at the handler
    /// the registration named where the ref defines it; a set variable joins
    /// every read of it. Each edge names the step that joined its two ends.
    #[test]
    fn interface_edges_join_a_dependency_a_route_use_and_a_set_variable() {
        let from = [
            row("manifest", InterfaceKind::Dependency, "api-types", 3),
            row("manifest", InterfaceKind::PathDependency, "shared", 4),
            row("client", InterfaceKind::RouteUse, "/v1/items/42", 8),
            row("client", InterfaceKind::RouteUse, "/v1/items", 9),
            row("env", InterfaceKind::EnvSet, "API_TOKEN", 1),
        ];
        let mut template = row("routes", InterfaceKind::Route, "/v1/items/{id}", 12);
        template.handlers = Some("get get_item".into());
        let mut unresolved = row("routes", InterfaceKind::Route, "/v1/items", 13);
        unresolved.symbol = Some(70);
        let to = [
            row("cargo", InterfaceKind::Package, "api-types", 2),
            row("other", InterfaceKind::Package, "shared", 1),
            template,
            unresolved,
            row("handler", InterfaceKind::EnvRead, "API_TOKEN", 20),
        ];
        let handlers = HashMap::from([(
            "get_item".to_string(),
            vec![Candidate {
                id: 42,
                blob: "handler".into(),
                path: "handler.rs".into(),
                language: "rust".into(),
                qualified_name: "get_item".into(),
                start_line: 18,
            }],
        )]);

        let edges = interface_edges(&from, &to, &handlers);
        let summary: Vec<String> = edges
            .iter()
            .map(|e| {
                format!(
                    "{} {}:{} -> {}:{}{} {} {} via {}",
                    e.kind,
                    e.from_blob,
                    e.from_line,
                    e.to_blob,
                    e.to_line,
                    e.to_symbol.map(|id| format!("#{id}")).unwrap_or_default(),
                    e.name.as_deref().unwrap_or("-"),
                    e.confidence,
                    e.step
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                "depends_on manifest:3 -> cargo:2 api-types heuristic via name",
                "depends_on manifest:4 -> other:1 shared exact via path",
                "calls_route client:8 -> handler:18#42 /v1/items/{id} heuristic via route",
                "calls_route client:9 -> routes:13#70 /v1/items exact via route",
                "sets_env env:1 -> handler:20 API_TOKEN exact via name",
            ]
        );
    }

    /// A foreign reference points at every definition of its name in the
    /// other repository, as a guess, and a name defined too often there
    /// makes none.
    #[test]
    fn a_foreign_reference_is_a_guess_at_every_definition_of_its_name() {
        let mention = |from_symbol| Mention {
            kind: "calls".into(),
            name: "Item".into(),
            line: 5,
            from_symbol,
        };
        let mentions = HashMap::from([
            (
                "Item".to_string(),
                vec![
                    ("client".to_string(), mention(Some(1))),
                    ("client".to_string(), mention(None)),
                ],
            ),
            (
                "Everywhere".to_string(),
                vec![("client".to_string(), mention(Some(1)))],
            ),
        ]);
        let defined = HashMap::from([
            (
                "Item".to_string(),
                vec![
                    candidate(10, "types", "types.rs", "Item"),
                    candidate(11, "more", "more.rs", "Item"),
                ],
            ),
            (
                "Everywhere".to_string(),
                (0..MAX_CANDIDATES as i64 + 1)
                    .map(|id| candidate(100 + id, "b", "b.rs", "Everywhere"))
                    .collect(),
            ),
        ]);

        let mut edges = foreign_edges(&mentions, &defined);
        edges.sort_by_key(|e| (e.from_symbol, e.to_symbol));
        let summary: Vec<String> = edges
            .iter()
            .map(|e| {
                format!(
                    "{:?} -> {:?} {} via {}, {}",
                    e.from_symbol, e.to_symbol, e.confidence, e.step, e.candidates
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                "None -> Some(10) heuristic via name, 2",
                "None -> Some(11) heuristic via name, 2",
                "Some(1) -> Some(10) heuristic via name, 2",
                "Some(1) -> Some(11) heuristic via name, 2",
            ]
        );
        assert!(edges.iter().all(|e| e.kind == "references"));
        assert!(
            edges.iter().all(|e| e.name.as_deref() == Some("Item")),
            "a name past the cap makes no edge"
        );
    }

    #[test]
    fn a_foreign_reference_skips_outline_definitions() {
        let mentions = HashMap::from([(
            "Value".to_string(),
            vec![(
                "caller".to_string(),
                Mention {
                    kind: "references".into(),
                    name: "Value".into(),
                    line: 1,
                    from_symbol: Some(1),
                },
            )],
        )]);
        let mut outline = candidate(2, "config", "config.yml", "Value");
        outline.language = "yaml".into();
        let defined = HashMap::from([("Value".to_string(), vec![outline])]);

        assert!(foreign_edges(&mentions, &defined).is_empty());
    }

    #[test]
    fn a_route_handler_skips_outline_definitions() {
        let from = [row("client", InterfaceKind::RouteUse, "/items", 1)];
        let mut route = row("routes", InterfaceKind::Route, "/items", 2);
        route.handlers = Some("handle".into());
        let mut outline = candidate(2, "config", "routes.yml", "handle");
        outline.language = "yaml".into();
        let handlers = HashMap::from([(
            "handle".to_string(),
            vec![outline, candidate(3, "handler", "handler.rs", "handle")],
        )]);

        let edges = interface_edges(&from, &[route], &handlers);
        assert_eq!(edges.len(), 1, "{edges:#?}");
        assert_eq!(edges[0].to_symbol, Some(3));
    }

    /// Two files that import the same name each get their own edge. An
    /// import sits at file scope, so its edges are told apart by the blob
    /// they come from and by nothing else.
    #[test]
    fn two_files_that_import_the_same_name_each_get_an_edge() {
        let importer = |module: &str| Names {
            path: "caller.rs".into(),
            mentions: Vec::new(),
            imports: vec![Import {
                module: module.into(),
                name: Some("b".into()),
                line: 1,
            }],
        };
        let named = HashMap::from([
            ("first".to_string(), importer("inner::m")),
            ("second".to_string(), importer("inner::m")),
        ]);
        let candidates = HashMap::from([(
            "b".to_string(),
            vec![candidate(7, "target", "inner/m.rs", "b")],
        )]);

        let edges = edges_of(&named, &candidates);
        let mut from: Vec<&str> = edges
            .iter()
            .map(|edge| {
                assert_eq!(edge.kind, "imports");
                assert_eq!(edge.to_symbol, Some(7));
                assert_eq!(edge.from_symbol, None, "an import sits at file scope");
                edge.from_blob.as_str()
            })
            .collect();
        from.sort_unstable();
        assert_eq!(from, ["first", "second"]);
    }

    /// The four steps, in order: the same file beats the same directory,
    /// which beats an import, which beats anywhere in the repository. Each
    /// answer names the step that gave it.
    #[test]
    fn a_name_resolves_at_the_nearest_step_that_holds_a_definition() {
        let matched = [
            candidate(1, "here", "src/a.rs", "b"),
            candidate(2, "beside", "src/c.rs", "b"),
            candidate(3, "far", "other/m.rs", "b"),
            candidate(4, "stranger", "deep/down/x.rs", "b"),
        ];
        let imports = [module_segments("other::m")];
        let elsewhere = [module_segments("nowhere")];

        let resolved = narrow(&matched, "here", "src/a.rs", &imports).unwrap();
        assert_eq!(resolved.candidates[0].id, 1, "the same file first");
        assert_eq!(resolved.step, "file");
        assert_eq!(resolved.confidence, "exact");

        let resolved = narrow(&matched, "nothing", "src/a.rs", &imports).unwrap();
        let ids: Vec<i64> = resolved.candidates.iter().map(|c| c.id).collect();
        assert_eq!(ids, [1, 2], "then the same directory");
        assert_eq!(resolved.step, "directory");
        assert_eq!(
            resolved.confidence, "heuristic",
            "two definitions is a guess"
        );

        let resolved = narrow(&matched, "nothing", "far/away.rs", &imports).unwrap();
        assert_eq!(
            resolved.candidates.iter().map(|c| c.id).collect::<Vec<_>>(),
            [3],
            "then what the file imports"
        );
        assert_eq!(resolved.step, "import");

        let resolved = narrow(&matched, "nothing", "far/away.rs", &elsewhere).unwrap();
        assert_eq!(
            resolved.candidates.len(),
            4,
            "then anywhere in the repository"
        );
        assert_eq!(resolved.step, "repository");
    }

    /// A name that matches more definitions than the cap says nothing about
    /// which one was meant, and is left unresolved.
    #[test]
    fn a_name_past_the_candidate_cap_is_left_unresolved() {
        let matched: Vec<Candidate> = (0..MAX_CANDIDATES as i64 + 1)
            .map(|id| candidate(id, "blob", &format!("src/{id}/new.rs"), "new"))
            .collect();
        assert!(narrow(&matched, "nothing", "far/away.rs", &[]).is_none());
    }

    #[test]
    fn a_module_is_matched_by_the_path_or_the_qualified_name_it_names() {
        let holds =
            |module: &str, candidate: &Candidate| module_holds(&module_segments(module), candidate);
        let rust = candidate(1, "b", "src/m.rs", "b");
        assert!(holds("crate::m", &rust));
        assert!(holds("m", &rust));
        assert!(!holds("other", &rust));

        let python = candidate(2, "b", "pkg/m.py", "x");
        assert!(holds("pkg.m", &python));

        let typescript = candidate(3, "b", "src/m/index.ts", "x");
        assert!(holds("./m", &typescript));

        let csharp = candidate(4, "b", "Lib/Helper.cs", "Lib.Helper.Run");
        assert!(holds("Lib", &csharp), "a namespace, not a path");

        let dart = candidate(5, "b", "lib/src/x.dart", "dartTarget");
        assert!(holds("package:app/src/x.dart", &dart));
        assert!(holds("../src/x.dart", &dart));
    }
}
