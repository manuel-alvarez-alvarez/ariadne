/**
 * The Attention section of the Stats screen: how much did it need me? It reads
 * `GET /v1/stats/attention` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { type AttentionStatsDto, api, qk, type StatsFilter, unwrap } from "@/api"
import { StatSection } from "./stat-section"

/** `GET /v1/stats/attention`, narrowed by the screen's filter. */
function attentionStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.attention(filter),
    queryFn: (): Promise<AttentionStatsDto> =>
      unwrap(api().GET("/v1/stats/attention", { params: { query: filter } })),
  })
}

export function AttentionSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(attentionStatsQueryOptions(filter))
  return (
    <StatSection
      title="Attention"
      description="How much did it need me?"
      query={stats}
      isEmpty={() => true}
      empty="Nothing needed you in this span."
    >
      {() => null}
    </StatSection>
  )
}
