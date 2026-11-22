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
    <InspectorLayoutContext.Provider value={{ reserve }}>
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

it("shows an icon-only pin control without focus buttons", () => {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 })
  renderScreen(
    <InspectorLayoutContext.Provider value={{ reserve: () => {} }}>
      <PanelSheet onClose={() => {}}>Details</PanelSheet>
    </InspectorLayoutContext.Provider>,
  )
  const pin = screen.getByRole("button", { name: "Pin inspector" })
  expect(pin.textContent).toBe("")
  expect(pin.getAttribute("title")).toBe("Pin inspector")
  expect(pin.getAttribute("aria-pressed")).toBe("false")
  fireEvent.click(pin)
  expect(pin.getAttribute("aria-label")).toBe("Unpin inspector")
  expect(pin.getAttribute("title")).toBe("Unpin inspector")
  expect(pin.getAttribute("aria-pressed")).toBe("true")
  expect(screen.queryByRole("button", { name: "Focus overview" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Focus inspector" })).toBeNull()
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
    <InspectorLayoutContext.Provider value={{ reserve: () => {} }}>
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
