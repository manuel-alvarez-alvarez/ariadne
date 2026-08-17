import { describe, expect, it } from "vitest"

import { panelSessionTo, sessionPanelTo, taskPanelTo, taskSessionPanelTo } from "./paths"

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

describe("sessionPanelTo", () => {
  it("keeps the filters of the screen it opens over", () => {
    const to = sessionPanelTo(new URLSearchParams("status=failed&role=reviewer"), "s1")
    const params = new URLSearchParams(to.search)
    expect(params.get("status")).toBe("failed")
    expect(params.get("role")).toBe("reviewer")
    expect(params.get("session")).toBe("s1")
  })

  it("leaves no panel around the session", () => {
    const to = sessionPanelTo(new URLSearchParams("goal=g1&task=t1&tab=diff&session=s1"), "s2")
    const params = new URLSearchParams(to.search)
    expect(params.get("session")).toBe("s2")
    expect(params.has("goal")).toBe(false)
    expect(params.has("task")).toBe(false)
    expect(params.has("tab")).toBe(false)
  })
})

describe("panelSessionTo", () => {
  it("points the open panel at the session", () => {
    const to = panelSessionTo(new URLSearchParams("goal=g1&task=t1&tab=diff"), "s1")
    const params = new URLSearchParams(to.search)
    expect(params.get("goal")).toBe("g1")
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s1")
  })

  it("replaces the session that was selected", () => {
    const to = panelSessionTo(new URLSearchParams("task=t1&tab=sessions&session=s1"), "s2")
    expect(new URLSearchParams(to.search).get("session")).toBe("s2")
  })

  it("comes back out of the session onto the list it came from", () => {
    const to = panelSessionTo(new URLSearchParams("task=t1&tab=sessions&session=s1"), null)
    const params = new URLSearchParams(to.search)
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.has("session")).toBe(false)
  })
})

describe("taskSessionPanelTo", () => {
  it("opens the task's panel on the session, keeping the screen's filters", () => {
    const to = taskSessionPanelTo(new URLSearchParams("status=failed&role=reviewer"), "t1", "s1")
    const params = new URLSearchParams(to.search)
    expect(params.get("status")).toBe("failed")
    expect(params.get("role")).toBe("reviewer")
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s1")
  })

  it("moves a panel that was open on another task's session", () => {
    const to = taskSessionPanelTo(new URLSearchParams("task=t1&tab=diff&session=s1"), "t2", "s2")
    const params = new URLSearchParams(to.search)
    expect(params.get("task")).toBe("t2")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s2")
  })
})
