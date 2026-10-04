/**
 * The Spend section of the Stats screen: what did it spend? Tokens over
 * time, by model, and per finished task — never a cost. It reads
 * `GET /v1/stats/spend` under the screen's filter.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type SpendStatsDto, type StatsFilter, unwrap } from "@/api"
import { cachedShare, formatTokens, plural } from "@/lib/format"
import { SPEND_CONFIG } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatSection } from "./stat-section"
import { StatTile, StatTiles } from "./stat-tiles"
import { StatTimeChart } from "./stat-time-chart"

/** `GET /v1/stats/spend`, narrowed by the screen's filter. */
function spendStatsQueryOptions(filter: StatsFilter) {
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

type ModelRow = {
  label: string
  input_tokens: number
  output_tokens: number
  cached_input_tokens: number
  share: number
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
        const modelRows: ModelRow[] = data.by_model.map((model) => ({
          label: model.model,
          input_tokens: model.input_tokens,
          output_tokens: model.output_tokens,
          cached_input_tokens: model.cached_input_tokens,
          share: model.share,
        }))
        const perTask = data.per_finished_task.input_tokens + data.per_finished_task.output_tokens

        return (
          <div className="flex flex-col gap-4">
            <StatTiles>
              <StatTile label="Input tokens" value={formatTokens(data.totals.input_tokens)} />
              <StatTile label="Cache share" value={formatShare(data.totals.cached_share)} />
              <StatTile label="Output tokens" value={formatTokens(data.totals.output_tokens)} />
              <StatTile
                label="Tokens per finished task"
                value={formatTokens(perTask)}
                hint={`of ${plural(data.per_finished_task.tasks, "task")}`}
              />
            </StatTiles>
            <StatTimeChart
              data={timeRows}
              config={SPEND_CONFIG}
              keys={["input_tokens", "output_tokens"]}
              caption="Tokens spent over time"
              valueFormatter={formatTokens}
              tooltipExtra={(row) => <p>Cached: {cachedShare(row)}</p>}
            />
            <StatBarChart
              data={modelRows}
              config={SPEND_CONFIG}
              bars={[["input_tokens", "output_tokens"]]}
              caption="Tokens spent by model"
              tooltip={(row) => (
                <div className="flex flex-col gap-1">
                  <p className="font-medium">{row.label}</p>
                  <p>Input: {formatTokens(row.input_tokens)}</p>
                  <p>Cached: {cachedShare(row)}</p>
                  <p>Output: {formatTokens(row.output_tokens)}</p>
                  <p>Share: {formatShare(row.share)}</p>
                </div>
              )}
              columns={[
                { header: "Model", render: (row) => row.label },
                { header: "Input", render: (row) => formatTokens(row.input_tokens) },
                { header: "Cached", render: (row) => cachedShare(row) },
                { header: "Output", render: (row) => formatTokens(row.output_tokens) },
                { header: "Share", render: (row) => formatShare(row.share) },
              ]}
            />
          </div>
        )
      }}
    </StatSection>
  )
}
