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
