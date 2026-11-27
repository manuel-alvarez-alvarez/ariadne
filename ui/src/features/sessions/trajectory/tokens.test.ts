/**
 * The turn console draws with the app's tokens alone, so it follows the
 * light and the dark theme: no source of it names a fixed colour.
 */

import { readdirSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

import { expect, it } from "vitest"

const folder = fileURLToPath(new URL(".", import.meta.url))

/** Tailwind's own palette, which does not change with the theme. */
const PALETTE =
  /\b(?:bg|text|border|ring|fill|stroke|outline|divide|from|via|to|shadow|decoration|accent|caret)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white)(?:-\d+)?\b/g

/** A colour written out: a hex value, or a colour function. */
const LITERAL = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\(/g

it("names no fixed colour in any source of the turn console", () => {
  const sources = readdirSync(folder).filter(
    (name) => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name),
  )
  expect(sources.length).toBeGreaterThan(0)
  const found = sources.flatMap((name) => {
    const source = readFileSync(`${folder}${name}`, "utf8")
    return [...(source.match(PALETTE) ?? []), ...(source.match(LITERAL) ?? [])].map(
      (hit) => `${name}: ${hit}`,
    )
  })
  expect(found).toEqual([])
})
