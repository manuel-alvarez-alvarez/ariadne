// @vitest-environment jsdom

/**
 * The one thing worth a test of its own: a version-0 blob — what every prior
 * release left in `localStorage` — still carries its seat filter forward
 * under the renamed field, and the blob on disk is rewritten so the next
 * load skips the migration.
 */

import { beforeEach, describe, expect, it, vi } from "vitest"

const STORAGE_KEY = "ariadne.settings"

beforeEach(() => {
  localStorage.clear()
  vi.resetModules()
})

describe("useSettingsStore", () => {
  it("migrates a version-0 sessionRoleFilter onto sessionSeatFilter", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        state: {
          baseUrl: "http://127.0.0.1:7676",
          diagnosticsEnabled: true,
          goalStatusFilter: "",
          sessionStatusFilter: "",
          sessionRoleFilter: "agent",
          sessionGoalFilter: "",
          sessionTaskFilter: "",
          sidebarCollapsed: false,
          panelWidth: 576,
        },
        version: 0,
      }),
    )

    const { useSettingsStore } = await import("./settings")
    await useSettingsStore.persist.rehydrate()

    expect(useSettingsStore.getState().sessionSeatFilter).toBe("agent")

    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}")
    expect(stored.version).toBe(1)
    expect(stored.state.sessionSeatFilter).toBe("agent")
    expect(stored.state.sessionRoleFilter).toBeUndefined()
  })
})
