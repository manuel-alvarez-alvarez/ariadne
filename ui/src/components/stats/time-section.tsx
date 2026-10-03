/**
 * The Time section of the Stats screen: how long does it take? It reads
 * `GET /v1/stats/time` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type TimeStatsDto, unwrap } from "@/api"
import { StatSection } from "./stat-section"

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
      isEmpty={() => true}
      empty="No work was timed in this span."
    >
      {() => null}
    </StatSection>
  )
}
