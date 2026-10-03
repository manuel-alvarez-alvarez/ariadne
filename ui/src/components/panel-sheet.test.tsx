// @vitest-environment jsdom

import { act, fireEvent, screen } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { useSettingsStore } from "@/stores/settings"
import { renderScreen } from "@/test/harness"
import { PanelSheet } from "./panel-sheet"

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ panelWidth: 576 })
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1280 })
})

it("clamps a dragged pane width and restores it from settings on remount", async () => {
  const pane = <PanelSheet onClose={() => {}}>Content</PanelSheet>
  const { rerender } = renderScreen(pane)
  const handle = screen.getByRole("separator", { name: "Resize details" })
  expect(handle.getAttribute("aria-valuenow")).toBe("576")
  fireEvent.mouseDown(handle, { clientX: 704 })
  fireEvent.mouseMove(window, { clientX: 1000 })
  expect(handle.getAttribute("aria-valuenow")).toBe("384")
  fireEvent.mouseMove(window, { clientX: 0 })
  expect(handle.getAttribute("aria-valuenow")).toBe("768")
  fireEvent.mouseMove(window, { clientX: 640 })
  fireEvent.mouseUp(window)
  expect(handle.getAttribute("aria-valuenow")).toBe("640")
  rerender(null)
  // Reset only memory, then hydrate the stored choice as a new window does.
  const stored = localStorage.getItem("ariadne.settings")
  useSettingsStore.setState({ panelWidth: 576 })
  localStorage.setItem("ariadne.settings", stored ?? "")
  await act(() => useSettingsStore.persist.rehydrate())
  rerender(pane)
  expect(screen.getByRole("separator").getAttribute("aria-valuenow")).toBe("640")
})

it("leaves Escape on the board alone and returns focus when the pane closes", async () => {
  const opener = document.createElement("button")
  document.body.append(opener)
  opener.focus()
  const { rerender } = renderScreen(<PanelSheet onClose={() => rerender(null)}>Content</PanelSheet>)
  const pane = screen.getByRole("region")
  expect(pane.contains(document.activeElement)).toBe(true)
  opener.focus()
  fireEvent.keyDown(opener, { key: "Escape" })
  expect(screen.getByRole("region")).toBe(pane)
  pane.focus()
  fireEvent.keyDown(pane, { key: "Escape" })
  await act(async () => {})
  expect(screen.queryByRole("region")).toBeNull()
  expect(document.activeElement).toBe(opener)
  opener.remove()
})

it("clamps the pane when the window shrinks and supports keyboard resizing", () => {
  renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const handle = screen.getByRole("separator")
  fireEvent.keyDown(handle, { key: "End" })
  expect(handle.getAttribute("aria-valuenow")).toBe("768")
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 })
  fireEvent(window, new Event("resize"))
  expect(handle.getAttribute("aria-valuenow")).toBe("600")
  fireEvent.keyDown(handle, { key: "Home" })
  expect(handle.getAttribute("aria-valuenow")).toBe("384")
  fireEvent.keyDown(handle, { key: "ArrowLeft" })
  expect(handle.getAttribute("aria-valuenow")).toBe("400")
})
