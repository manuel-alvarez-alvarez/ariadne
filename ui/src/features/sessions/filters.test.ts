import { describe, expect, it } from "vitest"

import { readRoleFilter, readStatusFilter, restoreSessionFilters, withFilter } from "./filters"

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
    const next = restoreSessionFilters(new URLSearchParams(""), {
      status: "live",
      role: "engineer",
    })
    expect(next?.get("status")).toBe("live")
    expect(next?.get("role")).toBe("engineer")
  })

  it("keeps a panel the entry was opened on", () => {
    const next = restoreSessionFilters(new URLSearchParams("session=s1"), {
      status: "failed",
      role: "",
    })
    expect(next?.get("session")).toBe("s1")
    expect(next?.get("status")).toBe("failed")
  })

  it("leaves an explicit filter alone, whatever is remembered", () => {
    const next = restoreSessionFilters(new URLSearchParams("status=failed&role=planner"), {
      status: "live",
      role: "engineer",
    })
    expect(next).toBeNull()
  })

  it("leaves an explicitly empty filter alone", () => {
    expect(
      restoreSessionFilters(new URLSearchParams("status=&role="), {
        status: "live",
        role: "engineer",
      }),
    ).toBeNull()
  })

  it("restores each filter on its own", () => {
    const next = restoreSessionFilters(new URLSearchParams("status=failed"), {
      status: "live",
      role: "engineer",
    })
    expect(next?.get("status")).toBe("failed")
    expect(next?.get("role")).toBe("engineer")
  })

  it("restores nothing when both filters were cleared", () => {
    expect(restoreSessionFilters(new URLSearchParams(""), { status: "", role: "" })).toBeNull()
  })

  it("restores nothing from values the daemon no longer defines", () => {
    expect(
      restoreSessionFilters(new URLSearchParams(""), { status: "nonsense", role: "nobody" }),
    ).toBeNull()
  })
})
