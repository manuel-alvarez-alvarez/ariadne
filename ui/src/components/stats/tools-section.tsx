/**
 * The Tools section of the Stats screen: what do the agents do? It reads
 * `GET /v1/stats/tools` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type ToolStatsDto, unwrap } from "@/api"
import { StatSection } from "./stat-section"

/** `GET /v1/stats/tools`, narrowed by the screen's filter. */
function toolsStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.tools(filter),
    queryFn: (): Promise<ToolStatsDto> =>
      unwrap(api().GET("/v1/stats/tools", { params: { query: filter } })),
  })
}

export function ToolsSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(toolsStatsQueryOptions(filter))
  return (
    <StatSection
      title="Tools"
      description="What do the agents do?"
      query={stats}
      isEmpty={() => true}
      empty="No tool ran in this span."
    >
      {() => null}
    </StatSection>
  )
}
