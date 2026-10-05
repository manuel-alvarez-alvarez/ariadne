// @vitest-environment jsdom

/**
 * `StatChartLegend` draws every chart's legend as a sibling of the measured
 * chart area, outside recharts' own `ResponsiveContainer` — the one piece of
 * a chart that is not gated behind jsdom never measuring a real box. Each
 * entry with an `explain` carries it as a tooltip, on hover and on focus; one
 * with none renders plainly.
 */

import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { ChartConfig } from "@/components/ui/chart"
import { TooltipProvider } from "@/components/ui/tooltip"
import { StatChartLegend } from "./stat-chart-legend"

const CONFIG: ChartConfig = {
  finished: { label: "Finished", color: "green", explain: "Finished: a stub explanation." },
  other: { label: "Other", color: "grey" },
}

function mount(ui: React.ReactNode) {
  render(<TooltipProvider delay={0}>{ui}</TooltipProvider>)
}

it("explains an entry with an explanation, on hover", async () => {
  const user = userEvent.setup()
  mount(<StatChartLegend config={CONFIG} keys={["finished", "other"]} />)

  const trigger = screen.getByText("Finished")
  await user.hover(trigger)
  const content = await screen.findByText("Finished: a stub explanation.")
  expect(trigger.getAttribute("aria-describedby")).toBe(content.id)
})

it("renders an entry with no explanation plainly, with no trigger to open", () => {
  mount(<StatChartLegend config={CONFIG} keys={["finished", "other"]} />)

  const label = screen.getByText("Other")
  expect(label.getAttribute("aria-describedby")).toBeNull()
  expect(label.closest("[data-slot='tooltip-trigger']")).toBeNull()
})
