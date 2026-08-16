import { describe, expect, it } from "vitest"

import { formatAbsolute, formatAge, formatDuration, formatRelative } from "./time"

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

  it("stands in for a missing timestamp", () => {
    expect(formatAge(null, NOW)).toBe("—")
    expect(formatAge(undefined, NOW)).toBe("—")
  })

  it("passes an unparsable value through", () => {
    expect(formatAge("not a date", NOW)).toBe("not a date")
  })
})

describe("formatRelative", () => {
  it("names the largest unit that fits", () => {
    expect(formatRelative(ago(3 * 3600), NOW)).toBe("3 hours ago")
    expect(formatRelative(ago(5 * 60), NOW)).toBe("5 minutes ago")
    expect(formatRelative(ago(5), NOW)).toBe("5 seconds ago")
  })

  it("passes an unparsable value through", () => {
    expect(formatRelative("not a date", NOW)).toBe("not a date")
  })
})

describe("formatAbsolute", () => {
  it("stands in for a missing timestamp", () => {
    expect(formatAbsolute(null)).toBe("—")
    expect(formatAbsolute("")).toBe("—")
  })

  it("passes an unparsable value through", () => {
    expect(formatAbsolute("not a date")).toBe("not a date")
  })
})
