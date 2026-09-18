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
//! The first of those that holds a definition is the one that answers. One
//! definition there is `exact`; several are `heuristic`, the count is kept,
//! and an edge is written to each — an author asking who calls a name is
//! better served by a list that holds the answer than by a guess.

use std::collections::{HashMap, HashSet};

use crate::parser::{EdgeKind, Import};

/// How many definitions a name may match and still be resolved. Past this,
/// the name says nothing about which definition was meant — `new` in a large
/// repository — and the edges would be noise measured in thousands.
pub const MAX_CANDIDATES: usize = 20;

/// One definition a name could mean, at one ref of one repository.
#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub id: i64,
    pub blob: String,
    pub path: String,
    pub qualified_name: String,
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

/// One row of `edges`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub kind: String,
    pub from_blob: String,
    pub from_symbol: Option<i64>,
    pub to_symbol: i64,
    pub from_line: i64,
    pub confidence: &'static str,
    pub candidates: i64,
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
        let mut narrowed: HashMap<String, Option<(Vec<&Candidate>, &'static str)>> = HashMap::new();
        // One edge per pair, within this blob. Per blob and not per run: a
        // mention at file scope has no definition to be from, so two files
        // that import the same name would share a key across the run and only
        // the first would get its edge.
        let mut seen: HashSet<(String, Option<i64>, i64)> = HashSet::new();
        for mention in names.mentions.iter().cloned().chain(imported) {
            let step = narrowed.entry(mention.name.clone()).or_insert_with(|| {
                let matched = candidates.get(&mention.name)?;
                narrow(matched, blob, &names.path, &modules)
            });
            let Some((step, confidence)) = step else {
                continue;
            };
            for candidate in step.iter() {
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
                    to_symbol: candidate.id,
                    from_line: mention.line,
                    confidence,
                    candidates: step.len() as i64,
                });
            }
        }
    }
    edges
}

/// The definitions a name resolves to, and how sure that is: the nearest
/// step that holds any, unless it holds more than [`MAX_CANDIDATES`].
fn narrow<'a>(
    matched: &'a [Candidate],
    blob: &str,
    path: &str,
    modules: &[Vec<&str>],
) -> Option<(Vec<&'a Candidate>, &'static str)> {
    let directory = directory_of(path);
    let held = |step: Vec<&'a Candidate>| (!step.is_empty()).then_some(step);
    let step = held(matched.iter().filter(|c| c.blob == blob).collect())
        // The same module or directory.
        .or_else(|| {
            held(
                matched
                    .iter()
                    .filter(|c| directory_of(&c.path) == directory)
                    .collect(),
            )
        })
        // What the file imports.
        .or_else(|| {
            held(
                matched
                    .iter()
                    .filter(|c| modules.iter().any(|module| module_holds(module, c)))
                    .collect(),
            )
        })
        // Anywhere in the repository.
        .or_else(|| held(matched.iter().collect()))?;
    if step.len() > MAX_CANDIDATES {
        return None;
    }
    let confidence = match step.len() {
        1 => "exact",
        _ => "heuristic",
    };
    Some((step, confidence))
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
/// `crate::m` and `./m` and `.m` are all `m`.
fn module_segments(module: &str) -> Vec<&str> {
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
            qualified_name: qualified_name.into(),
        }
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
                assert_eq!(edge.to_symbol, 7);
                assert_eq!(edge.from_symbol, None, "an import sits at file scope");
                edge.from_blob.as_str()
            })
            .collect();
        from.sort_unstable();
        assert_eq!(from, ["first", "second"]);
    }

    /// The four steps, in order: the same file beats the same directory,
    /// which beats an import, which beats anywhere in the repository.
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

        let (step, confidence) = narrow(&matched, "here", "src/a.rs", &imports).unwrap();
        assert_eq!(step[0].id, 1, "the same file first");
        assert_eq!(confidence, "exact");

        let (step, confidence) = narrow(&matched, "nothing", "src/a.rs", &imports).unwrap();
        let ids: Vec<i64> = step.iter().map(|c| c.id).collect();
        assert_eq!(ids, [1, 2], "then the same directory");
        assert_eq!(confidence, "heuristic", "two definitions is a guess");

        let (step, _) = narrow(&matched, "nothing", "far/away.rs", &imports).unwrap();
        assert_eq!(
            step.iter().map(|c| c.id).collect::<Vec<_>>(),
            [3],
            "then what the file imports"
        );

        let (step, _) = narrow(&matched, "nothing", "far/away.rs", &elsewhere).unwrap();
        assert_eq!(step.len(), 4, "then anywhere in the repository");
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
    }
}
