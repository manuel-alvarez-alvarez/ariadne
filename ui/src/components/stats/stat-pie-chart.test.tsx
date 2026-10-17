// @vitest-environment jsdom

/**
 * `StatPieChart` draws an `sr-only` table beside the chart. `sr-only` is
 * `position: absolute`, so with no positioned ancestor the table lays out
 * against the document instead of its own box, stretching the page past the
 * viewport — the Stats screen's second scrollbar.
 */

import { render, screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import type { ChartConfig } from "@/components/ui/chart"
import { StatPieChart } from "./stat-pie-chart"

const CONFIG: ChartConfig = {
  a: { label: "Slice A", color: "red" },
  b: { label: "Slice B", color: "blue" },
}

it("gives the sr-only table a positioned ancestor", () => {
  render(
    <StatPieChart
      data={[
        { key: "a", value: 3 },
        { key: "b", value: 1 },
      ]}
      config={CONFIG}
      caption="Stub chart"
      columns={[{ header: "Value", render: (row) => row.value }]}
    />,
  )

  const table = screen.getByRole("table", { hidden: true })
  expect(table.closest(".relative")).not.toBeNull()
})

it("labels each slice's row with its own config label, not its raw key", () => {
  render(
    <StatPieChart
      data={[
        { key: "a", value: 3 },
        { key: "b", value: 1 },
      ]}
      config={CONFIG}
      caption="Stub chart"
      columns={[{ header: "Value", render: (row) => row.value }]}
    />,
  )

  const table = screen.getByRole("table", { hidden: true })
  expect(within(table).queryByText("a")).toBeNull()
  expect(screen.getByText("Slice A")).toBeDefined()
  expect(screen.getByText("Slice B")).toBeDefined()
})
