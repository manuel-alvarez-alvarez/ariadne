import { describe, expect, it } from "vitest"

import {
  DEFAULT_GOAL_STATUS_FILTER,
  NO_STATUS_FILTER,
  normalizeStatusFilter,
  parseStatusFilter,
  readStatusFilter,
  restoreStatusFilter,
  serializeStatusFilter,
  showsFinished,
  toggleStatusFilter,
  withFinished,
  withStatusFilter,
} from "./filters"
import { GOAL_STATUSES } from "./status"

describe("readStatusFilter", () => {
  it("reads a status the daemon defines", () => {
    expect(readStatusFilter(new URLSearchParams("status=planning"))).toEqual(["planning"])
  })

  it("reads several comma-separated statuses", () => {
    expect(readStatusFilter(new URLSearchParams("status=completed,planning"))).toEqual([
      "planning",
      "completed",
    ])
  })

  it("falls back to no filter for a missing status", () => {
    expect(readStatusFilter(new URLSearchParams(""))).toEqual(NO_STATUS_FILTER)
    expect(readStatusFilter(new URLSearchParams("status="))).toEqual(NO_STATUS_FILTER)
  })

  it("drops the values the daemon does not define", () => {
    expect(readStatusFilter(new URLSearchParams("status=nonsense"))).toEqual(NO_STATUS_FILTER)
    expect(readStatusFilter(new URLSearchParams("status=active,nonsense"))).toEqual(["active"])
    expect(readStatusFilter(new URLSearchParams("status=,,active,"))).toEqual(["active"])
  })

  it("reads a selection of every status as no filter", () => {
    const every = new URLSearchParams(`status=${GOAL_STATUSES.join(",")}`)
    expect(readStatusFilter(every)).toEqual(NO_STATUS_FILTER)
  })
})

describe("normalizeStatusFilter", () => {
  it("orders the selection and drops duplicates", () => {
    expect(normalizeStatusFilter(["cancelled", "planning", "cancelled"])).toEqual([
      "planning",
      "cancelled",
    ])
  })
})

describe("withStatusFilter", () => {
  it("keeps the params the panels own", () => {
    const next = withStatusFilter(new URLSearchParams("goal=g1&task=t1"), ["active"])
    expect(next.get("goal")).toBe("g1")
    expect(next.get("task")).toBe("t1")
    expect(next.get("status")).toBe("active")
  })

  it("serializes several statuses in lifecycle order", () => {
    const next = withStatusFilter(new URLSearchParams(""), ["completed", "active"])
    expect(next.get("status")).toBe("active,completed")
  })

  it("gives equal selections equal URLs", () => {
    const one = withStatusFilter(new URLSearchParams(""), ["completed", "active"])
    const other = withStatusFilter(new URLSearchParams(""), ["active", "completed", "active"])
    expect(one.toString()).toBe(other.toString())
  })

  it("drops the param when the filter is off", () => {
    const next = withStatusFilter(new URLSearchParams("status=active&goal=g1"), NO_STATUS_FILTER)
    expect(next.has("status")).toBe(false)
    expect(next.get("goal")).toBe("g1")
  })

  it("drops the param when every status is selected", () => {
    const next = withStatusFilter(new URLSearchParams("status=active"), GOAL_STATUSES)
    expect(next.has("status")).toBe(false)
  })

  it("round-trips a single status through the URL", () => {
    const next = withStatusFilter(new URLSearchParams(""), ["completed"])
    expect(readStatusFilter(next)).toEqual(["completed"])
  })

  it("round-trips several statuses through the URL", () => {
    const filter = ["planning", "completed"] as const
    expect(readStatusFilter(withStatusFilter(new URLSearchParams(""), filter))).toEqual(filter)
  })
})

describe("toggleStatusFilter", () => {
  it("checks a status onto the selection, in order", () => {
    expect(toggleStatusFilter(["completed"], "active")).toEqual(["active", "completed"])
  })

  it("unchecks a status off the selection", () => {
    expect(toggleStatusFilter(["active", "completed"], "active")).toEqual(["completed"])
  })

  it("lands back on no filter when the last status is unchecked", () => {
    expect(toggleStatusFilter(["active"], "active")).toEqual(NO_STATUS_FILTER)
  })

  it("lands back on no filter when every status ends up selected", () => {
    const allButOne = GOAL_STATUSES.filter((status) => status !== "cancelled")
    expect(toggleStatusFilter(allButOne, "cancelled")).toEqual(NO_STATUS_FILTER)
  })
})

