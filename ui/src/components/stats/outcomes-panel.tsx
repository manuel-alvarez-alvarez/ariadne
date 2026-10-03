/**
 * The outcomes panel of the Stats screen: how tasks end per author model, and
 * who wins a contest several authors ran, off `GET /v1/stats/outcomes`.
 *
 * A row is a model's own figures; the last row is the totals the daemon
 * already summed across every model. Both come straight off the daemon's
 * aggregate, same as the models panel.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import {
  api,
  type OutcomeStatDto,
  type OutcomeTotalsDto,
  qk,
  type StatsFilter,
  unwrap,
} from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { TableCell, TableRow } from "@/components/ui/table"
import { formatDuration, formatRate } from "@/lib/format"

const COLUMNS = [
  { header: "Model" },
  { header: "Finished", className: "text-right" },
  { header: "Failed", className: "text-right" },
  { header: "Cancelled", className: "text-right" },
  { header: "Finish rate", className: "text-right" },
  { header: "Mean lead time", className: "text-right" },
  { header: "Median lead time", className: "text-right" },
  { header: "Mean reviews", className: "text-right" },
  { header: "Contests", className: "text-right" },
  { header: "Win rate", className: "text-right" },
]

/** One row of the table: a model's own figures, or the totals across all. */
type OutcomeRow =
  | (OutcomeStatDto & { isTotal: false })
  | (OutcomeTotalsDto & { model: string; isTotal: true })

/** `GET /v1/stats/outcomes`, narrowed by the screen's filter. */
function outcomeStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.outcomes(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/outcomes", { params: { query: filter } })),
    select: (response): OutcomeRow[] =>
      response.items.length === 0
        ? []
        : [
            ...response.items.map((item) => ({ ...item, isTotal: false as const })),
            { ...response.totals, model: "Totals", isTotal: true as const },
          ],
  })
}

export function OutcomesPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(outcomeStatsQueryOptions(filter))
  return (
    <section aria-label="Outcomes" className="flex flex-col gap-2">
      <h2 className="font-heading text-sm font-semibold">Outcomes</h2>
      <DataTable
        query={stats}
        errorTitle="Could not load the outcome stats"
        columns={COLUMNS}
        empty={
          <EmptyState
            title="No task has ended in this span"
            description="A row appears for each author model once a task on it ends."
          />
        }
        rowKey={(row) => row.model}
        renderRow={(row) => <OutcomeStatRow row={row} />}
      />
    </section>
  )
}

function OutcomeStatRow({ row }: { row: OutcomeRow }) {
  return (
    <TableRow className={row.isTotal ? "font-semibold" : undefined}>
      <TableCell className={row.isTotal ? undefined : "font-mono text-xs"}>{row.model}</TableCell>
      <TableCell className="text-right tabular-nums">{row.finished}</TableCell>
      <TableCell className="text-right tabular-nums">{row.failed}</TableCell>
      <TableCell className="text-right tabular-nums">{row.cancelled}</TableCell>
      <TableCell className="text-right tabular-nums">{formatRate(row.finish_rate)}</TableCell>
      <TableCell className="text-right tabular-nums">
        {formatDuration(row.mean_lead_time_secs)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {formatDuration(row.median_lead_time_secs)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {row.mean_review_requests.toFixed(1)}
      </TableCell>
      <TableCell className="text-right tabular-nums">
        {row.contests_won}/{row.contests_entered}
      </TableCell>
      <TableCell className="text-right tabular-nums">{formatRate(row.win_rate)}</TableCell>
    </TableRow>
  )
}
