/**
 * The comma-separated box, read as a list of skill names.
 *
 * One function serves the control the user types into and the form values the
 * daemon is sent, so what the screen says an agent knows and what is asked for
 * cannot come apart. What it has to survive is what people actually type.
 */

import { describe, expect, it } from "vitest"

import { parseSkillNames } from "./parse"

describe("parseSkillNames", () => {
  it("keeps the order the names were written in", () => {
    expect(parseSkillNames("testing, coding, research")).toEqual(["testing", "coding", "research"])
  })

  it("trims the spaces around each name", () => {
    expect(parseSkillNames("  coding ,testing  ")).toEqual(["coding", "testing"])
  })

  it("drops the blanks a trailing or doubled comma leaves", () => {
    expect(parseSkillNames("coding,,testing,")).toEqual(["coding", "testing"])
    expect(parseSkillNames("")).toEqual([])
    expect(parseSkillNames("   ")).toEqual([])
  })

  it("keeps the first of a repeated name and drops the rest", () => {
    expect(parseSkillNames("coding, testing, coding")).toEqual(["coding", "testing"])
  })
})
