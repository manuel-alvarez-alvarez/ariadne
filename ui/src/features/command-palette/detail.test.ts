import { describe, expect, it } from "vitest"

import { splitDetail } from "./detail"

describe("splitDetail", () => {
  it("leaves a short detail whole, with nothing to truncate around", () => {
    expect(splitDetail("…0000000C")).toEqual({ head: "…0000000C", tail: "" })
    expect(splitDetail("Reviewer")).toEqual({ head: "Reviewer", tail: "" })
  })

  it("keeps the end of a branch — its last words and the id tail — whole", () => {
    const branch = "render-prompts-from-the-database-r9jr7c"
    const { head, tail } = splitDetail(branch)
    expect(tail).toBe("-database-r9jr7c")
    expect(head + tail).toBe(branch)
  })

  it("keeps the end of an id, where no separator is near it", () => {
    const id = "01m06j3y2rsx7j5t9bkasxm7cg"
    const { head, tail } = splitDetail(id)
    expect(tail).toBe(id.slice(-16))
    expect(head).toBe("01m06j3y2r")
  })

  it("cuts at the separator, so the tail is never half a word", () => {
    const { head, tail } = splitDetail("feature/some-very-long-branch/ui")
    expect(tail).toBe("-long-branch/ui")
    expect(head).toBe("feature/some-very")
  })
})
