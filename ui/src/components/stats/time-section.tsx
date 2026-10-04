/**
 * The Time section of the Stats screen: how long does it take? It reads
 * `GET /v1/stats/time` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type TimeStatsDto, unwrap } from "@/api"
import { formatDuration } from "@/lib/format"
import { TIME_STATUS_CONFIG } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatSection } from "./stat-section"
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
      isEmpty={(data) => data.tasks === 0}
      empty="No work was timed in this span."
    >
      {(data) => {
        const rows = data.in_status.map((row) => ({ label: row.status, ...row }))
        return (
          <>
            <StatTiles>
              <StatTile
                label="Median lead time"
                value={formatDuration(data.lead_time.median_secs)}
              />
              <StatTile label="P90 lead time" value={formatDuration(data.lead_time.p90_secs)} />
              <StatTile
                label="Time waiting on a person"
                value={formatDuration(data.waiting_on_person.total_secs)}
              />
              <StatTile label="Prompts waited on" value={data.waiting_on_person.prompts} />
            </StatTiles>
            <StatBarChart
              data={rows}
              config={TIME_STATUS_CONFIG}
              bars={["total_secs"]}
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
          </>
        )
      }}
    </StatSection>
  )
}
