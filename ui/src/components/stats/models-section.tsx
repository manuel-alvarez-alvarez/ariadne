/**
 * The Models section of the Stats screen: which model does the job? It reads
 * `GET /v1/stats/models` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, type ModelStatsDto, qk, type StatsFilter, unwrap } from "@/api"
import { StatSection } from "./stat-section"

/** `GET /v1/stats/models`, narrowed by the screen's filter. */
function modelsStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.models(filter),
    queryFn: (): Promise<ModelStatsDto> =>
      unwrap(api().GET("/v1/stats/models", { params: { query: filter } })),
  })
}

export function ModelsSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(modelsStatsQueryOptions(filter))
  return (
    <StatSection
      title="Models"
      description="Which model does the job?"
      query={stats}
      isEmpty={() => true}
      empty="No model ran in this span."
    >
      {() => null}
    </StatSection>
  )
}
