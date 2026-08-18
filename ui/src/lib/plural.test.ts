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

  it("takes an irregular plural, for the nouns -s does not reach", () => {
    expect(plural(1, "repository", "repositories")).toBe("1 repository")
    expect(plural(3, "repository", "repositories")).toBe("3 repositories")
  })
})
