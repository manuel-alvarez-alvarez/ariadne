/**
 * The Time section of the Stats screen: how long does it take? It reads
 * `GET /v1/stats/time` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type TimeStatsDto, unwrap } from "@/api"
import { formatDuration } from "@/lib/format"
import { TIME_STATUS_CONFIG, TIME_STATUS_PIE_CONFIG } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatPieChart } from "./stat-pie-chart"
import { StatSection } from "./stat-section"
import { StatTable } from "./stat-table"
import { StatTile, StatTiles } from "./stat-tiles"

/** `GET /v1/stats/time`, narrowed by the screen's filter. */
function timeStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.time(filter),
    queryFn: (): Promise<TimeStatsDto> =>
      unwrap(api().GET("/v1/stats/time", { params: { query: filter } })),
  })
}

export function TimeSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(timeStatsQueryOptions(filter))
  return (
    <StatSection
      title="Time"
      description="How long does it take?"
      query={stats}
      isEmpty={(data) => !data.tasks}
      empty="No work was timed in this span."
    >
      {(data) => {
        const rows = data.in_status.map((row) => ({ label: row.status, ...row }))
        const shareRows = data.in_status.map((row) => ({
          key: row.status,
          value: row.total_secs,
          ...row,
        }))
        return (
          <div className="flex flex-1 flex-col gap-4">
            <StatTiles>
              <StatTile
                label="Median lead time"
                value={formatDuration(data.lead_time.median_secs)}
                explain="Median lead time: the median time from a finished task's creation to its finish."
              />
              <StatTile
                label="P90 lead time"
                value={formatDuration(data.lead_time.p90_secs)}
                explain="P90 lead time: the nearest-rank 90th percentile of that same lead time."
              />
              <StatTile
                label="Time waiting on a person"
                value={formatDuration(data.waiting_on_person.total_secs)}
                explain="Time waiting on a person: the total time spent on permission prompts you answered."
              />
              <StatTile
                label="Prompts waited on"
                value={data.waiting_on_person.prompts}
                explain="Prompts waited on: the count of permission prompts you answered."
              />
            </StatTiles>
            <div className="flex flex-1 flex-col gap-4 md:flex-row">
              <StatBarChart
                data={rows}
                config={TIME_STATUS_CONFIG}
                bars={["total_secs"]}
                legendExtra={["median_secs", "share"]}
                caption="Where the time goes"
                tooltip={(row) => (
                  <>
                    <p className="font-medium">{row.status}</p>
                    <p>Median: {formatDuration(row.median_secs)}</p>
                    <p>Share: {(row.share * 100).toFixed(1)}%</p>
                  </>
                )}
                columns={[
                  { header: "Status", render: (row) => row.status },
                  { header: "Total", render: (row) => formatDuration(row.total_secs) },
                  { header: "Median", render: (row) => formatDuration(row.median_secs) },
                  { header: "Share", render: (row) => `${(row.share * 100).toFixed(1)}%` },
                ]}
              />
              <div className="flex flex-1 flex-col gap-2 md:max-w-64">
                <StatPieChart
                  data={shareRows}
                  config={TIME_STATUS_PIE_CONFIG}
                  caption="Share of time by status"
                  valueFormatter={formatDuration}
                  columns={[
                    { header: "Status", render: (row) => row.status },
                    { header: "Total", render: (row) => formatDuration(row.total_secs) },
                    { header: "Median", render: (row) => formatDuration(row.median_secs) },
                    { header: "Share", render: (row) => `${(row.share * 100).toFixed(1)}%` },
                  ]}
                />
                <StatTable
                  rows={shareRows}
                  rowKey={(row) => row.status}
                  caption="Share of time by status, exact values"
                  columns={[
                    {
                      header: "Status",
                      explain:
                        "Status: which step of a finished task's lifecycle this row measures.",
                      render: (row) => row.status,
                    },
                    {
                      header: "Share",
                      numeric: true,
                      explain: "Share: this status's share of the total time measured here.",
                      render: (row) => `${(row.share * 100).toFixed(1)}%`,
                    },
                  ]}
                />
              </div>
            </div>
          </div>
        )
      }}
    </StatSection>
  )
}
