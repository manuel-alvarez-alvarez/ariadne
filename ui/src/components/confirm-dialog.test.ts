import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import { type ConfirmFlow, isSettling } from "./confirm-dialog"

/** Nothing has happened yet: the trigger is on screen, the dialog is not. */
const IDLE: ConfirmFlow = { open: false, pending: false, error: null }

describe("isSettling", () => {
  it("is false when nothing is happening, so terminal rows keep no actions", () => {
    expect(isSettling(IDLE)).toBe(false)
    expect(isSettling(IDLE, IDLE)).toBe(false)
    expect(isSettling()).toBe(false)
  })

  it("holds the screen for whichever flow is busy, not just the first", () => {
    expect(isSettling(IDLE, { ...IDLE, open: true })).toBe(true)
    expect(isSettling({ ...IDLE, pending: true }, IDLE)).toBe(true)
  })

  it("keeps the dialog mounted while the request is in flight", () => {
    // This is the step that used to unmount: the optimistic flip has already
    // made the row terminal, so the caller's own `canCancel` is false here.
    expect(isSettling({ open: true, pending: true, error: null })).toBe(true)
  })

  it("keeps it mounted after a refusal, so the error has somewhere to render", () => {
    const refused: ConfirmFlow = {
      open: true,
      pending: false,
      error: new ApiError({ status: 409, code: "illegal_transition", message: "cannot cancel" }),
    }
    expect(isSettling(refused)).toBe(true)
  })

  it("treats a reset mutation as settled, whichever way the error was cleared", () => {
    expect(isSettling({ open: false, pending: false, error: undefined })).toBe(false)
    expect(isSettling({ open: false, pending: false })).toBe(false)
  })
})
