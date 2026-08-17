import { describe, expect, it } from "vitest"

import { middleTruncate } from "./truncate"

describe("middleTruncate", () => {
  it("keeps the slug of a task branch and offers the ULID prefix up", () => {
    const { head, tail } = middleTruncate("ariadne/task-01k2ta9v7m1qz8xr4bd6hnpc3e/fix-login-flow")
    expect(tail).toBe("/fix-login-flow")
    expect(head).toBe("ariadne/task-01k2ta9v7m1qz8xr4bd6hnpc3e")
  })

  it("cuts at the last separator, not the first", () => {
    expect(middleTruncate("/Users/me/.ariadne/worktrees/abc/eng").tail).toBe("/eng")
  })

  it("keeps the end of a slugless branch, whose last segment is the id itself", () => {
    // What the daemon names a task's branch when the task has no slug: the
    // segment split would keep `/task-01m0…` and cut the half that identifies
    // it, so the character split wins.
    const { head, tail } = middleTruncate("ariadne/task-01m06j3g920ekbp7zbsjbcajyp")
    expect(tail).toBe("sjbcajyp")
    expect(head).toBe("ariadne/task-01m06j3g920ekbp7zb")
  })

  it("keeps the last characters when there is no segment to keep", () => {
    expect(middleTruncate("01k2ta9v7m1qz8xr4bd6hnpc3e")).toEqual({
      head: "01k2ta9v7m1qz8xr4b",
      tail: "d6hnpc3e",
    })
  })

  it("leaves a short value whole", () => {
    expect(middleTruncate("main")).toEqual({ head: "main", tail: "" })
  })

  it("puts every character in one part or the other", () => {
    for (const value of ["ariadne/task-01k2/slug", "abcdefghij", "a/", "/a", "//"]) {
      const { head, tail } = middleTruncate(value)
      expect(head + tail).toBe(value)
    }
  })
})
