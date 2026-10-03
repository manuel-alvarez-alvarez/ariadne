import { describe, expect, it } from "vitest"

import { closePanel, type Panel } from "./panel-history"

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
    // The board, then a goal opened from a lane.
    const history = new History("")
    history.push("goal=g1")

    history.close("goal")
    expect(history.current).toBe("")

    history.back()
    expect(history.current).toBe("")
    expect(history.index).toBe(0)
  })

  it("does not step back into the goal a task replaced", () => {
    // The board, a goal opened from a lane, one of its tasks opened over it —
    // a task replaces its goal in the same entry rather than stacking on it
    // (see `task-card.tsx`), so there is nothing of the goal's left to step
    // back into.
    const history = new History("")
    history.push("goal=g1")
    history.replace("task=t1")

    history.close("task")
    expect(history.current).toBe("") // back past the goal, onto the bare board
  })

  it("closes in place when its entry is the first of the session", () => {
    // A deep link, or a reload on an open panel: there is nothing to step back
    // to, so the URL is rewritten instead.
    const history = new History("task=t1")

    history.close("task")
    expect(history.current).toBe("")
    expect(history.entries).toHaveLength(1)
  })

  it("clears a stray goal alongside the task it replaces, even hand-typed", () => {
    // `?goal=` and `?task=` never open together through the app, but a
    // deep link can still hold both — and a task's close must not leave the
    // goal panel to reappear from it.
    const history = new History("goal=g1&task=t1")

    history.close("task")
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
