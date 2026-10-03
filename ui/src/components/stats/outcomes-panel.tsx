/**
 * The outcomes panel of the Stats screen: how tasks end per author model, and
 * who wins a contest several authors ran, off `GET /v1/stats/outcomes`.
 *
 * The figures are the daemon's own aggregate, same as the models panel:
 * nothing here adds anything up.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, type OutcomeStatDto, qk, type StatsFilter, unwrap } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { formatDuration, formatRate } from "@/lib/format"
import { ENDINGS_CONFIG, LEAD_TIME_CONFIG } from "./chart-configs"
import { StatSectionHeading } from "./section-heading"
import { StatBarChart } from "./stat-bar-chart"
import { StatQueryState } from "./stat-query-state"

interface OutcomeRow {
  label: string
  finished: number
  failed: number
  cancelled: number
  finish_rate: number
  win_rate: number
  contests_entered: number
  contests_won: number
  mean_lead_time_secs: number
  median_lead_time_secs: number
  mean_review_requests: number
}

function toRows(items: OutcomeStatDto[]): OutcomeRow[] {
  return items.map((row) => ({
    label: row.model,
    finished: row.finished,
    failed: row.failed,
    cancelled: row.cancelled,
    finish_rate: row.finish_rate,
    win_rate: row.win_rate,
    contests_entered: row.contests_entered,
    contests_won: row.contests_won,
    mean_lead_time_secs: row.mean_lead_time_secs,
    median_lead_time_secs: row.median_lead_time_secs,
    mean_review_requests: row.mean_review_requests,
  }))
}

/** `GET /v1/stats/outcomes`, narrowed by the screen's filter. */
function outcomeStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.outcomes(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/outcomes", { params: { query: filter } })),
    select: (response) => toRows(response.items),
  })
}

export function OutcomesPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(outcomeStatsQueryOptions(filter))
  const rows = stats.data ?? []

  return (
    <section aria-label="Outcomes" className="flex flex-col gap-3">
      <StatSectionHeading>Outcomes</StatSectionHeading>
      <StatQueryState query={stats} errorTitle="Could not load the outcome stats">
        {rows.length === 0 ? (
          <EmptyState emphasis="quiet" title="No task has ended in this span" />
        ) : (
          <>
            <StatBarChart
              data={[...rows].sort(
                (a, b) =>
                  b.finished + b.failed + b.cancelled - (a.finished + a.failed + a.cancelled),
              )}
              config={ENDINGS_CONFIG}
              bars={[["finished", "failed", "cancelled"]]}
              caption="Tasks per author model, by how they ended"
              columns={[
                { header: "Model", render: (row) => row.label },
                { header: "Finished", render: (row) => row.finished },
                { header: "Failed", render: (row) => row.failed },
                { header: "Cancelled", render: (row) => row.cancelled },
                { header: "Finish rate", render: (row) => formatRate(row.finish_rate) },
                { header: "Win rate", render: (row) => formatRate(row.win_rate) },
                {
                  header: "Contests",
                  render: (row) => `${row.contests_won}/${row.contests_entered}`,
                },
                {
                  header: "Mean lead time",
                  render: (row) => formatDuration(row.mean_lead_time_secs),
                },
                { header: "Mean reviews", render: (row) => row.mean_review_requests.toFixed(1) },
              ]}
              tooltip={(row) => (
                <div className="flex flex-col gap-1">
                  <p className="font-medium">{row.label}</p>
                  <p>Finished: {row.finished}</p>
                  <p>Failed: {row.failed}</p>
                  <p>Cancelled: {row.cancelled}</p>
                  <p>Finish rate: {formatRate(row.finish_rate)}</p>
                  <p>Win rate: {formatRate(row.win_rate)}</p>
                  <p>
                    Contests: {row.contests_won}/{row.contests_entered}
                  </p>
                  <p>Mean lead time: {formatDuration(row.mean_lead_time_secs)}</p>
                  <p>Mean reviews: {row.mean_review_requests.toFixed(1)}</p>
                </div>
              )}
            />
            <StatBarChart
              data={[...rows].sort((a, b) => b.median_lead_time_secs - a.median_lead_time_secs)}
              config={LEAD_TIME_CONFIG}
              bars={["median_lead_time_secs"]}
              caption="Median lead time per model"
              columns={[
                { header: "Model", render: (row) => row.label },
                {
                  header: "Median lead time",
                  render: (row) => formatDuration(row.median_lead_time_secs),
                },
              ]}
              tooltip={(row) => (
                <div className="flex flex-col gap-1">
                  <p className="font-medium">{row.label}</p>
                  <p>Median: {formatDuration(row.median_lead_time_secs)}</p>
                  <p>Mean: {formatDuration(row.mean_lead_time_secs)}</p>
                </div>
              )}
            />
          </>
        )}
      </StatQueryState>
    </section>
  )
}
