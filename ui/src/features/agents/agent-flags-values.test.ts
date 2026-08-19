/**
 * The two questions the flags form asks of a list, both of which decide what
 * the user sees rather than only what is sent.
 *
 * `cleanFlags` is what an argv line survives: a row being typed into is blank
 * for a while and must not become an empty argument, and stray whitespace
 * around a flag is not part of it. `sameFlags` is what "Customized" and the
 * restore button are driven by, and it is deliberately order-sensitive —
 * flags go on a command line, where order is meaning.
 */

import { describe, expect, it } from "vitest"

import { cleanFlags, flagRows, sameFlags } from "./agent-flags-values"

describe("cleanFlags", () => {
  it("trims the rows and drops the blank ones", () => {
    const rows = [{ value: "  --verbose  " }, { value: "   " }, { value: "--flag=1" }]
    expect(cleanFlags(rows)).toEqual(["--verbose", "--flag=1"])
  })

  it("answers an empty list, which is a legitimate flag list", () => {
    expect(cleanFlags([{ value: "" }])).toEqual([])
  })
})

describe("flagRows", () => {
  it("wraps the stored flags as rows, keeping their order", () => {
    expect(flagRows(["--auto", "--print"])).toEqual([{ value: "--auto" }, { value: "--print" }])
  })
})

describe("sameFlags", () => {
  it("holds for the same argv", () => {
    expect(sameFlags(["--auto"], ["--auto"])).toBe(true)
    expect(sameFlags([], [])).toBe(true)
  })

  it("fails on a reorder, which is a different launch", () => {
    expect(sameFlags(["--a", "--b"], ["--b", "--a"])).toBe(false)
  })

  it("fails on a different length", () => {
    expect(sameFlags(["--auto"], [])).toBe(false)
    expect(sameFlags([], ["--auto"])).toBe(false)
  })
})
