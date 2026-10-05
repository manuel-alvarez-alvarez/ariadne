/**
 * A chart's legend, drawn as a sibling of the measured chart area rather than
 * through recharts' own `<Legend>`: recharts renders nothing — the legend
 * included — until it measures a real box for its `ResponsiveContainer`, and
 * jsdom never gives it one (`stat-time-chart.test.tsx` already keeps every
 * other assertion on the `sr-only` side of that same line, for the same
 * reason). This is also the one place a figure drawn as a bar, with no label
 * of its own on the chart, can carry its meaning on hover and on keyboard
 * focus — and the one place a figure the chart carries with no bar of its
 * own at all, such as a bucket's cached share or a status row's median, can
 * carry it too: `keys` is not only the stacked or grouped series, it is
 * every figure the caller wants explained here.
 */

import type { ChartConfig } from "@/components/ui/chart"
import { StatExplain } from "./stat-explain"

export function StatChartLegend({ config, keys }: { config: ChartConfig; keys: string[] }) {
  return (
    <div className="flex flex-wrap items-center justify-center gap-4 pt-3 text-xs">
      {keys.map((key) => {
        const item = config[key]
        const label = item?.label ?? key
        return (
          <span key={key} className="flex items-center gap-1.5">
            {item?.color ? (
              <span
                aria-hidden
                className="h-2 w-2 shrink-0 rounded-[2px]"
                style={{ backgroundColor: item.color }}
              />
            ) : null}
            {item?.explain ? <StatExplain explain={item.explain}>{label}</StatExplain> : label}
          </span>
        )
      })}
    </div>
  )
}
