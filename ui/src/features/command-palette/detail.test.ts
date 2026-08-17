import { describe, expect, it } from "vitest"

import { splitDetail } from "./detail"

describe("splitDetail", () => {
  it("leaves a short detail whole, with nothing to truncate around", () => {
    expect(splitDetail("…0000000C")).toEqual({ head: "…0000000C", tail: "" })
    expect(splitDetail("Reviewer")).toEqual({ head: "Reviewer", tail: "" })
  })

  it("keeps the trailing slug of a branch, whole", () => {
    const { head, tail } = splitDetail("ariadne/task-01m06j3y2rsx7j-command-palette")
    expect(tail).toBe("-command-palette")
    expect(head + tail).toBe("ariadne/task-01m06j3y2rsx7j-command-palette")
  })

  it("keeps the end of a branch that is all id, where no separator is near it", () => {
    const branch = "ariadne/task-01m06j3y2rsx7j5t9bkasxm7cg"
    const { head, tail } = splitDetail(branch)
    expect(tail).toBe(branch.slice(-16))
    expect(head).toBe("ariadne/task-01m06j3y2r")
  })

  it("cuts at the separator, so the tail is never half a word", () => {
    const { head, tail } = splitDetail("feature/some-very-long-branch/ui")
    expect(tail).toBe("-long-branch/ui")
    expect(head).toBe("feature/some-very")
  })
})
