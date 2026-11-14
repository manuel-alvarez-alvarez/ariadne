import { expect, it } from "vitest"

import { highlight, knownLanguage, languageForPath } from "./highlight"

it("names the kind of each comment, string, number and keyword, and loses no text", () => {
  const code = `const n = 42 // answer\nlet s = "a // not a comment"`
  const tokens = highlight(code, knownLanguage("ts"))

  expect(tokens.map((token) => token.text).join("")).toBe(code)
  expect(tokens.filter((token) => token.kind).map((token) => [token.kind, token.text])).toEqual([
    ["keyword", "const"],
    ["number", "42"],
    ["comment", "// answer"],
    ["keyword", "let"],
    ["string", `"a // not a comment"`],
  ])
})

it("returns an unknown language as one plain token", () => {
  expect(highlight("fn main() {}", knownLanguage("mystery"))).toEqual([{ text: "fn main() {}" }])
})

it("reads a file's language from its extension", () => {
  expect(languageForPath("src/app.tsx")).toBe("typescript")
  expect(languageForPath("README")).toBeUndefined()
})
