//! The repo map: which files of a ref carry the repository, and the
//! definitions worth naming in each.
//!
//! Aider ranks a repository by PageRank over the graph its files make, and
//! hands the top of that ranking to the model under a token budget. The
//! ranking is the same here, over the symbol edges the resolution pass
//! derived (022, rule 9): a file that many files name collects their rank,
//! and a file nothing names keeps its own. The interface edges of the link
//! pass are left out, which `store::SYMBOL_KINDS` says why.
//!
//! A map is a tool an agent calls, never text a prompt carries: an
//! architecture overview injected into every prompt costs tokens and
//! answers no question the agent asked.

use crate::store::OutlineEntry;

/// How much of a file's rank flows on along its edges. The rest restarts at
/// the personalization, which is the whole repository, or the one file the
/// caller named.
const DAMPING: f64 = 0.85;

/// How many rounds the power iteration runs. The ranking is read for its
/// order alone, which settles long before the values do.
const ROUNDS: usize = 30;

/// What a reference counts for against its own direction. A reference points
/// from the file that names a definition to the file that holds it, so rank
/// flows to what the repository depends on. The back edge is kept small and
/// not dropped: a map personalized toward one file names the files that call
/// it as well as the files it calls.
const BACK_WEIGHT: f64 = 0.25;

/// How many characters one token is counted as.
const TOKEN_CHARS: usize = 4;

/// One file of a map: where it is, how central it is, and the definitions
/// the map names in it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapFile {
    pub path: String,
    pub rank: f64,
    pub symbols: Vec<OutlineEntry>,
}

/// One repo map, as `GET /v1/knowledge/map` answers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoMap {
    /// The map itself, plain text and under the budget.
    pub text: String,
    /// How many files the text names.
    pub files: i64,
    /// The ranked files the text did not hold, 0 where the budget held
    /// every one of them.
    pub files_left: i64,
}

impl RepoMap {
    /// How many tokens the text is: its characters divided by
    /// [`TOKEN_CHARS`].
    pub fn tokens(&self) -> i64 {
        (self.text.len() / TOKEN_CHARS) as i64
    }
}

/// The PageRank of `nodes` files over `edges`, each edge a from index, a to
/// index and how many references it joins by.
///
/// `toward` restarts every walk at one file instead of at the repository, so
/// the files that file joins to come out first.
pub(crate) fn page_rank(
    nodes: usize,
    edges: &[(usize, usize, f64)],
    toward: Option<usize>,
) -> Vec<f64> {
    if nodes == 0 {
        return Vec::new();
    }
    let restart: Vec<f64> = match toward.filter(|at| *at < nodes) {
        Some(at) => (0..nodes).map(|i| f64::from(u8::from(i == at))).collect(),
        None => vec![1.0 / nodes as f64; nodes],
    };
    // What each file hands on, which is what its own rank is divided by.
    let mut out = vec![0.0; nodes];
    for (from, _, weight) in edges {
        out[*from] += weight;
    }
    let dangling: Vec<usize> = (0..nodes).filter(|at| out[*at] == 0.0).collect();
    let mut rank = restart.clone();
    for _ in 0..ROUNDS {
        let mut next = vec![0.0; nodes];
        for (from, to, weight) in edges {
            next[*to] += DAMPING * rank[*from] * weight / out[*from];
        }
        // A file that names nothing hands its rank back to the restart,
        // rather than losing it out of the graph.
        let lost: f64 = dangling.iter().map(|at| rank[*at]).sum();
        for (at, share) in restart.iter().enumerate() {
            next[at] += (1.0 - DAMPING + DAMPING * lost) * share;
        }
        rank = next;
    }
    rank
}

/// Both directions of one reference between two files: the way it points,
/// and the way back at [`BACK_WEIGHT`] of it.
pub(crate) fn both_ways(from: usize, to: usize, count: f64) -> [(usize, usize, f64); 2] {
    [(from, to, count), (to, from, count * BACK_WEIGHT)]
}

/// Room kept for the files-left line, whatever number it carries.
const TAIL: usize = 64;

/// The map as the text an agent reads: a file per heading, its definitions
/// under it, and nothing past `budget` tokens. Where the budget cuts the
/// text, the last line says how many ranked files were left out, counted
/// against the budget like every other line.
pub(crate) fn render(files: &[MapFile], budget: usize) -> RepoMap {
    let cap = budget.saturating_mul(TOKEN_CHARS);
    let room = cap.saturating_sub(TAIL);
    let mut map = RepoMap::default();
    for (at, file) in files.iter().enumerate() {
        let heading = format!("{}\n", file.path);
        if map.text.len() + heading.len() > room {
            return cut(map, files.len() - at, cap);
        }
        map.text.push_str(&heading);
        map.files += 1;
        for symbol in &file.symbols {
            let line = format!(
                "  {}-{} {} {} {}\n",
                symbol.start_line, symbol.end_line, symbol.kind, symbol.name, symbol.signature
            );
            if map.text.len() + line.len() > room {
                return cut(map, files.len() - at - 1, cap);
            }
            map.text.push_str(&line);
        }
    }
    map
}

