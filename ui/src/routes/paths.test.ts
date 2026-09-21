// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react"
import { createElement } from "react"
import { BrowserRouter, useLocation } from "react-router-dom"
import { describe, expect, it } from "vitest"
import { startAt } from "@/test/harness"
import {
  paths,
  sessionPanelFrom,
  taskPanelFrom,
  taskPanelTo,
  taskSessionPanelFrom,
  usePanelSessionNavigation,
  usePanelSessionTo,
} from "./paths"

/** Any screen but the sessions one, where a task panel opens over the screen. */
const OVER = paths.goals()

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

describe("sessionPanelFrom", () => {
  it("keeps the filters of the screen it opens over", () => {
    const to = sessionPanelFrom(OVER, new URLSearchParams("status=failed&seat=reviewer"), "s1")
    const params = new URLSearchParams(to.search)
    expect(params.get("status")).toBe("failed")
    expect(params.get("seat")).toBe("reviewer")
    expect(params.get("session")).toBe("s1")
  })

  it("leaves no panel around the session", () => {
    const to = sessionPanelFrom(
      OVER,
      new URLSearchParams("goal=g1&task=t1&tab=diff&session=s1"),
      "s2",
    )
    const params = new URLSearchParams(to.search)
    expect(params.get("session")).toBe("s2")
    expect(params.has("goal")).toBe(false)
    expect(params.has("task")).toBe(false)
    expect(params.has("tab")).toBe(false)
  })

  it("leaves the sessions screen's own two params alone, being its filters", () => {
    const to = sessionPanelFrom(paths.sessions(), new URLSearchParams("goal=g1&task=t1"), "s1")
    const params = new URLSearchParams(to.search)
    expect(params.get("goal")).toBe("g1")
    expect(params.get("task")).toBe("t1")
    expect(params.get("session")).toBe("s1")
  })
})

describe("taskSessionPanelFrom", () => {
  it("opens the task's panel on the session, keeping the screen's filters", () => {
    const to = taskSessionPanelFrom(
      OVER,
      new URLSearchParams("status=failed&seat=reviewer"),
      "t1",
      "s1",
    )
    const params = new URLSearchParams(to.search)
    expect(to.pathname).toBeUndefined()
    expect(params.get("status")).toBe("failed")
    expect(params.get("seat")).toBe("reviewer")
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s1")
  })

  it("moves a panel that was open on another task's session", () => {
    const to = taskSessionPanelFrom(
      OVER,
      new URLSearchParams("task=t1&tab=diff&session=s1"),
      "t2",
      "s2",
    )
    const params = new URLSearchParams(to.search)
    expect(params.get("task")).toBe("t2")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s2")
  })

  it("takes the whole thing to the board from the screen that has no task panel", () => {
    const to = taskSessionPanelFrom(paths.sessions(), new URLSearchParams("goal=g1"), "t1", "s1")
    expect(to.pathname).toBe(paths.goals())
    const params = new URLSearchParams(to.search)
    // The sessions screen's own filters stay behind on the sessions screen:
    // this is a navigation away from it, not a panel over it.
    expect(params.has("goal")).toBe(false)
    expect(params.get("task")).toBe("t1")
    expect(params.get("session")).toBe("s1")
  })
})

describe("taskPanelFrom", () => {
  it("stacks the panel on the screen it was asked from", () => {
    const to = taskPanelFrom(OVER, new URLSearchParams("goal=g1"), "t1")
    expect(to).toEqual(taskPanelTo(new URLSearchParams("goal=g1"), "t1"))
  })

  it("opens on the board from the sessions screen, whose `?task=` is a filter", () => {
    // Narrowing the list to the task is what `?task=` does there, and it is
    // not what somebody who picked the task asked for.
    expect(taskPanelFrom(paths.sessions(), new URLSearchParams("status=failed"), "t1")).toEqual({
      pathname: paths.goals(),
      search: "?task=t1",
    })
  })
})

describe("usePanelSessionTo", () => {
  it("keeps the open panel and moves its selected session", () => {
    startAt("/?goal=g1&task=t1&tab=diff")
    const { result } = renderHook(() => usePanelSessionTo("s2"), {
      wrapper: ({ children }) => createElement(BrowserRouter, null, children),
    })

    const params = new URLSearchParams(result.current.search)
    expect(params.get("goal")).toBe("g1")
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.get("session")).toBe("s2")
  })
})

describe("usePanelSessionNavigation", () => {
  it("comes back out of the session onto the list it came from", () => {
    startAt("/?task=t1&tab=sessions&session=s1")
    const { result } = renderHook(
      () => ({ navigate: usePanelSessionNavigation(), location: useLocation() }),
      { wrapper: ({ children }) => createElement(BrowserRouter, null, children) },
    )

    act(() => result.current.navigate(null))

    const params = new URLSearchParams(result.current.location.search)
    expect(params.get("task")).toBe("t1")
    expect(params.get("tab")).toBe("sessions")
    expect(params.has("session")).toBe(false)
  })
})
