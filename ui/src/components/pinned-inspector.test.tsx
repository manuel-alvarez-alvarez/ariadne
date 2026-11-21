// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { expect, it, vi } from "vitest"
import { renderScreen } from "@/test/harness"
import { InspectorLayoutContext } from "./inspector-layout-context"
import { PanelSheet } from "./panel-sheet"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog"

it("reserves overview space only when pinned and preserves content through fallback and expansion", () => {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 })
  const reserve = vi.fn()
  renderScreen(
    <InspectorLayoutContext.Provider value={{ reserve, focusOverview: () => {} }}>
      <PanelSheet onClose={() => {}}>
        <textarea aria-label="Draft" defaultValue="Keep this" />
      </PanelSheet>
    </InspectorLayoutContext.Provider>,
  )
  const draft = screen.getByRole("textbox")
  const pane = screen.getByRole("region")
  expect(document.querySelector('[data-slot="docked-pane-scrim"]')).toBeTruthy()
  fireEvent.click(screen.getByRole("button", { name: "Pin inspector" }))
  expect(reserve).toHaveBeenLastCalledWith(576)
  expect(pane.getAttribute("data-pinned")).toBe("true")
  expect(document.querySelector('[data-slot="docked-pane-scrim"]')).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: "Expand the panel" }))
  expect(reserve).toHaveBeenLastCalledWith(0)
  fireEvent.click(screen.getByRole("button", { name: "Collapse the panel" }))
  expect(reserve).toHaveBeenLastCalledWith(576)
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1000 })
  fireEvent(window, new Event("resize"))
  expect(reserve).toHaveBeenLastCalledWith(0)
  expect(screen.queryByRole("button", { name: "Unpin inspector" })).toBeNull()
  expect(document.querySelector('[data-slot="docked-pane-scrim"]')).toBeTruthy()
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 })
  fireEvent(window, new Event("resize"))
  expect(reserve).toHaveBeenLastCalledWith(576)
  expect(screen.getByRole("textbox")).toBe(draft)
  expect((draft as HTMLTextAreaElement).value).toBe("Keep this")
  fireEvent.click(screen.getByRole("button", { name: "Unpin inspector" }))
  expect(reserve).toHaveBeenLastCalledWith(0)
})

it("moves focus between regions and lets Tab leave the pinned inspector", async () => {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 })
  const overview = document.createElement("button")
  overview.textContent = "Overview action"
  document.body.append(overview)
  const close = vi.fn()
  renderScreen(
    <InspectorLayoutContext.Provider
      value={{ reserve: () => {}, focusOverview: () => overview.focus() }}
    >
      <PanelSheet onClose={close}>
        <button type="button">Inside</button>
      </PanelSheet>
    </InspectorLayoutContext.Provider>,
  )
  const user = userEvent.setup()
  fireEvent.click(screen.getByRole("button", { name: "Pin inspector" }))
  await user.click(screen.getByRole("button", { name: "Focus overview" }))
  expect(document.activeElement).toBe(overview)
  fireEvent.keyDown(overview, { key: "Escape" })
  expect(close).not.toHaveBeenCalled()
  await user.click(screen.getByRole("button", { name: "Focus inspector" }))
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Focus overview" }))
  screen.getByRole("separator").focus()
  await user.tab()
  expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Close" }))
  overview.remove()
})

it("keeps nested confirmations modal and gives Escape to the dialog first", async () => {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 })
  const close = vi.fn()
  function Content() {
    const [open, setOpen] = useState(false)
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Confirm action
        </button>
        <Dialog open={open} onOpenChange={setOpen}>
          <DialogContent>
            <DialogTitle>Confirmation</DialogTitle>
            <DialogDescription>Check the action.</DialogDescription>
          </DialogContent>
        </Dialog>
      </>
    )
  }
  renderScreen(
    <InspectorLayoutContext.Provider value={{ reserve: () => {}, focusOverview: () => {} }}>
      <PanelSheet onClose={close}>
        <Content />
      </PanelSheet>
    </InspectorLayoutContext.Provider>,
  )
  const user = userEvent.setup()
  fireEvent.click(screen.getByRole("button", { name: "Pin inspector" }))
  await user.click(screen.getByRole("button", { name: "Confirm action" }))
  await waitFor(() =>
    expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(true),
  )
  await user.tab()
  await waitFor(() =>
    expect(screen.getByRole("dialog").contains(document.activeElement)).toBe(true),
  )
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("dialog")).toBeNull()
  expect(close).not.toHaveBeenCalled()
  expect(screen.getByRole("region")).toBeTruthy()
})
