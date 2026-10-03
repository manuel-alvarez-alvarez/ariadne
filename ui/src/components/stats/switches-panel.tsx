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
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { TableCell, TableRow } from "@/components/ui/table"

const COLUMNS = [
  { header: "Model" },
  { header: "Switches", className: "text-right" },
  { header: "Exhausted", className: "text-right" },
  { header: "Automatic", className: "text-right" },
  { header: "Arrivals", className: "text-right" },
]

/** `GET /v1/stats/switches`, narrowed by the screen's filter. */
function switchStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.switches(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/switches", { params: { query: filter } })),
    select: (response) => response.items,
  })
}

export function SwitchesPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(switchStatsQueryOptions(filter))
  return (
    <section aria-label="Switches" className="flex flex-col gap-2">
      <h2 className="font-heading text-sm font-semibold">Switches</h2>
      <DataTable
        query={stats}
        errorTitle="Could not load the switch stats"
        columns={COLUMNS}
        empty={
          <EmptyState
            title="No session has switched in this span"
            description="A row appears for each model once a session leaves it or arrives on it."
          />
        }
        rowKey={(row) => row.model}
        renderRow={(row) => <SwitchStatRow row={row} />}
      />
    </section>
  )
}

function SwitchStatRow({ row }: { row: SwitchStatDto }) {
  return (
    <TableRow>
      <TableCell className="font-mono text-xs">{row.model}</TableCell>
      <TableCell className="text-right tabular-nums">{row.switches}</TableCell>
      <TableCell className="text-right tabular-nums">{row.exhaustions}</TableCell>
      <TableCell className="text-right tabular-nums">
        {(row.automatic_share * 100).toFixed(1)}%
      </TableCell>
      <TableCell className="text-right tabular-nums">{row.arrivals}</TableCell>
    </TableRow>
  )
}
