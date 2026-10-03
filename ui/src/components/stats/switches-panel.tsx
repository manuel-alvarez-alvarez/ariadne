/**
 * The switches panel of the Stats screen: how often a session left each
 * model, and why, one row per model of `GET /v1/stats/switches`.
 *
 * The figures are the daemon's own aggregates of its stats ledger, and nothing
 * here adds anything up. The dispatcher invalidates the `stats` group on every
 * task and session update, so a switch shows up in its row without a refresh.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type SwitchStatDto, unwrap } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { SWITCHES_CONFIG } from "./chart-configs"
import { StatSectionHeading } from "./section-heading"
import { StatBarChart } from "./stat-bar-chart"
import { StatQueryState } from "./stat-query-state"

interface SwitchRow {
  label: string
  exhausted: number
  automaticOther: number
  other: number
  arrivals: number
}

function toRows(items: SwitchStatDto[]): SwitchRow[] {
  return items.map((row) => {
    // `automatic_share` counts every automatic switch, `exhaustions` the ones
    // of them left for running out — the only reason that is automatic
    // today, so this is 0 until the daemon grows another.
    const automaticTotal = Math.round(row.switches * row.automatic_share)
    const automaticOther = Math.max(0, automaticTotal - row.exhaustions)
    return {
      label: row.model,
      exhausted: row.exhaustions,
      automaticOther,
      other: Math.max(0, row.switches - row.exhaustions - automaticOther),
      arrivals: row.arrivals,
    }
  })
}

/** `GET /v1/stats/switches`, narrowed by the screen's filter. */
function switchStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.switches(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/switches", { params: { query: filter } })),
    select: (response) => toRows(response.items),
  })
}

export function SwitchesPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(switchStatsQueryOptions(filter))
  const rows = stats.data ?? []

  return (
    <section aria-label="Switches" className="flex flex-col gap-3">
      <StatSectionHeading>Switches</StatSectionHeading>
      <StatQueryState query={stats} errorTitle="Could not load the switch stats">
        {rows.length === 0 ? (
          <EmptyState emphasis="quiet" title="No session has switched in this span" />
        ) : (
          <StatBarChart
            data={[...rows].sort(
              (a, b) =>
                b.exhausted +
                b.automaticOther +
                b.other -
                (a.exhausted + a.automaticOther + a.other),
            )}
            config={SWITCHES_CONFIG}
            bars={[["exhausted", "automaticOther", "other"], "arrivals"]}
            caption="Switches per model, by reason, and arrivals"
            columns={[
              { header: "Model", render: (row) => row.label },
              { header: "Exhausted", render: (row) => row.exhausted },
              { header: "Automatic", render: (row) => row.automaticOther },
              { header: "Other", render: (row) => row.other },
              { header: "Arrivals", render: (row) => row.arrivals },
            ]}
            tooltip={(row) => (
              <div className="flex flex-col gap-1">
                <p className="font-medium">{row.label}</p>
                <p>Exhausted: {row.exhausted}</p>
                <p>Automatic: {row.automaticOther}</p>
                <p>Other: {row.other}</p>
                <p>Arrivals: {row.arrivals}</p>
              </div>
            )}
          />
        )}
      </StatQueryState>
    </section>
  )
}
