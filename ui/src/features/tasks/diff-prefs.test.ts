import { describe, expect, it } from "vitest"

import { parseWrap, serializeWrap } from "./diff-prefs"

describe("parseWrap", () => {
  it("reads back what was stored", () => {
    expect(parseWrap(serializeWrap(true))).toBe(true)
    expect(parseWrap(serializeWrap(false))).toBe(false)
  })

  it("defaults to wrapping when nothing sensible is stored", () => {
    expect(parseWrap(null)).toBe(true)
    expect(parseWrap("")).toBe(true)
    expect(parseWrap("nonsense")).toBe(true)
  })
})
