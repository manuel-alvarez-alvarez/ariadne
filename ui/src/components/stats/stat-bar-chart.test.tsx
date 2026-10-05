// @vitest-environment jsdom

/**
 * `StatBarChart` draws an `sr-only` table beside the chart. `sr-only` is
 * `position: absolute`, so with no positioned ancestor the table lays out
 * against the document instead of its own box, stretching the page past the
 * viewport — the Stats screen's second scrollbar.
 */

import { render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import type { ChartConfig } from "@/components/ui/chart"
import { StatBarChart } from "./stat-bar-chart"

const CONFIG: ChartConfig = {
  count: { label: "Count", color: "red" },
}

it("gives the sr-only table a positioned ancestor", () => {
  render(
    <StatBarChart
      data={[{ label: "stub:model", count: 3 }]}
      config={CONFIG}
      bars={["count"]}
      tooltip={() => null}
      caption="Stub chart"
      columns={[{ header: "Count", render: (row) => row.count }]}
    />,
  )

  const table = screen.getByRole("table", { hidden: true })
  expect(table.closest(".relative")).not.toBeNull()
})
