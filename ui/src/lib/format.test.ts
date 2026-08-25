import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import {
  describeError,
  formatAbsolute,
  formatAge,
  formatDuration,
  formatRelative,
  middleTruncate,
  plural,
  shortId,
  shortSha,
} from "./format"

describe("plural", () => {
  it("agrees with the count, and treats zero as many", () => {
    expect(plural(1, "task")).toBe("1 task")
    expect(plural(2, "task")).toBe("2 tasks")
    expect(plural(0, "item")).toBe("0 items")
  })

  it("takes an irregular plural, for the nouns -s does not reach", () => {
    expect(plural(1, "repository", "repositories")).toBe("1 repository")
    expect(plural(3, "repository", "repositories")).toBe("3 repositories")
  })
})

describe("shortId", () => {
  it("keeps the tail of a ULID, which is what tells two apart", () => {
    expect(shortId("01k2ta9v7m1qz8xr4bd6hnpc3e")).toBe("…d6hnpc3e")
  })

  it("leaves a value short enough to read whole", () => {
    expect(shortId("main")).toBe("main")
  })
})

describe("shortSha", () => {
  it("shows a git object id the way git does", () => {
    expect(shortSha("9f6a1c2b3d4e5f60718293a4b5c6d7e8f9012345")).toBe("9f6a1c2b3d")
  })
})

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

describe("describeError", () => {
  it("appends the daemon's error code, which is what its docs talk about", () => {
    const error = new ApiError({
      status: 409,
      code: "illegal_transition",
      message: "cannot cancel",
    })
    expect(describeError(error)).toBe("cannot cancel (illegal_transition)")
  })

  it("leaves off the codes that name nothing a reader could look up", () => {
    expect(describeError(ApiError.network(new Error("connection refused")))).toBe(
      "cannot reach the daemon: connection refused",
    )
    expect(
      describeError(new ApiError({ status: 502, code: "http_error", message: "502 Bad Gateway" })),
    ).toBe("502 Bad Gateway")
  })

  it("falls back to whatever was thrown", () => {
    expect(describeError(new Error("boom"))).toBe("boom")
    expect(describeError("boom")).toBe("boom")
  })
})

const NOW = Date.parse("2026-08-16T12:00:00Z")

function ago(seconds: number): string {
  return new Date(NOW - seconds * 1000).toISOString()
}

describe("formatDuration", () => {
  it("floors every unit rather than rounding into the next one", () => {
    expect(formatDuration(59)).toBe("59s")
    expect(formatDuration(89)).toBe("1m")
    expect(formatDuration(90)).toBe("1m")
    expect(formatDuration(119)).toBe("1m")
    expect(formatDuration(120)).toBe("2m")
    expect(formatDuration(3599)).toBe("59m")
    expect(formatDuration(3600)).toBe("1h")
    expect(formatDuration(86_399)).toBe("23h")
    expect(formatDuration(86_400)).toBe("1d")
  })

  it("never goes negative", () => {
    expect(formatDuration(-5)).toBe("0s")
  })
})

describe("formatAge", () => {
  it("is the compact form, with no suffix", () => {
    expect(formatAge(ago(12), NOW)).toBe("12s")
    expect(formatAge(ago(89), NOW)).toBe("1m")
    expect(formatAge(ago(4 * 3600), NOW)).toBe("4h")
  })
})

describe("formatRelative", () => {
  it("names the largest unit that fits", () => {
    expect(formatRelative(ago(3 * 3600), NOW)).toBe("3 hours ago")
    expect(formatRelative(ago(5 * 60), NOW)).toBe("5 minutes ago")
    expect(formatRelative(ago(5), NOW)).toBe("5 seconds ago")
  })
})

describe("the timestamp formatters", () => {
  it("stand in for a missing timestamp, so a column never goes blank", () => {
    expect(formatAge(null, NOW)).toBe("—")
    expect(formatAge(undefined, NOW)).toBe("—")
    expect(formatAbsolute(null)).toBe("—")
    expect(formatAbsolute("")).toBe("—")
  })

  it("pass an unparsable value through rather than showing 'Invalid Date'", () => {
    expect(formatAge("not a date", NOW)).toBe("not a date")
    expect(formatRelative("not a date", NOW)).toBe("not a date")
    expect(formatAbsolute("not a date")).toBe("not a date")
  })
})
