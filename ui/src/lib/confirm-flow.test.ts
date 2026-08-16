import { describe, expect, it } from "vitest"

import { ApiError } from "@/api"
import { type ConfirmFlow, isSettling } from "./confirm-flow"

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

  describe("the lifecycle an optimistic cancel goes through", () => {
    // Each step is what the component sees on one render, in order.
    const opened: ConfirmFlow = { open: true, pending: false, error: null }
    const confirmed: ConfirmFlow = { open: true, pending: true, error: null }
    const refused: ConfirmFlow = {
      open: true,
      pending: false,
      error: new ApiError({ status: 409, code: "illegal_transition", message: "cannot cancel" }),
    }
    const dismissed: ConfirmFlow = { open: false, pending: false, error: null }

    it("keeps the dialog mounted while the request is in flight", () => {
      // This is the step that used to unmount: the optimistic flip has already
      // made the row terminal, so the caller's own `canCancel` is false here.
      expect(isSettling(confirmed)).toBe(true)
    })

    it("keeps it mounted after a refusal, so the error has somewhere to render", () => {
      expect(isSettling(refused)).toBe(true)
    })

    it("lets go once the dialog is closed and the mutation is reset", () => {
      expect([opened, confirmed, refused].map((step) => isSettling(step))).toEqual([
        true,
        true,
        true,
      ])
      expect(isSettling(dismissed)).toBe(false)
    })

    it("still lets go when a success closes the dialog on a now-terminal row", () => {
      // Success is not an error and closes the dialog, so nothing holds the
      // actions on screen and the cancelled row correctly loses them.
      expect(isSettling({ open: false, pending: false, error: null })).toBe(false)
    })
  })

  it("treats a reset mutation as settled, whichever way the error was cleared", () => {
    expect(isSettling({ open: false, pending: false, error: undefined })).toBe(false)
    expect(isSettling({ open: false, pending: false })).toBe(false)
  })
})
