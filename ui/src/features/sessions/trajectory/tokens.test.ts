/**
 * The turn console draws with the app's tokens alone, so it follows the
 * light and the dark theme: no source of it names a fixed colour. The
 * console test checks what it draws, shared parts included, in both themes.
 */

import { readdirSync, readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

import { expect, it } from "vitest"

import { fixedColours } from "@/test/colours"

const folder = fileURLToPath(new URL(".", import.meta.url))

it("names no fixed colour in any source of the turn console", () => {
  const sources = readdirSync(folder).filter(
    (name) => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name),
  )
  expect(sources.length).toBeGreaterThan(0)
  const found = sources.flatMap((name) =>
    fixedColours(readFileSync(`${folder}${name}`, "utf8")).map((hit) => `${name}: ${hit}`),
  )
  expect(found).toEqual([])
})
