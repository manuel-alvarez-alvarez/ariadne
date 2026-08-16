/**
 * How the palette decides what a query matched, and how well.
 *
 * cmdk scores a row by subsequence: every letter of the query, in order,
 * anywhere in the row. That is what makes `kybrd` find "Keyboard support" and
 * is worth keeping — but the rows here carry 26-character ulids and branch
 * names, and a long enough string of random letters answers to almost anything.
 * `planner` finding a task called "Keyboard support" (through its id) is the
 * everyday version of that.
 *
 * So the matching happens in three tiers, strongest first:
 *
 * 1. every word of the query is *in* the row's own text — ranked by how much of
 *    a name it turned out to be: the whole start of it, a word inside it, or
 *    letters in the middle;
 * 2. a word is in what the row is searchable *by* rather than named by (its
 *    full id, its status): the ids are matched here, literally, which is what
 *    pasting one is;
 * 3. otherwise cmdk's fuzzy score over the row's text alone — never the
 *    keywords, so the ids stay out of it — pushed under both, and rounded down
 *    to nothing when it is the accidental kind.
 */

/** cmdk's `filter`: `0` hides the row, and bigger is higher up the list. */
export type PaletteScore = (value: string, search: string, keywords?: string[]) => number

/** What one word of the query is worth, by where it was found. */
const EXACT = 1
const WORD_START = 0.9
const ANYWHERE = 0.8
const IN_KEYWORDS = 0.75

/**
 * How far a fuzzy hit is pushed under the literal ones. It scales rather than
 * flattens, so cmdk's ordering of the fuzzy hits among themselves survives.
 */
const FUZZY_WEIGHT = 0.5

/**
 * Under this, a fuzzy hit is not a hit. cmdk's score falls off sharply with the
 * distance between the letters it matched, so a real abbreviation of a name
 * lands two orders of magnitude above the letters it happened to find scattered
 * across a row — `kybrd` scores ~0.03 against "Keyboard support", where
 * `planner` scores ~0.002 against the same row's id.
 */
const FUZZY_FLOOR = 0.01

/** Wraps a fuzzy scorer — cmdk's `defaultFilter` — with the tiers above. */
export function preferLiteralMatches(fuzzy: PaletteScore): PaletteScore {
  return (value, search, keywords) => {
    const terms = search.toLowerCase().split(/\s+/).filter(Boolean)
    if (terms.length === 0) return fuzzy(value, search)

    const text = value.toLowerCase()
    const searchableBy = (keywords ?? []).join(" ").toLowerCase()

    let total = 0
    for (const term of terms) {
      const at = text.indexOf(term)
      if (at === 0) total += EXACT
      else if (at > 0) total += isWordStart(value, at) ? WORD_START : ANYWHERE
      else if (searchableBy.includes(term)) total += IN_KEYWORDS
      // One word unaccounted for and the row is a fuzzy hit at best:
      // `planner ux` is not asking for every row with a planner in it.
      else return fuzzyScore(fuzzy, value, search)
    }
    return total / terms.length
  }
}

/**
 * The best score any of these rows gets for this query, `0` when none of them
 * match — what the palette orders its groups by.
 *
 * cmdk sorts the rows *inside* a group and leaves the groups themselves in the
 * order they were written, so a group with one strong match would otherwise sit
 * under a group with three weak ones — and the row the palette pre-selects
 * would be the wrong one.
 */
export function bestScore(
  score: PaletteScore,
  rows: readonly { value: string; keywords: string[] }[],
  search: string,
): number {
  let best = 0
  for (const row of rows) best = Math.max(best, score(row.value, search, row.keywords))
  return best
}

function fuzzyScore(fuzzy: PaletteScore, value: string, search: string): number {
  const score = fuzzy(value, search)
  return score < FUZZY_FLOOR ? 0 : score * FUZZY_WEIGHT
}

/** Whether the match starts a word — "review" in "Reviewer (strict)". */
function isWordStart(value: string, at: number): boolean {
  return /[\s\-_/·(]/.test(value[at - 1] ?? "")
}
