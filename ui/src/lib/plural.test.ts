import { describe, expect, it } from "vitest"

import { plural } from "./plural"

describe("plural", () => {
  it("agrees with the count", () => {
    expect(plural(1, "task")).toBe("1 task")
    expect(plural(2, "task")).toBe("2 tasks")
  })

  it("pluralizes zero, the way English does", () => {
    expect(plural(0, "item")).toBe("0 items")
  })
})
