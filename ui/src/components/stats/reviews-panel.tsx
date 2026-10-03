/**
 * The reviews panel of the Stats screen: how often an author model's first
 * submission was approved, one bar per author model of `GET /v1/stats/reviews`.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { type AuthorReviewStatDto, api, qk, type StatsFilter, unwrap } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { formatRate } from "@/lib/format"
import { REVIEWS_CONFIG } from "./chart-configs"
import { StatSectionHeading } from "./section-heading"
import { StatBarChart } from "./stat-bar-chart"
import { StatQueryState } from "./stat-query-state"

interface ReviewRow {
  label: string
  firstPassRate: number
  approvals: number
  mean_rounds: number
  median_rounds: number
}

function toRows(authors: AuthorReviewStatDto[]): ReviewRow[] {
  return authors.map((row) => ({
    label: row.model,
    firstPassRate: row.first_pass_rate * 100,
    approvals: row.approvals,
    mean_rounds: row.mean_rounds,
    median_rounds: row.median_rounds,
  }))
}

function reviewsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.reviews(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/reviews", { params: { query: filter } })),
    select: (response) => toRows(response.authors),
  })
}

export function ReviewsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(reviewsQueryOptions(filter))
  const rows = stats.data ?? []

  return (
    <section aria-label="Reviews" className="flex flex-col gap-3">
      <StatSectionHeading>Reviews</StatSectionHeading>
      <StatQueryState query={stats} errorTitle="Could not load reviews">
        {rows.length === 0 ? (
          <EmptyState emphasis="quiet" title="No review approvals in this span." />
        ) : (
          <StatBarChart
            data={[...rows].sort((a, b) => b.firstPassRate - a.firstPassRate)}
            config={REVIEWS_CONFIG}
            bars={["firstPassRate"]}
            caption="First-pass rate per author model"
            columns={[
              { header: "Author model", render: (row) => row.label },
              { header: "First pass", render: (row) => `${row.firstPassRate.toFixed(1)}%` },
              { header: "Approvals", render: (row) => row.approvals },
              { header: "Mean rounds", render: (row) => row.mean_rounds },
              { header: "Median rounds", render: (row) => row.median_rounds },
            ]}
            tooltip={(row) => (
              <div className="flex flex-col gap-1">
                <p className="font-medium">{row.label}</p>
                <p>First pass: {formatRate(row.firstPassRate / 100)}</p>
                <p>Approvals: {row.approvals}</p>
                <p>Mean rounds: {row.mean_rounds}</p>
                <p>Median rounds: {row.median_rounds}</p>
              </div>
            )}
          />
        )}
      </StatQueryState>
    </section>
  )
}
