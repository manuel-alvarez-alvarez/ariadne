// @vitest-environment jsdom

import { renderHook } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import { useOpenExternalLinks } from "@/hooks/use-open-external-links"

import { openExternalUrl } from "./open-external"

const { isTauri, openUrl } = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  openUrl: vi.fn(),
}))

vi.mock("@tauri-apps/api/core", () => ({ isTauri }))
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }))

afterEach(() => {
  vi.restoreAllMocks()
})

/**
 * The same route a rendered `<a href>` takes, for a target with no link of
 * its own to render: the app's click handler is what actually reaches the
 * Tauri opener, so this proves the anchor this helper builds is real enough
 * for that handler to catch, not only that `window.open` was asked.
 */
it("reaches the forge through the app's own external-link handler", () => {
  isTauri.mockReturnValue(true)
  renderHook(() => useOpenExternalLinks())

  openExternalUrl("https://github.com/acme/widgets/pull/7")

  expect(openUrl).toHaveBeenCalledWith("https://github.com/acme/widgets/pull/7")
})

it("drops the anchor it built once the click is handled", () => {
  isTauri.mockReturnValue(true)
  renderHook(() => useOpenExternalLinks())

  openExternalUrl("https://github.com/acme/widgets/pull/7")

  expect(document.querySelector("a")).toBeNull()
})
