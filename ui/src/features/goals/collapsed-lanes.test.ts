import { describe, expect, it } from "vitest"

import { parseCollapsed, serializeCollapsed, toggleCollapsed } from "./collapsed-lanes"

describe("parseCollapsed", () => {
  it("reads back what was stored", () => {
    expect(parseCollapsed(serializeCollapsed(new Set(["g2", "g1"])))).toEqual(new Set(["g1", "g2"]))
  })

  it("reads anything that is not a list of ids as none", () => {
    expect(parseCollapsed(null)).toEqual(new Set())
    expect(parseCollapsed("")).toEqual(new Set())
    expect(parseCollapsed("{oops")).toEqual(new Set())
    expect(parseCollapsed('{"g1":true}')).toEqual(new Set())
  })

  it("drops entries that are not ids", () => {
    expect(parseCollapsed('["g1",7,null]')).toEqual(new Set(["g1"]))
  })
})

describe("toggleCollapsed", () => {
  it("collapses a lane and expands it again", () => {
    const collapsed = toggleCollapsed(new Set(), "g1")
    expect(collapsed.has("g1")).toBe(true)
    expect(toggleCollapsed(collapsed, "g1").has("g1")).toBe(false)
  })

  it("leaves the other lanes alone and does not mutate the input", () => {
    const before = new Set(["g1"])
    const after = toggleCollapsed(before, "g2")
    expect(after).toEqual(new Set(["g1", "g2"]))
    expect(before).toEqual(new Set(["g1"]))
  })
})
