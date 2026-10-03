import { describe, expect, it } from "vitest"

import { STATUS_COLORS } from "./status-colors"

describe("STATUS_COLORS", () => {
  it("maps each meaning to the status ramp's own CSS variable, so every panel's chart draws it the same", () => {
    expect(STATUS_COLORS).toEqual({
      done: "var(--color-status-done)",
      danger: "var(--color-status-danger)",
      warn: "var(--color-status-warn)",
      pending: "var(--color-status-pending)",
      active: "var(--color-status-active)",
    })
  })
})
