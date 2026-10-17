/**
 * The Spend section of the Stats screen: what did it spend? Tokens over
 * time and per finished task — never a cost. It reads `GET /v1/stats/spend`
 * under the screen's filter; the Models section is where a model is compared.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type SpendStatsDto, type StatsFilter, unwrap } from "@/api"
import { cachedShare, formatTokens, plural } from "@/lib/format"
import { SPEND_COMPOSITION_CONFIG, SPEND_CONFIG } from "./chart-configs"
import { StatPieChart } from "./stat-pie-chart"
import { StatSection } from "./stat-section"
import { StatTable } from "./stat-table"
import { StatTile, StatTiles } from "./stat-tiles"
import { StatTimeChart } from "./stat-time-chart"

/** `GET /v1/stats/spend`, narrowed by the screen's filter. */
export function spendStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.spend(filter),
    queryFn: (): Promise<SpendStatsDto> =>
      unwrap(api().GET("/v1/stats/spend", { params: { query: filter } })),
  })
}

/** A share, already a 0-1 ratio off the daemon, to one decimal place: `89.1%`. */
function formatShare(ratio: number): string {
  const tenths = Math.round(Math.min(1000, Math.max(0, ratio * 1000)))
  return `${(tenths / 10).toFixed(1)}%`
}

type TimeRow = {
  bucket: string
  input_tokens: number
  output_tokens: number
  cached_input_tokens: number
}

export function SpendSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(spendStatsQueryOptions(filter))
  return (
    <StatSection
      title="Spend"
      description="What did it spend?"
      query={stats}
      isEmpty={(data) => (data.totals?.sessions ?? 0) === 0}
      empty="Nothing was spent in this span."
    >
      {(data) => {
        const timeRows: TimeRow[] = data.buckets.map((bucket) => ({
          bucket: bucket.start,
          input_tokens: bucket.input_tokens,
          output_tokens: bucket.output_tokens,
          cached_input_tokens: bucket.cached_input_tokens,
        }))
        const perTask = data.per_finished_task.input_tokens + data.per_finished_task.output_tokens
        const total = data.totals.input_tokens + data.totals.output_tokens
        const compositionRows = [
          {
            key: "fresh_input_tokens",
            value: data.totals.input_tokens - data.totals.cached_input_tokens,
          },
          { key: "cached_input_tokens", value: data.totals.cached_input_tokens },
          { key: "output_tokens", value: data.totals.output_tokens },
        ]

        return (
          <div className="flex flex-1 flex-col gap-4">
            <StatTiles>
              <StatTile
                label="Input tokens"
                value={formatTokens(data.totals.input_tokens)}
                explain="Input tokens: input tokens spent across every session in this span."
              />
              <StatTile
                label="Cache share"
                value={formatShare(data.totals.cached_share)}
                explain="Cache share: the share of input tokens that were served from cache."
              />
              <StatTile
                label="Output tokens"
                value={formatTokens(data.totals.output_tokens)}
                explain="Output tokens: output tokens spent across every session in this span."
              />
              <StatTile
                label="Tokens per finished task"
                value={formatTokens(perTask)}
                hint={`of ${plural(data.per_finished_task.tasks, "task")}`}
                explain="Tokens per finished task: input and output tokens divided by the tasks that finished in this span."
              />
            </StatTiles>
            <div className="flex flex-1 flex-col gap-4 md:flex-row">
              <StatTimeChart
                data={timeRows}
                config={SPEND_CONFIG}
                keys={["input_tokens", "output_tokens"]}
                legendExtra={["cached_share"]}
                caption="Tokens spent over time"
                valueFormatter={formatTokens}
                tooltipExtra={(row) => <p>Cached: {cachedShare(row)}</p>}
                bucket={data.bucket}
              />
              <div className="flex flex-1 flex-col gap-2 md:max-w-64">
                <StatPieChart
                  data={compositionRows}
                  config={SPEND_COMPOSITION_CONFIG}
                  caption="How the span's tokens split: fresh input, cached input and output"
                  valueFormatter={formatTokens}
                  columns={[
                    { header: "Type", render: (row) => SPEND_COMPOSITION_CONFIG[row.key]?.label },
                    { header: "Tokens", render: (row) => formatTokens(row.value) },
                    {
                      header: "Share",
                      render: (row) =>
                        total === 0 ? formatShare(0) : formatShare(row.value / total),
                    },
                  ]}
                />
                <StatTable
                  rows={compositionRows}
                  rowKey={(row) => row.key}
                  caption="Token composition, exact values"
                  columns={[
                    {
                      header: "Type",
                      explain: "Type: which part of the span's tokens this row measures.",
                      render: (row) => SPEND_COMPOSITION_CONFIG[row.key]?.label,
                    },
                    {
                      header: "Tokens",
                      numeric: true,
                      explain:
                        "Tokens: the tokens of this type spent across every session in this span.",
                      render: (row) => formatTokens(row.value),
                    },
                    {
                      header: "Share",
                      numeric: true,
                      explain:
                        "Share: this type's share of the span's tokens, input and output together.",
                      render: (row) =>
                        total === 0 ? formatShare(0) : formatShare(row.value / total),
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
