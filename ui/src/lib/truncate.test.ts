import { describe, expect, it } from "vitest"

import { middleTruncate } from "./truncate"

describe("middleTruncate", () => {
  it("keeps the last segment of a worktree path and offers the rest up", () => {
    const { head, tail } = middleTruncate("/Users/me/.ariadne/worktrees/nnqqbdx4/nwvfdf4v-eng")
    expect(tail).toBe("/nwvfdf4v-eng")
    expect(head).toBe("/Users/me/.ariadne/worktrees/nnqqbdx4")
  })

  it("cuts at the last separator, not the first", () => {
    expect(middleTruncate("/Users/me/.ariadne/worktrees/abc/eng").tail).toBe("/eng")
  })

  it("keeps the end of a task branch, where the id's tail identifies it", () => {
    // A branch has no segment to split on at all, so the character split
    // keeps the tail of the id — the half that tells two same-titled tasks
    // apart — and offers the title up.
    const { head, tail } = middleTruncate("fix-the-landing-briefing-real-fetch-r9jr7c")
    expect(tail).toBe("h-r9jr7c")
    expect(head).toBe("fix-the-landing-briefing-real-fetc")
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
    for (const value of ["worktrees/nnqqbdx4/eng", "abcdefghij", "a/", "/a", "//"]) {
      const { head, tail } = middleTruncate(value)
      expect(head + tail).toBe(value)
    }
  })
})