/// `map`, ended with the line that says how many ranked files `files_left`
/// leaves out — only where that whole line still fits under `cap`, the same
/// rule every other line of the map keeps.
fn cut(mut map: RepoMap, files_left: usize, cap: usize) -> RepoMap {
    map.files_left = files_left as i64;
    let tail = format!(
        "{} files left. Raise the budget or name a path.\n",
        map.files_left
    );
    if map.text.len() + tail.len() <= cap {
        map.text.push_str(&tail);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> OutlineEntry {
        OutlineEntry {
            kind: "function".into(),
            name: name.into(),
            start_line: 1,
            end_line: 3,
            signature: format!("pub fn {name}()"),
        }
    }

    fn file(path: &str, symbols: &[&str]) -> MapFile {
        MapFile {
            path: path.into(),
            rank: 1.0,
            symbols: symbols.iter().copied().map(entry).collect(),
        }
    }

    /// The file everything names collects the rank, and the file that names
    /// it keeps less.
    #[test]
    fn the_file_every_other_file_names_ranks_first() {
        let edges: Vec<(usize, usize, f64)> = [(1, 0), (2, 0), (3, 0)]
            .into_iter()
            .flat_map(|(from, to)| both_ways(from, to, 1.0))
            .collect();
        let rank = page_rank(4, &edges, None);
        assert!(rank[0] > rank[1], "{rank:?}");
        assert!(rank[0] > rank[3], "{rank:?}");
    }

    /// Personalized toward one file, the files it joins to come out above a
    /// file it reaches nowhere.
    #[test]
    fn a_map_toward_one_file_ranks_what_that_file_joins_to() {
        // 0 names 1, 2 names 0, and 3 stands apart.
        let edges: Vec<(usize, usize, f64)> = [(0, 1), (2, 0)]
            .into_iter()
            .flat_map(|(from, to)| both_ways(from, to, 1.0))
            .collect();
        let rank = page_rank(4, &edges, Some(0));
        assert!(rank[1] > rank[3], "the callee of the file: {rank:?}");
        assert!(rank[2] > rank[3], "the caller of the file: {rank:?}");
    }

    /// The text stops at the budget, whole lines only.
    #[test]
    fn the_text_stops_at_the_budget() {
        let files = [file("a.rs", &["one", "two"]), file("b.rs", &["three"])];
        let whole = render(&files, 1000);
        assert_eq!(whole.files, 2);
        assert!(whole.text.ends_with("pub fn three()\n"), "{}", whole.text);
    }

    /// A budget that holds every ranked file whole says nothing about what
    /// it left out. A budget that cuts the text ends it with a line naming
    /// the ranked files the cut left out, and `files_left` counts them.
    #[test]
    fn the_text_names_the_files_the_budget_left_out() {
        let files = [file("a.rs", &["one", "two"]), file("b.rs", &["three"])];

        let whole = render(&files, 1000);
        assert_eq!(whole.files_left, 0);
        assert!(!whole.text.contains("files left"), "{}", whole.text);

        // `a.rs` and its two definitions fit; `b.rs` does not, and the
        // budget holds only the line that says so.
        let cut = render(&files, 34);
        assert_eq!(cut.files, 1, "{}", cut.text);
        assert_eq!(cut.files_left, 1, "{}", cut.text);
        assert_eq!(
            cut.text,
            "a.rs\n\
             \x20 1-3 function one pub fn one()\n\
             \x20 1-3 function two pub fn two()\n\
             1 files left. Raise the budget or name a path.\n"
        );
    }

    /// The files-left line only writes whole, the same rule every other
    /// line keeps: below its own length the budget holds none of it, but
    /// `files_left` still counts every ranked file the cut left out.
    #[test]
    fn the_files_left_line_never_passes_its_own_budget() {
        let files = [file("a.rs", &["one", "two"]), file("b.rs", &["three"])];

        // 44 bytes: shorter than the 47-byte line a count of two needs.
        let too_small = render(&files, 11);
        assert_eq!(too_small.text, "");
        assert_eq!(too_small.files_left, 2);
        assert!(too_small.tokens() <= 11, "{}", too_small.text);

        // 48 bytes: just enough to hold that same line whole.
        let just_enough = render(&files, 12);
        assert_eq!(
            just_enough.text,
            "2 files left. Raise the budget or name a path.\n"
        );
        assert_eq!(just_enough.files_left, 2);
        assert!(just_enough.tokens() <= 12, "{}", just_enough.text);
    }
}
