/**
 * The Spend section of the Stats screen: what did it spend? It reads
 * `GET /v1/stats/spend` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type SpendStatsDto, type StatsFilter, unwrap } from "@/api"
import { StatSection } from "./stat-section"

/** `GET /v1/stats/spend`, narrowed by the screen's filter. */
function spendStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.spend(filter),
    queryFn: (): Promise<SpendStatsDto> =>
      unwrap(api().GET("/v1/stats/spend", { params: { query: filter } })),
  })
}

export function SpendSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(spendStatsQueryOptions(filter))
  return (
    <StatSection
      title="Spend"
      description="What did it spend?"
      query={stats}
      isEmpty={() => true}
      empty="Nothing was spent in this span."
    >
      {() => null}
    </StatSection>
  )
}
