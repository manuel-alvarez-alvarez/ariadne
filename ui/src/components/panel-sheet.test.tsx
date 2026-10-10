// @vitest-environment jsdom

import { act, fireEvent, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { SCRIM } from "@/components/ui/dialog"
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
  fireEvent.pointerDown(handle, { clientX: 704, button: 0, pointerId: 1 })
  fireEvent.pointerMove(handle, { clientX: 1000, pointerId: 1 })
  expect(handle.getAttribute("aria-valuenow")).toBe("384")
  fireEvent.pointerMove(handle, { clientX: 0, pointerId: 1 })
  expect(handle.getAttribute("aria-valuenow")).toBe("768")
  fireEvent.pointerMove(handle, { clientX: 640, pointerId: 1 })
  fireEvent.pointerUp(handle, { clientX: 640, pointerId: 1 })
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

it("writes a dragged width to settings once, on release", () => {
  const writes: number[] = []
  const stop = useSettingsStore.subscribe((state, before) => {
    if (state.panelWidth !== before.panelWidth) writes.push(state.panelWidth)
  })
  renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const handle = screen.getByRole("separator")
  fireEvent.pointerDown(handle, { clientX: 704, button: 0, pointerId: 1 })
  fireEvent.pointerMove(handle, { clientX: 680, pointerId: 1 })
  fireEvent.pointerMove(handle, { clientX: 660, pointerId: 1 })
  fireEvent.pointerMove(handle, { clientX: 640, pointerId: 1 })
  expect(handle.getAttribute("aria-valuenow")).toBe("640")
  expect(writes).toEqual([])
  fireEvent.pointerUp(handle, { clientX: 640, pointerId: 1 })
  expect(writes).toEqual([640])
  stop()
})

it("resets the pane to 36rem on a double-click of the handle", () => {
  useSettingsStore.setState({ panelWidth: 700 })
  renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const handle = screen.getByRole("separator")
  expect(handle.getAttribute("aria-valuenow")).toBe("700")
  fireEvent.doubleClick(handle)
  expect(handle.getAttribute("aria-valuenow")).toBe("576")
  expect(useSettingsStore.getState().panelWidth).toBe(576)
})

it("closes the pane from a click on its scrim, as from its close button", async () => {
  const onClose = vi.fn()
  renderScreen(<PanelSheet onClose={onClose}>Content</PanelSheet>)
  const user = userEvent.setup()
  const scrim = document.querySelector('[data-slot="docked-pane-scrim"]')
  if (!scrim) throw new Error("no scrim over the screen")
  await user.click(scrim)
  expect(onClose).toHaveBeenCalledTimes(1)
  await user.click(screen.getByRole("button", { name: "Close" }))
  expect(onClose).toHaveBeenCalledTimes(2)
})

it("puts the close button first in the pane's tab order", async () => {
  renderScreen(
    <PanelSheet onClose={() => {}}>
      <button type="button">Inside</button>
    </PanelSheet>,
  )
  const pane = screen.getByRole("region")
  pane.focus()
  await userEvent.setup().tab()
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }))
})

it("keeps Tab inside the pane while it is open", async () => {
  const outside = document.createElement("button")
  document.body.append(outside)
  renderScreen(
    <PanelSheet onClose={() => {}}>
      <button type="button">Inside</button>
    </PanelSheet>,
  )
  const user = userEvent.setup()
  const close = screen.getByRole("button", { name: "Close" })
  const last = screen.getByRole("separator")
  last.focus()
  await user.tab()
  expect(document.activeElement).toBe(close)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(last)
  outside.remove()
})

it("keeps the dragged width when the pane closes in mid-drag", () => {
  const { rerender } = renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const handle = screen.getByRole("separator")
  fireEvent.pointerDown(handle, { clientX: 704, button: 0, pointerId: 1 })
  fireEvent.pointerMove(handle, { clientX: 640, pointerId: 1 })
  rerender(null)
  expect(useSettingsStore.getState().panelWidth).toBe(640)
})

it("darkens the screen with the dialog's own SCRIM token", () => {
  renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const scrim = document.querySelector('[data-slot="docked-pane-scrim"]')
  for (const token of SCRIM.split(" ")) expect(scrim?.className).toContain(token)
  expect(scrim?.className).not.toContain("bg-black/20")
})

it("expands the pane to near-fullscreen and back on a second press", async () => {
  useSettingsStore.setState({ panelWidth: 576 })
  renderScreen(<PanelSheet onClose={() => {}}>Content</PanelSheet>)
  const user = userEvent.setup()
  const pane = screen.getByRole("region")
  expect(pane.className).toContain("md:w-[var(--pane-width)]")
  expect(screen.getByRole("separator", { name: "Resize details" })).toBeTruthy()

  await user.click(screen.getByRole("button", { name: "Expand the panel" }))
  expect(pane.className).toContain("h-[calc(100dvh-2rem)]")
  expect(pane.className).toContain("w-[calc(100vw-2rem)]")
  expect(pane.className).not.toContain("md:w-[var(--pane-width)]")
  expect(screen.queryByRole("separator", { name: "Resize details" })).toBeNull()

  await user.click(screen.getByRole("button", { name: "Collapse the panel" }))
  expect(pane.className).not.toContain("h-[calc(100dvh-2rem)]")
  expect(pane.className).toContain("md:w-[var(--pane-width)]")
  expect(
    screen.getByRole("separator", { name: "Resize details" }).getAttribute("aria-valuenow"),
  ).toBe("576")
})

it("still closes the expanded pane from its scrim and from Escape", async () => {
  const onClose = vi.fn()
  renderScreen(<PanelSheet onClose={onClose}>Content</PanelSheet>)
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Expand the panel" }))
  const scrim = document.querySelector('[data-slot="docked-pane-scrim"]')
  if (!scrim) throw new Error("no scrim over the screen")
  await user.click(scrim)
  expect(onClose).toHaveBeenCalledTimes(1)
  fireEvent.keyDown(screen.getByRole("region"), { key: "Escape" })
  expect(onClose).toHaveBeenCalledTimes(2)
})

it("keeps Tab inside the expanded pane, past the close and toggle, with no resize handle to land on", async () => {
  const outside = document.createElement("button")
  document.body.append(outside)
  renderScreen(
    <PanelSheet onClose={() => {}}>
      <button type="button">Inside</button>
    </PanelSheet>,
  )
  const user = userEvent.setup()
  await user.click(screen.getByRole("button", { name: "Expand the panel" }))
  const close = screen.getByRole("button", { name: "Close" })
  const toggle = screen.getByRole("button", { name: "Collapse the panel" })
  const inside = screen.getByRole("button", { name: "Inside" })
  inside.focus()
  await user.tab()
  expect(document.activeElement).toBe(close)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(inside)
  await user.tab({ shift: true })
  expect(document.activeElement).toBe(toggle)
  outside.remove()
})
