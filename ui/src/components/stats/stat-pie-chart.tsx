/**
 * One donut chart of the Stats screen: a slice per row, summing to the
 * whole, each coloured and labelled off the caller's own `config` — the one
 * widget of the five that reads a composition or a share at a glance rather
 * than a trend or a ranking.
 *
 * Every slice's colour comes from `config`, built by the caller off
 * {@link import("./status-colors").STATUS_COLORS}, so the same meaning reads
 * as the same colour in every panel. The `sr-only` table beside it carries
 * the same numbers a sighted reader gets from the slices and the tooltip,
 * for a screen reader and for a test, the same way `StatBarChart` and
 * `StatTimeChart` keep one. The legend below is `StatChartLegend` rather
 * than recharts' own, for the reason both already give: recharts renders
 * nothing, its own legend included, until its `ResponsiveContainer` measures
 * a real box, and jsdom never gives it one.
 */

import type { ReactNode } from "react"
import { Cell, Pie, PieChart } from "recharts"

import { type ChartConfig, ChartContainer, ChartTooltip } from "@/components/ui/chart"
import { StatChartLegend } from "./stat-chart-legend"
import { CHART_HEIGHT_PX } from "./stat-time-chart"

export function StatPieChart<T extends { key: string; value: number }>({
  data,
  config,
  caption,
  columns,
  valueFormatter = String,
}: {
  /** One row per slice, its `key` naming its own entry in `config`. */
  data: T[]
  config: ChartConfig
  /** The `sr-only` table's caption: what the chart is a chart of. */
  caption: string
  columns: { header: string; render: (row: T) => ReactNode }[]
  /** How a slice's value reads in the tooltip and the `sr-only` table. */
  valueFormatter?: (value: number) => string
}) {
  return (
    <div className="flex flex-1 flex-col gap-1">
      {/* `relative`: the lone positioned ancestor the `sr-only` table below
          needs, the same reason `StatBarChart` and `StatTimeChart` give. */}
      <div className="relative flex-1" style={{ minHeight: CHART_HEIGHT_PX }}>
        <ChartContainer config={config} className="absolute inset-0 block aspect-auto">
          <PieChart margin={{ top: 4, right: 4, bottom: 4, left: 4 }}>
            <ChartTooltip
              cursor={false}
              content={({ active, payload }) => {
                const row = payload?.[0]?.payload as T | undefined
                return active && row ? (
                  <div className="rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl">
                    <p className="font-medium">{config[row.key]?.label ?? row.key}</p>
                    <p>{valueFormatter(row.value)}</p>
                  </div>
                ) : null
              }}
            />
            <Pie
              data={data}
              dataKey="value"
              nameKey="key"
              innerRadius="55%"
              outerRadius="90%"
              strokeWidth={2}
              stroke="var(--color-background)"
              isAnimationActive={false}
            >
              {data.map((row) => (
                <Cell key={row.key} fill={`var(--color-${row.key})`} />
              ))}
            </Pie>
          </PieChart>
        </ChartContainer>
        <table className="sr-only">
          <caption>{caption}</caption>
          <thead>
            <tr>
              {columns.map((column) => (
                <th key={column.header}>{column.header}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.map((row) => (
              <tr key={row.key}>
                {columns.map((column) => (
                  <td key={column.header}>{column.render(row)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <StatChartLegend config={config} keys={data.map((row) => row.key)} />
    </div>
  )
}
