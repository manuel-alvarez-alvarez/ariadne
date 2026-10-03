/**
 * The models panel of the Stats screen: how each model did in each seat, one
 * row per `(model, seat)` of `GET /v1/stats/models`.
 *
 * The figures are the daemon's own aggregates of its stats ledger, and nothing
 * here adds anything up. The dispatcher invalidates the `stats` group on every
 * task and session update, so a session that ends shows up in its row without
 * a refresh.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, type ModelStatDto, qk, type StatsFilter, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { TokenFigure } from "@/components/token-figure"
import { TableCell, TableRow } from "@/components/ui/table"
import { formatDuration } from "@/lib/format"

const COLUMNS = [
  { header: "Model" },
  { header: "Seat" },
  { header: "Sessions", className: "text-right" },
  { header: "Failed", className: "text-right" },
  { header: "Stalled", className: "text-right" },
  { header: "Tokens" },
  { header: "Mean lifetime", className: "text-right" },
  { header: "Skills" },
]

/** `GET /v1/stats/models`, narrowed by the screen's filter. */
function modelStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.models(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/models", { params: { query: filter } })),
    select: (response) => response.items,
  })
}

export function ModelsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(modelStatsQueryOptions(filter))
  return (
    <section aria-label="Models" className="flex flex-col gap-2">
      <h2 className="font-heading text-sm font-semibold">Models</h2>
      <DataTable
        query={stats}
        errorTitle="Could not load the model stats"
        columns={COLUMNS}
        empty={
          <EmptyState
            title="No session has ended in this span"
            description="A row appears for each model and seat once a session on it ends."
          />
        }
        rowKey={(row) => `${row.model} ${row.seat ?? ""}`}
        renderRow={(row) => <ModelStatRow row={row} />}
      />
    </section>
  )
}

function ModelStatRow({ row }: { row: ModelStatDto }) {
  return (
    <TableRow>
      <TableCell className="font-mono text-xs">{row.model}</TableCell>
      <TableCell>{row.seat ?? "-"}</TableCell>
      <TableCell className="text-right tabular-nums">{row.sessions}</TableCell>
      <TableCell className="text-right tabular-nums">{row.failed}</TableCell>
      <TableCell className="text-right tabular-nums">{row.stalled}</TableCell>
      <TableCell>
        <TokenFigure usage={row.usage} />
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {formatDuration(row.mean_lifetime_secs)}
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        {row.skills.length === 0
          ? "-"
          : row.skills.map((skill) => `${skill.name} ${skill.sessions}`).join(", ")}
      </TableCell>
    </TableRow>
  )
}
