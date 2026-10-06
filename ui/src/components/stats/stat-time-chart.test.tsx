// @vitest-environment jsdom

/**
 * `StatTimeChart` draws an `sr-only` table beside the chart. `sr-only` is
 * `position: absolute`, so with no positioned ancestor the table lays out
 * against the document instead of its own box, stretching the page past the
 * viewport — the Stats screen's second scrollbar.
 */

import { render, screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import type { ChartConfig } from "@/components/ui/chart"
import { StatTimeChart } from "./stat-time-chart"

const CONFIG: ChartConfig = {
  count: { label: "Count", color: "red" },
}

it("gives the sr-only table a positioned ancestor", () => {
  render(
    <StatTimeChart
      data={[{ bucket: "2026-09-28T00:00:00Z", count: 3 }]}
      config={CONFIG}
      keys={["count"]}
      caption="Stub chart"
    />,
  )

  const table = screen.getByRole("table", { hidden: true })
  expect(table.closest(".relative")).not.toBeNull()
})

it("labels an hour bucket as a time, with the date added, not a bare date", () => {
  render(
    <StatTimeChart
      data={[{ bucket: "2026-10-06T14:00:00Z", count: 3 }]}
      config={CONFIG}
      keys={["count"]}
      caption="Stub chart"
      bucket="hour"
    />,
  )

  const table = screen.getByRole("table", { hidden: true })
  expect(within(table).getByText("14:00 · Oct 6")).toBeDefined()
  expect(within(table).queryByText("Oct 6")).toBeNull()
})
