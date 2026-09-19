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
pub const TOKEN_CHARS: usize = 4;

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

/// The map as the text an agent reads: a file per heading, its definitions
/// under it, and nothing past `budget` tokens.
pub(crate) fn render(files: &[MapFile], budget: usize) -> RepoMap {
    let cap = budget.saturating_mul(TOKEN_CHARS);
    let mut map = RepoMap::default();
    for file in files {
        let heading = format!("{}\n", file.path);
        if map.text.len() + heading.len() > cap {
            break;
        }
        map.text.push_str(&heading);
        map.files += 1;
        for symbol in &file.symbols {
            let line = format!(
                "  {}-{} {} {} {}\n",
                symbol.start_line, symbol.end_line, symbol.kind, symbol.name, symbol.signature
            );
            if map.text.len() + line.len() > cap {
                return map;
            }
            map.text.push_str(&line);
        }
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

        let cut = render(&files, 8);
        assert!(cut.tokens() <= 8, "{} tokens: {}", cut.tokens(), cut.text);
        assert!(
            cut.text.lines().all(|line| !line.is_empty()),
            "{}",
            cut.text
        );
    }
}
