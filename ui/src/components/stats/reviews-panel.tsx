import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { TableCell, TableRow } from "@/components/ui/table"

function reviewsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.reviews(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/reviews", { params: { query: filter } })),
  })
}

export function ReviewsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(reviewsQueryOptions(filter))
  const rows = stats.data?.authors ?? []
  return (
    <section aria-label="Reviews" className="flex flex-col gap-3">
      <h2 className="text-lg font-semibold">Reviews</h2>
      <DataTable
        query={{ ...stats, data: rows }}
        errorTitle="Could not load reviews"
        columns={[
          { header: "Author model" },
          { header: "Approvals" },
          { header: "Mean rounds" },
          { header: "Median rounds" },
          { header: "First pass" },
        ]}
        empty={<EmptyState emphasis="quiet" title="No review approvals in this span." />}
        rowKey={(row) => row.model}
        renderRow={(row) => (
          <TableRow>
            <TableCell className="font-mono text-xs">{row.model}</TableCell>
            <TableCell>{row.approvals}</TableCell>
            <TableCell>{row.mean_rounds}</TableCell>
            <TableCell>{row.median_rounds}</TableCell>
            <TableCell>{Math.round(row.first_pass_rate * 100)}%</TableCell>
          </TableRow>
        )}
      />
    </section>
  )
}
