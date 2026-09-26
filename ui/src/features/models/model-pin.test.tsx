// @vitest-environment jsdom

import { render, screen } from "@testing-library/react"
import type { ComponentProps } from "react"
import { expect, it } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { ModelPin } from "./model-pin"

const MODEL = "claude-acp:claude-opus-4-5-20251101"
const PIN = `${MODEL} @ high`

function mount(props: ComponentProps<typeof ModelPin>) {
  return render(
    <TooltipProvider delay={0}>
      <ModelPin {...props} />
    </TooltipProvider>,
  )
}

it("wraps every character of a pin without a tooltip", () => {
  const { container } = mount({ model: MODEL, effort: "high", mode: "wrap" })
  const pin = screen.getByText(PIN)

  expect(pin.classList).toContain("break-all")
  expect(container.querySelector("[data-slot='tooltip-trigger']")).toBeNull()
})

it("keeps an effort visible beside a middle-cut model and opens the whole pin on focus", async () => {
  const { container } = mount({ model: MODEL, effort: "high", mode: "row" })
  const trigger = container.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")
  if (!trigger) throw new Error("no model pin tooltip trigger")

  const truncated = trigger.querySelector<HTMLElement>("[title]")
  if (!truncated) throw new Error("no middle-truncated model")
  const [head, tail] = [...truncated.children] as HTMLElement[]

  expect(trigger.classList).toContain("whitespace-nowrap")
  expect(trigger.textContent).toBe(PIN)
  expect(truncated.classList).toContain("overflow-hidden")
  expect(head?.classList).toContain("truncate")
  expect(head?.textContent).toBe("claude-acp:claude-opus-4-5-")
  expect(tail?.textContent).toBe("20251101")

  trigger.focus()
  expect(document.activeElement).toBe(trigger)
  const popup = await screen.findByText(PIN)
  expect(popup.closest("[data-slot='tooltip-content']")).not.toBeNull()
})

it("leaves out an empty effort and its at sign", () => {
  const { container } = mount({ model: MODEL, effort: "", mode: "row" })
  const trigger = container.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")

  expect(trigger?.textContent).toBe(MODEL)
  expect(trigger?.textContent).not.toContain("@")
})
