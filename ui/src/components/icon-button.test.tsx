// @vitest-environment jsdom

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it, vi } from "vitest"

import { IconButton } from "@/components/icon-button"
import { TooltipProvider } from "@/components/ui/tooltip"

function mount(props: Partial<Parameters<typeof IconButton>[0]> = {}) {
  return render(
    <TooltipProvider>
      <IconButton icon={<svg data-testid="glyph" />} label="Retry" {...props} />
    </TooltipProvider>,
  )
}

it("names the button by its label and draws no text", () => {
  mount()

  const button = screen.getByRole("button", { name: "Retry" })
  expect(button.textContent).toBe("")
  expect(button.querySelector("[data-testid=glyph]")).not.toBeNull()
})

it("shows the label as the tooltip when the button takes focus", async () => {
  const user = userEvent.setup()
  mount()

  await user.tab()

  expect(
    (await screen.findByText("Retry", { selector: "[data-slot=tooltip-content]" })).textContent,
  ).toBe("Retry")
})

it("shows a tooltip of its own without changing the accessible name", async () => {
  const user = userEvent.setup()
  mount({ label: "Search", tooltip: "Search ⌘K" })

  await user.tab()

  expect(await screen.findByText("Search ⌘K")).not.toBeNull()
  expect(screen.getByRole("button", { name: "Search" })).not.toBeNull()
})

it("shows a mark only when it has one", () => {
  const { rerender } = mount({ mark: "New output" })
  expect(screen.getByRole("status").textContent).toBe("New output")

  rerender(
    <TooltipProvider>
      <IconButton icon={<svg />} label="Retry" />
    </TooltipProvider>,
  )
  expect(screen.queryByRole("status")).toBeNull()
})

it("keeps its pressed state and click", async () => {
  const user = userEvent.setup()
  const onClick = vi.fn()
  mount({ "aria-pressed": true, onClick })

  const button = screen.getByRole("button", { name: "Retry" })
  expect(button.getAttribute("aria-pressed")).toBe("true")
  await user.click(button)
  expect(onClick).toHaveBeenCalledOnce()
})
