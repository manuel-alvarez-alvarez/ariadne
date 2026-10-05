/**
 * The Attention section of the Stats screen: how much did it need me? It reads
 * `GET /v1/stats/attention` under the screen's filter, and says its empty sentence
 * until its family task draws the figures.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { type AttentionStatsDto, api, qk, type StatsFilter, unwrap } from "@/api"
import { formatDuration } from "@/lib/format"
import { attentionFlagConfig, attentionPermissionConfig } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatSection } from "./stat-section"
import { StatTile, StatTiles } from "./stat-tiles"

/** `GET /v1/stats/attention`, narrowed by the screen's filter. */
export function attentionStatsQueryOptions(filter: StatsFilter) {
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
      isEmpty={(data) =>
        (data.permissions?.total ?? 0) === 0 &&
        (data.flags ?? []).every((flag) => flag.raised === 0) &&
        (data.sessions_failed ?? 0) === 0 &&
        (data.sessions_stalled ?? 0) === 0 &&
        (data.exhaustions ?? 0) === 0
      }
      empty="Nothing needed you in this span."
    >
      {(data) => <AttentionFigures data={data} />}
    </StatSection>
  )
}

function AttentionFigures({ data }: { data: AttentionStatsDto }) {
  const permissions = data.permissions.by_decider.map((row) => ({
    label: row.decided_by,
    allowed: row.allowed,
    denied: row.denied,
    cancelled: row.cancelled,
  }))
  const flags = data.flags.map((row) => ({ label: row.reason, raised: row.raised }))
  const person = data.permissions.by_decider.find((row) => row.decided_by === "console")
  const personWait = person?.mean_wait_ms ?? 0
  return (
    <div className="flex flex-1 flex-col gap-4">
      <StatTiles>
        <StatTile
          label="Prompts you answered"
          value={person?.total ?? 0}
          explain="Prompts you answered: the count of permission prompts you answered."
        />
        <StatTile
          label="Your mean wait"
          value={formatDuration(personWait / 1000)}
          explain="Your mean wait: the mean time a permission prompt waited on you."
        />
        <StatTile
          label="Questions asked"
          value={data.flags.find((flag) => flag.reason === "waiting_input")?.raised ?? 0}
          explain="Questions asked: the count of times an agent asked you a question."
        />
        <StatTile
          label="Stalled sessions"
          value={data.sessions_stalled}
          explain="Stalled sessions: the count of sessions that ended stalled or in an agent error."
        />
      </StatTiles>
      <StatBarChart
        data={permissions}
        config={attentionPermissionConfig}
        bars={[["allowed", "denied", "cancelled"]]}
        tooltip={(row) => (
          <>
            {row.label}: {row.allowed} allow, {row.denied} deny, {row.cancelled} cancelled
          </>
        )}
        caption="Permission answers by decider"
        columns={[
          { header: "Decided by", render: (row) => row.label },
          { header: "Allow", render: (row) => row.allowed },
          { header: "Deny", render: (row) => row.denied },
          { header: "Cancelled", render: (row) => row.cancelled },
        ]}
      />
      <StatBarChart
        data={flags}
        config={attentionFlagConfig}
        bars={["raised"]}
        tooltip={(row) => (
          <>
            {row.label}: {row.raised} raised
          </>
        )}
        caption="Attention flags by reason"
        columns={[
          { header: "Reason", render: (row) => row.label },
          { header: "Raised", render: (row) => row.raised },
        ]}
      />
    </div>
  )
}
