/**
 * One chart over time of the Stats screen: a bar per bucket — a day, or a
 * week from its Monday, as the daemon bucketed the span — with every key the
 * caller names stacked into it, on a date axis.
 *
 * Every key's colour comes from `config`, built by the caller off
 * {@link import("./status-colors").STATUS_COLORS}, so the same meaning reads
 * as the same colour in every section.
 *
 * The `sr-only` table beside the chart carries the same numbers a sighted
 * reader gets from the bars and the tooltip, for a screen reader and for a
 * test, the same way `StatBarChart` keeps one.
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

/** The least height of every chart of the screen, so charts side by side align. */
export const CHART_HEIGHT_PX = 160
/** The thickest a bar draws, however much room a grown chart gives it. */
export const MAX_BAR_PX = 32

/** The RFC 3339 start of a bucket as its axis label: `Sep 28`, read in UTC. */
function bucketLabel(start: string): string {
  return new Date(start).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    timeZone: "UTC",
  })
}

export function StatTimeChart<T extends { bucket: string }>({
  data,
  config,
  keys,
  caption,
  extra = [],
  tooltipExtra,
  valueFormatter = String,
}: {
  /** One row per bucket, oldest first, `bucket` its RFC 3339 start. */
  data: T[]
  config: ChartConfig
  /** The numeric keys of a row, stacked into its bar bottom up. */
  keys: (keyof T & string)[]
  /** The `sr-only` table's caption: what the chart is a chart of. */
  caption: string
  /** Keys shown in the tooltip and the `sr-only` table beyond the stacked
   * bars, labelled the same way — a count the bar does not draw but the
   * reader still wants beside it. */
  extra?: (keyof T & string)[]
  /** Extra lines for one bucket's tooltip, beyond its stacked keys — a
   * share the bar has no room to draw of its own, such as a cached one. */
  tooltipExtra?: (row: T) => ReactNode
  /** How a stacked value reads on the axis and in the tooltip: raw by
   * default, compact (`1.2M`) where the caller's values are tokens. */
  valueFormatter?: (value: number) => string
}) {
  return (
    // `relative`: the lone positioned ancestor the `sr-only` table below needs.
    // `sr-only` is `position: absolute`, and with no ancestor positioned it
    // places against the document instead of this box, stretching the page
    // past the viewport and drawing a second scrollbar. The chart is absolute
    // too, so this box takes its height from the card and never from the
    // chart: it grows into the room its row leaves, and shrinks back with it.
    <div className="relative flex-1" style={{ minHeight: CHART_HEIGHT_PX }}>
      <ChartContainer config={config} className="absolute inset-0 block aspect-auto">
        <BarChart data={data} margin={{ left: 0, right: 8, top: 4, bottom: 4 }}>
          <CartesianGrid vertical={false} stroke="var(--color-border)" strokeOpacity={0.5} />
          <XAxis
            dataKey="bucket"
            tickFormatter={bucketLabel}
            axisLine={false}
            tickLine={false}
            className="text-xs"
          />
          <YAxis
            axisLine={false}
            tickLine={false}
            width={40}
            className="text-xs"
            tickFormatter={valueFormatter}
          />
          <ChartTooltip
            cursor={false}
            content={({ active, payload }) => {
              const row = payload?.[0]?.payload as T | undefined
              return active && row ? (
                <div className="flex flex-col gap-1 rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl">
                  <p className="font-medium">{bucketLabel(row.bucket)}</p>
                  {[...keys, ...extra].map((key) => (
                    <p key={key}>
                      {config[key]?.label ?? key}: {valueFormatter(Number(row[key]))}
                    </p>
                  ))}
                  {tooltipExtra?.(row)}
                </div>
              ) : null
            }}
          />
          {keys.length > 1 ? <ChartLegend content={<ChartLegendContent />} /> : null}
          {keys.map((key, index) => (
            <Bar
              key={key}
              dataKey={String(key)}
              stackId="bucket"
              maxBarSize={MAX_BAR_PX}
              fill={`var(--color-${key})`}
              radius={index === keys.length - 1 ? [4, 4, 0, 0] : 0}
            />
          ))}
        </BarChart>
      </ChartContainer>
      <table className="sr-only">
        <caption>{caption}</caption>
        <thead>
          <tr>
            <th>Bucket</th>
            {[...keys, ...extra].map((key) => (
              <th key={key}>{config[key]?.label ?? key}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((row) => (
            <tr key={row.bucket}>
              <td>{bucketLabel(row.bucket)}</td>
              {[...keys, ...extra].map((key) => (
                <td key={key}>{valueFormatter(Number(row[key]))}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
