// @vitest-environment jsdom

/**
 * `StatExplain` is the one place a figure's meaning is wired up: a short
 * sentence that shows on hover and on keyboard focus, and reaches a screen
 * reader as the trigger's accessible description rather than a sighted-only
 * hint. Both paths are asserted here, the way `when.test.tsx` already pins
 * them for the app's other tooltip-backed hint.
 */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import { TooltipProvider } from "@/components/ui/tooltip"
import { StatExplain } from "./stat-explain"

afterEach(cleanup)

const EXPLAIN = "Rounds/task: the mean number of review requests on each finished task."

function mount(ui: React.ReactNode) {
  render(<TooltipProvider delay={0}>{ui}</TooltipProvider>)
}

it("shows the explanation on hover, and names it as the trigger's accessible description", async () => {
  const user = userEvent.setup()
  mount(<StatExplain explain={EXPLAIN}>ROUNDS/TASK</StatExplain>)

  const trigger = screen.getByText("ROUNDS/TASK")
  expect(screen.queryByText(EXPLAIN)).toBeNull()

  await user.hover(trigger)
  const content = await screen.findByText(EXPLAIN)
  expect(trigger.getAttribute("aria-describedby")).toBe(content.id)
})

it("shows the explanation on keyboard focus too, not only on hover", async () => {
  const user = userEvent.setup()
  mount(<StatExplain explain={EXPLAIN}>ROUNDS/TASK</StatExplain>)

  await user.tab()
  expect(await screen.findByText(EXPLAIN)).toBeDefined()
})
