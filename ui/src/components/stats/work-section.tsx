/**
 * The Work section of the Stats screen: what got done? It reads
 * `GET /v1/stats/work` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, unwrap, type WorkStatsDto } from "@/api"
import { StatSection } from "./stat-section"

/** `GET /v1/stats/work`, narrowed by the screen's filter. */
function workStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.work(filter),
    queryFn: (): Promise<WorkStatsDto> =>
      unwrap(api().GET("/v1/stats/work", { params: { query: filter } })),
  })
}

export function WorkSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(workStatsQueryOptions(filter))
  return (
    <StatSection
      title="Work"
      description="What got done?"
      query={stats}
      isEmpty={() => true}
      empty="Nothing got done in this span."
    >
      {() => null}
    </StatSection>
  )
}
