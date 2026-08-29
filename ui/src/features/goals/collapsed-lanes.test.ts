import { describe, expect, it } from "vitest"

import {
  isLaneCollapsed,
  type LaneCollapse,
  parseCollapsed,
  serializeCollapsed,
  setLaneCollapsed,
} from "./collapsed-lanes"

const NOTHING: LaneCollapse = { collapsed: new Set(), expanded: new Set() }

describe("parseCollapsed", () => {
  it("reads back what was stored", () => {
    const state = setLaneCollapsed(setLaneCollapsed(NOTHING, "g2", true), "g1", false)
    expect(parseCollapsed(serializeCollapsed(state))).toEqual({
      collapsed: new Set(["g2"]),
      expanded: new Set(["g1"]),
    })
  })

  it("reads the older id-list form as lanes that were folded away", () => {
    // What a board written by a previous version left in `localStorage`: a
    // plain list, which meant exactly "these were collapsed".
    expect(parseCollapsed('["g1","g2"]')).toEqual({
      collapsed: new Set(["g1", "g2"]),
      expanded: new Set(),
    })
  })

  it("reads anything that is not one of the two shapes as nothing said", () => {
    expect(parseCollapsed(null)).toEqual(NOTHING)
    expect(parseCollapsed("")).toEqual(NOTHING)
    expect(parseCollapsed("{oops")).toEqual(NOTHING)
    expect(parseCollapsed('"g1"')).toEqual(NOTHING)
  })

  it("drops entries that are not ids", () => {
    expect(parseCollapsed('{"collapsed":["g1",7,null],"expanded":"g2"}')).toEqual({
      collapsed: new Set(["g1"]),
      expanded: new Set(),
    })
  })
})

describe("setLaneCollapsed", () => {
  it("folds a lane away and opens it again", () => {
    const collapsed = setLaneCollapsed(NOTHING, "g1", true)
    expect(isLaneCollapsed(collapsed, "g1", false)).toBe(true)
    expect(isLaneCollapsed(setLaneCollapsed(collapsed, "g1", false), "g1", false)).toBe(false)
  })

  it("leaves the other lanes alone and does not mutate the input", () => {
    const before = setLaneCollapsed(NOTHING, "g1", true)
    const after = setLaneCollapsed(before, "g2", true)
    expect(after.collapsed).toEqual(new Set(["g1", "g2"]))
    expect(before.collapsed).toEqual(new Set(["g1"]))
  })
})

describe("isLaneCollapsed", () => {
  it("takes the board's default for a lane nobody has touched", () => {
    expect(isLaneCollapsed(NOTHING, "g1", true)).toBe(true)
    expect(isLaneCollapsed(NOTHING, "g1", false)).toBe(false)
  })

  it("remembers a lane opened against the default, so it stays open", () => {
    // The whole reason the state is two sets: a finished lane the user
    // expanded would otherwise fold itself away again on the next render.
    const state = setLaneCollapsed(NOTHING, "finished", false)
    expect(isLaneCollapsed(state, "finished", true)).toBe(false)
  })

  it("remembers a lane folded away against the default", () => {
    const state = setLaneCollapsed(NOTHING, "active", true)
    expect(isLaneCollapsed(state, "active", false)).toBe(true)
  })
})
