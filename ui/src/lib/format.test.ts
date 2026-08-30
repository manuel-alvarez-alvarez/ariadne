import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import {
  cachedShare,
  describeError,
  folderName,
  formatAbsolute,
  formatAge,
  formatDuration,
  formatRelative,
  formatTokens,
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

describe("formatTokens", () => {
  // The rule, band by band, in the one place both this and the CLI's own
  // `tokens` can be read against each other: a count reads the same in a
  // terminal and on a screen or the two look like they disagree about it.
  const TABLE: [number, string][] = [
    [0, "0"],
    [950, "950"],
    [999, "999"],
    [1_000, "1k"],
    [1_234, "1.2k"],
    [1_950, "2k"],
    [9_949, "9.9k"],
    [9_950, "10k"],
    [45_300, "45k"],
    [516_000, "516k"],
    [999_499, "999k"],
    [999_500, "1M"],
    [1_234_567, "1.2M"],
    [1_950_000, "2M"],
    [9_949_999, "9.9M"],
    [9_950_000, "10M"],
    [12_345_678, "12M"],
    [999_499_999, "999M"],
    [999_500_000, "1G"],
    [1_234_000_000, "1.2G"],
    [9_949_999_999, "9.9G"],
    [9_950_000_000, "10G"],
    [45_000_000_000, "45G"],
    [999_499_999_999, "999G"],
    [999_500_000_000, "1T"],
    [1_500_000_000_000, "1.5T"],
    // Past the top band the figure grows a digit rather than a unit: there is
    // no letter above `T`, and nothing counts high enough to need one.
    [1_234_000_000_000_000, "1234T"],
  ]

  it.for(TABLE)("spells %i as %s", ([count, spelled]) => {
    expect(formatTokens(count)).toBe(spelled)
  })

  it("has nothing below zero to show", () => {
    expect(formatTokens(-5)).toBe("0")
  })
})

describe("cachedShare", () => {
  /** A share is only ever read off the input pair; the output is along for the ride. */
  const share = (input: number, cached: number) =>
    cachedShare({ input_tokens: input, cached_input_tokens: cached, output_tokens: 0 })

  it("is the cached part of the input, to the whole percent", () => {
    // The fixture the figure's own tests use: 1,100,000 of 1,234,567 is
    // 89.1%, not the 92% the two rounded figures beside it would suggest —
    // which is the whole reason the share is computed rather than eyeballed.
    expect(share(1_234_567, 1_100_000)).toBe("89%")
    expect(share(1_000_000, 900_000)).toBe("90%")
  })

  it("rounds to the nearer percent, and keeps no decimals", () => {
    expect(share(1_000, 894)).toBe("89%")
    expect(share(1_000, 895)).toBe("90%")
    expect(share(3, 1)).toBe("33%")
  })

  it("is 100% where the cache served the whole input", () => {
    expect(share(1_234_567, 1_234_567)).toBe("100%")
  })

  it("is 0% for a run that sent nothing, rather than dividing by it", () => {
    // A run that sent nothing cached nothing. Not `NaN%`, and not a dash: a
    // figure that comes and goes as a session starts up is harder to read.
    expect(share(0, 0)).toBe("0%")
    expect(share(0, 5_000)).toBe("0%")
  })

  it("clamps a count the daemon could only report by being wrong", () => {
    expect(share(1_000, 2_000)).toBe("100%")
    expect(share(1_000, -100)).toBe("0%")
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

describe("folderName", () => {
  it("takes the last segment of a checkout path", () => {
    expect(folderName("/a/b/ariadne")).toBe("ariadne")
  })

  it("ignores a trailing slash", () => {
    expect(folderName("/a/b/ariadne/")).toBe("ariadne")
  })

  it("leaves a bare name, with no separator, whole", () => {
    expect(folderName("ariadne")).toBe("ariadne")
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
