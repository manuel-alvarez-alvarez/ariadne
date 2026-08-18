import { describe, expect, it } from "vitest"

import { canStepBack, closePanel, type Panel, withoutPanel } from "./panel-history"

/**
 * The browser's session history, as much of it as the panels use: entries the
 * links push, the index Back and Forward move, and the `idx` React Router
 * writes into each entry's state.
 */
class History {
  entries: string[]
  index = 0

  constructor(start: string) {
    this.entries = [start]
  }

  get current(): string {
    return this.entries[this.index] ?? ""
  }

  get state(): { idx: number } {
    return { idx: this.index }
  }

  /** What a `<Link>` does: a new entry, and anything ahead of it is dropped. */
  push(search: string) {
    this.entries = [...this.entries.slice(0, this.index + 1), search]
    this.index += 1
  }

  replace(search: string) {
    this.entries[this.index] = search
  }

  back() {
    this.index = Math.max(0, this.index - 1)
  }

  /** Closing a panel, exactly as `DetailPanels` does it. */
  close(panel: Panel) {
    const step = closePanel(panel, new URLSearchParams(this.current), this.state)
    if (step.kind === "back") this.back()
    else this.replace(step.search.toString())
  }
}

describe("closing a panel", () => {
  it("does not leave an entry behind that Back would reopen it from", () => {
    // The board, a goal opened from a lane, one of its tasks opened over it.
    const history = new History("")
    history.push("goal=g1")
    history.push("goal=g1&task=t1")

    history.close("task")
    expect(history.current).toBe("goal=g1") // back onto the goal it was opened from

    history.close("goal")
    expect(history.current).toBe("")

    history.back()
    expect(history.current).toBe("")
    expect(history.index).toBe(0)
  })

  it("closes in place when its entry is the first of the session", () => {
    // A deep link, or a reload on an open panel: there is nothing to step back
    // to, so the URL is rewritten instead.
    const history = new History("goal=g1&task=t1")

    history.close("task")
    expect(history.current).toBe("goal=g1")
    expect(history.entries).toHaveLength(1)

    history.close("goal")
    expect(history.current).toBe("")
    expect(history.entries).toHaveLength(1)
  })

  it("takes the whole stack down when it is closed from underneath", () => {
    // Not reachable from the UI — the goal's sheet is behind the task's while
    // one is stacked on it — but a step back would only close the task.
    const history = new History("")
    history.push("goal=g1")
    history.push("goal=g1&task=t1")

    history.close("goal")
    expect(history.current).toBe("")
  })

  it("keeps the filters the screen owns", () => {
    const history = new History("status=active")
    history.push("status=active&goal=g1")

    history.close("goal")
    expect(history.current).toBe("status=active")
  })

  it("steps back out of a session opened over a list", () => {
    // A filtered list, with a row picked from it.
    const history = new History("status=failed")
    history.push("status=failed&session=s1")

    history.close("session")
    expect(history.current).toBe("status=failed")
    expect(history.index).toBe(0)
  })

  it("closes a deep-linked session in place", () => {
    const history = new History("status=failed&session=s1")

    history.close("session")
    expect(history.current).toBe("status=failed")
    expect(history.entries).toHaveLength(1)
  })
})

describe("withoutPanel", () => {
  it("takes the panel's own state with it", () => {
    const left = withoutPanel("task", new URLSearchParams("goal=g1&task=t1&tab=diff&session=s1"))
    expect(left.toString()).toBe("goal=g1")
  })

  it("takes the task stacked on a goal down with the goal", () => {
    const left = withoutPanel("goal", new URLSearchParams("status=active&goal=g1&task=t1&tab=diff"))
    expect(left.toString()).toBe("status=active")
  })

  it("leaves the screen's own params when a session panel closes", () => {
    const left = withoutPanel(
      "session",
      new URLSearchParams("status=failed&role=engineer&session=s1"),
    )
    expect(left.toString()).toBe("status=failed&role=engineer")
  })
})

describe("canStepBack", () => {
  it("is true only for an entry this app pushed its way to", () => {
    expect(canStepBack({ idx: 1 })).toBe(true)
    expect(canStepBack({ idx: 0 })).toBe(false)
    expect(canStepBack({ usr: null })).toBe(false)
    expect(canStepBack(null)).toBe(false)
    expect(canStepBack(undefined)).toBe(false)
  })
})
