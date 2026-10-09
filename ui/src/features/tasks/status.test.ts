import { describe, expect, it } from "vitest"

import { canCancel, canEdit, canRetry, TASK_STATUS_META } from "./status"

describe("workflow task status", () => {
  it("uses the contracted lifecycle", () => {
    expect(Object.keys(TASK_STATUS_META)).toEqual([
      "pending",
      "ready",
      "in_progress",
      "finished",
      "cancelled",
      "failed",
    ])
  })

  it("allows user actions for the new lifecycle", () => {
    expect(canEdit("pending")).toBe(true)
    expect(canEdit("in_progress")).toBe(false)
    expect(canCancel("finished")).toBe(false)
    expect(canRetry("failed")).toBe(true)
  })
})
