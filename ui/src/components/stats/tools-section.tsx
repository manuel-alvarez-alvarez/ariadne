/**
 * The Tools section of the Stats screen: what do the agents do? It reads
 * `GET /v1/stats/tools` under the screen's filter, and draws the tool mix by
 * kind and the top tools, ok and errors stacked in both.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, type ToolStatsDto, unwrap } from "@/api"
import { TOOL_CALLS_CONFIG } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatSection } from "./stat-section"
import { StatTile, StatTiles } from "./stat-tiles"

/** The most top tools the chart asks for and draws, the `other` row aside. */
const LIMIT = 10

/** `GET /v1/stats/tools`, narrowed by the screen's filter and this section's
 * own `limit`. */
function toolsStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.tools(filter),
    queryFn: (): Promise<ToolStatsDto> =>
      unwrap(api().GET("/v1/stats/tools", { params: { query: { ...filter, limit: LIMIT } } })),
  })
}

/** `value`, in milliseconds, as a person reads it: `25ms`. */
function ms(value: number): string {
  return `${Math.round(value)}ms`
}

interface BarRow {
  label: string
  kind: string
  ok: number
  errors: number
  medianMs: number
  p90Ms: number
}

function kindRows(stats: ToolStatsDto): BarRow[] {
  return stats.by_kind.map((row) => ({
    label: row.kind,
    kind: row.kind,
    ok: row.calls - row.errors,
    errors: row.errors,
    medianMs: row.median_duration_ms,
    p90Ms: row.p90_duration_ms,
  }))
}

/** The top tools, the `other` row last where there is one beyond them —
 * never more than `LIMIT` plus that one row, whatever the answer holds. */
function toolRows(stats: ToolStatsDto): BarRow[] {
  const rows = stats.top.slice(0, LIMIT).map((row) => ({
    label: row.tool_name,
    kind: row.kind,
    ok: row.calls - row.errors,
    errors: row.errors,
    medianMs: row.median_duration_ms,
    p90Ms: row.p90_duration_ms,
  }))
  if (stats.other.tools > 0) {
    rows.push({
      label: "other",
      kind: `${stats.other.tools} tools`,
      ok: stats.other.calls - stats.other.errors,
      errors: stats.other.errors,
      medianMs: 0,
      p90Ms: 0,
    })
  }
  return rows
}

function tooltip(row: BarRow) {
  return (
    <div className="flex flex-col gap-0.5">
      <div className="font-medium">{row.label}</div>
      <div>
        {row.ok + row.errors} calls, {row.errors} errors
      </div>
      <div>
        median {ms(row.medianMs)}, p90 {ms(row.p90Ms)}
      </div>
    </div>
  )
}

const COLUMNS = [
  { header: "Name", render: (row: BarRow) => row.label },
  { header: "Kind", render: (row: BarRow) => row.kind },
  { header: "Calls", render: (row: BarRow) => row.ok + row.errors },
  { header: "Errors", render: (row: BarRow) => row.errors },
  { header: "Median", render: (row: BarRow) => ms(row.medianMs) },
  { header: "P90", render: (row: BarRow) => ms(row.p90Ms) },
]

export function ToolsSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(toolsStatsQueryOptions(filter))
  return (
    <StatSection
      title="Tools"
      description="What do the agents do?"
      query={stats}
      isEmpty={(data) => data.calls === 0}
      empty="No tool ran in this span."
    >
      {(data) => (
        <div className="flex flex-col gap-4">
          <StatTiles>
            <StatTile label="Calls" value={data.calls} />
            <StatTile label="Errors" value={data.errors} />
            <StatTile
              label="Error rate"
              value={data.calls === 0 ? "0%" : `${Math.round((data.errors / data.calls) * 100)}%`}
            />
            <StatTile label="Tools" value={data.tools} />
          </StatTiles>
          <StatBarChart
            data={kindRows(data)}
            config={TOOL_CALLS_CONFIG}
            bars={[["ok", "errors"]]}
            caption="Calls per kind, ok and errors"
            columns={COLUMNS}
            tooltip={tooltip}
          />
          <StatBarChart
            data={toolRows(data)}
            config={TOOL_CALLS_CONFIG}
            bars={[["ok", "errors"]]}
            caption="Calls per tool, ok and errors"
            columns={COLUMNS}
            tooltip={tooltip}
          />
        </div>
      )}
    </StatSection>
  )
}
