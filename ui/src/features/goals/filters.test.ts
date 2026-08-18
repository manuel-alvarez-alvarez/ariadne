import { describe, expect, it } from "vitest"

import { ALL, readStatusFilter, withStatusFilter } from "./filters"

describe("readStatusFilter", () => {
  it("reads a status the daemon defines", () => {
    expect(readStatusFilter(new URLSearchParams("status=planning"))).toBe("planning")
  })

  it("falls back to no filter for a missing or unknown status", () => {
    expect(readStatusFilter(new URLSearchParams(""))).toBe(ALL)
    expect(readStatusFilter(new URLSearchParams("status=nonsense"))).toBe(ALL)
    expect(readStatusFilter(new URLSearchParams("status="))).toBe(ALL)
  })
})

describe("withStatusFilter", () => {
  it("keeps the params the panels own", () => {
    const next = withStatusFilter(new URLSearchParams("goal=g1&task=t1"), "active")
    expect(next.get("goal")).toBe("g1")
    expect(next.get("task")).toBe("t1")
    expect(next.get("status")).toBe("active")
  })

  it("drops the param when the filter is off", () => {
    const next = withStatusFilter(new URLSearchParams("status=active&goal=g1"), ALL)
    expect(next.has("status")).toBe(false)
    expect(next.get("goal")).toBe("g1")
  })

  it("round-trips through the URL", () => {
    const next = withStatusFilter(new URLSearchParams(""), "completed")
    expect(readStatusFilter(next)).toBe("completed")
  })
})