describe("serializeStatusFilter", () => {
  it("spells a selection the way the param does", () => {
    expect(serializeStatusFilter(["completed", "active"])).toBe("active,completed")
  })

  it("spells no filter, and every status, as nothing at all", () => {
    expect(serializeStatusFilter(NO_STATUS_FILTER)).toBe("")
    expect(serializeStatusFilter(GOAL_STATUSES)).toBe("")
  })

  it("round-trips a remembered selection", () => {
    const filter = ["planning", "completed"] as const
    expect(parseStatusFilter(serializeStatusFilter(filter))).toEqual(filter)
  })
})

describe("restoreStatusFilter", () => {
  it("puts the remembered filter back on a bare entry", () => {
    const next = restoreStatusFilter(new URLSearchParams(""), "active,completed")
    expect(next?.get("status")).toBe("active,completed")
  })

  it("keeps a panel the entry was opened on", () => {
    const next = restoreStatusFilter(new URLSearchParams("goal=g1"), "active")
    expect(next?.get("goal")).toBe("g1")
    expect(next?.get("status")).toBe("active")
  })

  it("leaves an explicit filter alone, whatever is remembered", () => {
    expect(restoreStatusFilter(new URLSearchParams("status=planning"), "active")).toBeNull()
  })

  it("leaves an explicitly empty filter alone", () => {
    expect(restoreStatusFilter(new URLSearchParams("status="), "active")).toBeNull()
  })

  it("restores nothing when the filter was cleared", () => {
    expect(restoreStatusFilter(new URLSearchParams(""), "")).toBeNull()
  })

  it("restores nothing from a value the daemon no longer defines", () => {
    expect(restoreStatusFilter(new URLSearchParams(""), "nonsense")).toBeNull()
  })
})

describe("the default filter", () => {
  it("opens the board on the work that is still moving", () => {
    // Not "all statuses": a few weeks in, that is a wall of finished lanes.
    expect(parseStatusFilter(DEFAULT_GOAL_STATUS_FILTER)).toEqual(["planning", "active"])
  })

  it("is spelled the way the param and the settings store spell one", () => {
    expect(serializeStatusFilter(parseStatusFilter(DEFAULT_GOAL_STATUS_FILTER))).toBe(
      DEFAULT_GOAL_STATUS_FILTER,
    )
  })

  it("is put back on a bare entry, like any remembered filter", () => {
    const next = restoreStatusFilter(new URLSearchParams(""), DEFAULT_GOAL_STATUS_FILTER)
    expect(next?.get("status")).toBe("planning,active")
  })
})

describe("the finished toggle", () => {
  it("reads an unfiltered board as showing everything", () => {
    expect(showsFinished(NO_STATUS_FILTER)).toBe(true)
  })

  it("reads the default filter as hiding what is finished", () => {
    expect(showsFinished(parseStatusFilter(DEFAULT_GOAL_STATUS_FILTER))).toBe(false)
  })

  it("reads a selection that lets one finished status through as showing them", () => {
    expect(showsFinished(["active", "cancelled"])).toBe(true)
  })

  it("turns them on from the default, which is every status", () => {
    const shown = withFinished(parseStatusFilter(DEFAULT_GOAL_STATUS_FILTER), true)
    expect(shown).toEqual(NO_STATUS_FILTER)
    expect(showsFinished(shown)).toBe(true)
  })

  it("turns them off from an unfiltered board by narrowing it to the live ones", () => {
    expect(withFinished(NO_STATUS_FILTER, false)).toEqual(["planning", "active"])
  })

  it("leaves the rest of a selection where it was", () => {
    expect(withFinished(["active"], true)).toEqual(["active", "completed", "cancelled"])
    expect(withFinished(["active", "completed"], false)).toEqual(["active"])
  })

  it("never empties the board", () => {
    // Only finished statuses were selected: turning them off has to land
    // somewhere, and the live ones are what the toggle meant.
    expect(withFinished(["completed", "cancelled"], false)).toEqual(["planning", "active"])
  })

  it("round-trips: off, then on again, is every status", () => {
    const off = withFinished(NO_STATUS_FILTER, false)
    expect(withFinished(off, true)).toEqual(NO_STATUS_FILTER)
  })
})
