import { describe, expect, it } from "vitest"

import {
  GOAL_PARAM,
  readRoleFilter,
  readScopeFilter,
  readStatusFilter,
  restoreSessionFilters,
  statusFilters,
  TASK_PARAM,
  withFilter,
} from "./filters"

/** What the screen was last left with; a test only spells the part it is about. */
function remembered(overrides: Partial<Record<"status" | "role" | "goal" | "task", string>> = {}) {
  return { status: "", role: "", goal: "", task: "", ...overrides }
}

describe("readStatusFilter", () => {
  it("reads a status the daemon defines, and the one it does not", () => {
    expect(readStatusFilter(new URLSearchParams("status=failed"))).toBe("failed")
    expect(readStatusFilter(new URLSearchParams("status=live"))).toBe("live")
  })

  it("falls back to no filter for a missing or unknown status", () => {
    expect(readStatusFilter(new URLSearchParams(""))).toBeNull()
    expect(readStatusFilter(new URLSearchParams("status="))).toBeNull()
    expect(readStatusFilter(new URLSearchParams("status=nonsense"))).toBeNull()
  })
})

describe("readRoleFilter", () => {
  it("reads a role the daemon defines", () => {
    expect(readRoleFilter(new URLSearchParams("role=engineer"))).toBe("engineer")
  })

  it("falls back to no filter for a missing or unknown role", () => {
    expect(readRoleFilter(new URLSearchParams(""))).toBeNull()
    expect(readRoleFilter(new URLSearchParams("role=nobody"))).toBeNull()
  })
})

describe("withFilter", () => {
  it("sets one filter and keeps every other param", () => {
    const next = withFilter(new URLSearchParams("session=s1&role=planner"), "status", "failed")
    expect(next.get("session")).toBe("s1")
    expect(next.get("role")).toBe("planner")
    expect(next.get("status")).toBe("failed")
  })

  it("drops the param when the filter is cleared", () => {
    expect(withFilter(new URLSearchParams("status=failed"), "status", "all").has("status")).toBe(
      false,
    )
  })
})

describe("restoreSessionFilters", () => {
  it("puts both remembered filters back on a bare entry", () => {
    const next = restoreSessionFilters(
      new URLSearchParams(""),
      remembered({
        status: "live",
        role: "engineer",
      }),
    )
    expect(next?.get("status")).toBe("live")
    expect(next?.get("role")).toBe("engineer")
  })

  it("keeps a panel the entry was opened on", () => {
    const next = restoreSessionFilters(
      new URLSearchParams("session=s1"),
      remembered({ status: "failed" }),
    )
    expect(next?.get("session")).toBe("s1")
    expect(next?.get("status")).toBe("failed")
  })

  it("leaves an explicit filter alone, whatever is remembered", () => {
    const next = restoreSessionFilters(
      new URLSearchParams("status=failed&role=planner"),
      remembered({ status: "live", role: "engineer" }),
    )
    expect(next).toBeNull()
  })

  it("leaves an explicitly empty filter alone", () => {
    expect(
      restoreSessionFilters(
        new URLSearchParams("status=&role="),
        remembered({ status: "live", role: "engineer" }),
      ),
    ).toBeNull()
  })

  it("restores each filter on its own", () => {
    const next = restoreSessionFilters(
      new URLSearchParams("status=failed"),
      remembered({ status: "live", role: "engineer" }),
    )
    expect(next?.get("status")).toBe("failed")
    expect(next?.get("role")).toBe("engineer")
  })

  it("restores nothing when both filters were cleared", () => {
    expect(restoreSessionFilters(new URLSearchParams(""), remembered())).toBeNull()
  })

  it("restores nothing from values the daemon no longer defines", () => {
    expect(
      restoreSessionFilters(
        new URLSearchParams(""),
        remembered({ status: "nonsense", role: "nobody" }),
      ),
    ).toBeNull()
  })
})

describe("readScopeFilter", () => {
  it("reads the id a goal or a task narrows the list to", () => {
    const params = new URLSearchParams("goal=01JGOAL&task=01JTASK")
    expect(readScopeFilter(params, GOAL_PARAM)).toBe("01JGOAL")
    expect(readScopeFilter(params, TASK_PARAM)).toBe("01JTASK")
  })

  it("takes an empty or missing param as no scope at all", () => {
    expect(readScopeFilter(new URLSearchParams(""), GOAL_PARAM)).toBeNull()
    expect(readScopeFilter(new URLSearchParams("goal="), GOAL_PARAM)).toBeNull()
    expect(readScopeFilter(new URLSearchParams("goal=%20%20"), GOAL_PARAM)).toBeNull()
  })
})

describe("statusFilters", () => {
  it("sends a real status to the daemon and narrows the other two here", () => {
    expect(statusFilters("failed")).toEqual({ status: "failed" })
    expect(statusFilters("live")).toEqual({ live: true })
    expect(statusFilters("attention")).toEqual({ attention: true })
    expect(statusFilters(null)).toEqual({})
  })
})

describe("restoreSessionFilters, for the scope", () => {
  it("puts a remembered goal back, and keeps the one the URL asked for", () => {
    expect(
      restoreSessionFilters(new URLSearchParams(""), remembered({ goal: "01JGOAL" }))?.get("goal"),
    ).toBe("01JGOAL")
    expect(
      restoreSessionFilters(new URLSearchParams("goal=01JOTHER"), remembered({ goal: "01JGOAL" })),
    ).toBeNull()
  })

  it("leaves a cleared scope cleared", () => {
    expect(
      restoreSessionFilters(new URLSearchParams("goal="), remembered({ goal: "01JGOAL" })),
    ).toBeNull()
  })
})
