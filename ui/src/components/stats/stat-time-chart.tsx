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

import { Bar, BarChart, CartesianGrid, XAxis, YAxis } from "recharts"

import {
  type ChartConfig,
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
} from "@/components/ui/chart"

const HEIGHT_PX = 160

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
}: {
  /** One row per bucket, oldest first, `bucket` its RFC 3339 start. */
  data: T[]
  config: ChartConfig
  /** The numeric keys of a row, stacked into its bar bottom up. */
  keys: (keyof T & string)[]
  /** The `sr-only` table's caption: what the chart is a chart of. */
  caption: string
}) {
  return (
    <div className="flex flex-col gap-2">
      <ChartContainer
        config={config}
        className="block aspect-auto w-full"
        style={{ height: HEIGHT_PX }}
      >
        <BarChart data={data} margin={{ left: 0, right: 8, top: 4, bottom: 4 }}>
          <CartesianGrid vertical={false} stroke="var(--color-border)" strokeOpacity={0.5} />
          <XAxis
            dataKey="bucket"
            tickFormatter={bucketLabel}
            axisLine={false}
            tickLine={false}
            className="text-xs"
          />
          <YAxis axisLine={false} tickLine={false} width={40} className="text-xs" />
          <ChartTooltip
            cursor={false}
            content={({ active, payload }) => {
              const row = payload?.[0]?.payload as T | undefined
              return active && row ? (
                <div className="flex flex-col gap-1 rounded-lg border border-border/50 bg-background px-2.5 py-1.5 text-xs shadow-xl">
                  <p className="font-medium">{bucketLabel(row.bucket)}</p>
                  {keys.map((key) => (
                    <p key={key}>
                      {config[key]?.label ?? key}: {String(row[key])}
                    </p>
                  ))}
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
            {keys.map((key) => (
              <th key={key}>{config[key]?.label ?? key}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((row) => (
            <tr key={row.bucket}>
              <td>{bucketLabel(row.bucket)}</td>
              {keys.map((key) => (
                <td key={key}>{String(row[key])}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
