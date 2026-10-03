/**
 * One horizontal bar chart of the Stats screen: a row per model, seat or
 * tool, sorted by the caller's own first series descending, long ids read as
 * labels at the left.
 *
 * A group in `bars` is either one key, drawn as a bar of its own, or several
 * keys that stack into one bar — the way "exhausted", "automatic" and
 * "other" stack into one bar per model while "arrivals" stands beside it.
 * Every key's colour comes from `config`, built by the caller off
 * {@link import("./status-colors").STATUS_COLORS}, so the same meaning reads
 * as the same colour in every panel.
 *
 * The `sr-only` table beside the chart carries the same numbers a sighted
 * reader gets from the bars and the tooltip, for a screen reader and for a
 * test — rendering is proof enough there, where a canvas would need a pixel
 * read.
 */

import type { ReactNode } from "react"
import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts"

import {
  type ChartConfig,
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
} from "@/components/ui/chart"

/** One bar of its own, or several keys stacked into one. */
type StatBarGroup = string | string[]

const ROW_HEIGHT_PX = 28
const MIN_HEIGHT_PX = 96

export function StatBarChart<T extends { label: string }>({
  data,
  config,
  bars,
  tooltip,
  caption,
  columns,
}: {
  data: T[]
  config: ChartConfig
  bars: StatBarGroup[]
  /** The tooltip's body for one row, beyond the series values already shown. */
  tooltip: (row: T) => ReactNode
  /** The `sr-only` table's caption: what the chart is a chart of. */
  caption: string
  columns: { header: string; render: (row: T) => ReactNode }[]
}) {
  const height = Math.max(MIN_HEIGHT_PX, data.length * ROW_HEIGHT_PX)
  const legend = bars.some((group) => Array.isArray(group) && group.length > 1) || bars.length > 1
  return (
    <div className="flex flex-col gap-2">
      {/* `block`, not the base `flex justify-center`: a centred flex row leaves
          its `width: 100%` child to shrink-wrap instead of filling it. */}
      <ChartContainer config={config} className="block aspect-auto w-full" style={{ height }}>
        <BarChart data={data} layout="vertical" margin={{ left: 0, right: 8, top: 4, bottom: 4 }}>
          <CartesianGrid horizontal={false} stroke="var(--color-border)" strokeOpacity={0.5} />
          <YAxis
            dataKey="label"
            type="category"
            axisLine={false}
            tickLine={false}
            width={160}
            className="font-mono text-xs"
          />
          {/* Hidden: exact values are in the tooltip and the sr-only table below. */}
          <XAxis type="number" hide />
          <ChartTooltip
            cursor={false}
            content={({ active, payload }) =>
              active && payload?.length ? (
                <div className="rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl">
                  {tooltip(payload[0]?.payload as T)}
                </div>
              ) : null
            }
          />
          {legend ? <ChartLegend content={<ChartLegendContent />} /> : null}
          {bars.map((group, groupIndex) => {
            const keys = Array.isArray(group) ? group : [group]
            const stackId = Array.isArray(group) ? `stack-${groupIndex}` : undefined
            return keys.map((key, keyIndex) => (
              <Bar
                key={key}
                dataKey={key}
                stackId={stackId}
                fill={`var(--color-${key})`}
                stroke="var(--color-background)"
                strokeWidth={keys.length > 1 ? 2 : 0}
                radius={keyIndex === keys.length - 1 ? [0, 4, 4, 0] : 0}
              />
            ))
          })}
        </BarChart>
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
            <tr key={row.label}>
              {columns.map((column) => (
                <td key={column.header}>{column.render(row)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
