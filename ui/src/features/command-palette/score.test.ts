import { describe, expect, it } from "vitest"

import { preferLiteralMatches } from "./score"

/** Stands in for cmdk's subsequence scorer: every letter, in order, anywhere. */
const fuzzy = (value: string, search: string) => {
  let at = -1
  for (const letter of search.toLowerCase().replace(/\s/g, "")) {
    at = value.toLowerCase().indexOf(letter, at + 1)
    if (at === -1) return 0
  }
  return 0.4
}

const score = preferLiteralMatches(fuzzy)

describe("preferLiteralMatches", () => {
  it("puts a row the query names above one that only spells it out", () => {
    // The row that spells the query out is a fuzzy hit at its best — `() => 1`
    // — and still loses to the row that is called that.
    const ranked = preferLiteralMatches(() => 1)
    expect(ranked("Orchestrator · UX updates …0000PLAN", "orchestrator ux")).toBeGreaterThan(
      ranked("Keyboard support: command palette", "orchestrator ux"),
    )
  })

  it("ranks a prefix over a word start over a match in the middle", () => {
    expect(score("Reviewer (strict)", "review")).toBeGreaterThan(score("Deep reviewer", "review"))
    expect(score("Deep reviewer", "review")).toBeGreaterThan(score("Prereviewer", "review"))
  })

  it("treats the separators the palette writes rows with as word starts", () => {
    expect(score("Author · Keyboard support", "keyboard")).toBe(
      score("Author Keyboard support", "keyboard"),
    )
    expect(score("worktrees/eng-keyboard", "eng")).toBe(score("worktrees eng keyboard", "eng"))
  })

  it("finds a row by a keyword — its full id — under everything it is named by", () => {
    const byId = score("Keyboard support keyboard-support-key", "01jtask0000000000000000key", [
      "01JTASK0000000000000000KEY",
    ])
    const byName = score("Keyboard support keyboard-support-key", "keyboard")
    expect(byId).toBeGreaterThan(0)
    expect(byName).toBeGreaterThan(byId)
  })

  it("never scores the keywords fuzzily, which is what keeps the ids quiet", () => {
    // Every letter of "orchestrator", in order, scattered through the id — and
    // nowhere in the row's own text.
    expect(score("Keyboard support", "orchestrator", ["01JP7L4A2N9NZE5R"])).toBe(0)
  })

  it("wants every word of the query, not just one of them", () => {
    expect(score("Orchestrator · UX updates", "orchestrator ux")).toBeGreaterThan(
      score("Orchestrator · Documentation pass", "orchestrator ux"),
    )
  })

  it("keeps hiding what the fuzzy scorer rejects", () => {
    expect(score("Documentation pass", "zzz")).toBe(0)
  })

  it("drops a fuzzy hit that is only an accident of a long row", () => {
    const barelyMatched = preferLiteralMatches(() => 0.002)
    expect(barelyMatched("Keyboard support: command palette", "orchestrator")).toBe(0)
  })

  it("keeps the fuzzy order among the fuzzy hits it does keep", () => {
    const ranked = preferLiteralMatches((value) => (value === "closer" ? 0.8 : 0.2))
    expect(ranked("closer", "xyz")).toBeGreaterThan(ranked("further", "xyz"))
  })

  it("leaves an empty query to the fuzzy scorer, which lists everything", () => {
    expect(score("Anything", "")).toBe(fuzzy("Anything", ""))
    expect(score("Anything", "   ")).toBe(fuzzy("Anything", "   "))
  })
})
