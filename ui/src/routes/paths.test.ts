import { describe, expect, it } from "vitest"

import { taskPanelTo } from "./paths"

describe("taskPanelTo", () => {
  it("keeps the params the screen owns", () => {
    const to = taskPanelTo(new URLSearchParams("goal=g1&status=running"), "t1")
    const params = new URLSearchParams(to.search)
    expect(params.get("goal")).toBe("g1")
    expect(params.get("status")).toBe("running")
    expect(params.get("task")).toBe("t1")
  })

  it("drops the params the panel it replaces owned", () => {
    const to = taskPanelTo(new URLSearchParams("task=t1&tab=diff&session=s1"), "t2")
    const params = new URLSearchParams(to.search)
    expect(params.get("task")).toBe("t2")
    expect(params.has("tab")).toBe(false)
    expect(params.has("session")).toBe(false)
  })
})
