/** Tool calls and permission answers from `GET /v1/stats/tools`. */

import { queryOptions, useQuery } from "@tanstack/react-query"

import {
  api,
  type PermissionStatDto,
  qk,
  type StatsFilter,
  type ToolModelStatDto,
  type ToolStatDto,
  unwrap,
} from "@/api"
import { EmptyState } from "@/components/empty-state"
import { PERMISSIONS_CONFIG, TOOL_MODELS_CONFIG, TOOLS_CONFIG } from "./chart-configs"
import { StatSectionHeading } from "./section-heading"
import { StatBarChart } from "./stat-bar-chart"
import { StatQueryState } from "./stat-query-state"

interface ToolRow {
  label: string
  ok: number
  errors: number
  median_duration_ms: number
  p90_duration_ms: number
}

interface ToolModelRow {
  label: string
  calls: number
  mean_duration_ms: number
}

interface PermissionRow {
  label: string
  allow: number
  deny: number
  cancelled: number
  meanWait: { allow?: number; deny?: number; cancelled?: number }
}

const ms = (value: number) => `${Math.round(value)}ms`

function toolRows(tools: ToolStatDto[]): ToolRow[] {
  return tools.map((row) => ({
    label: row.tool_name,
    ok: Math.max(0, row.calls - row.errors),
    errors: row.errors,
    median_duration_ms: row.median_duration_ms,
    p90_duration_ms: row.p90_duration_ms,
  }))
}

function toolModelRows(models: ToolModelStatDto[]): ToolModelRow[] {
  return models.map((row) => ({
    label: row.model,
    calls: row.calls,
    mean_duration_ms: row.mean_duration_ms,
  }))
}

function permissionRows(permissions: PermissionStatDto[]): PermissionRow[] {
  const byDecider = new Map<string, PermissionRow>()
  for (const row of permissions) {
    const entry = byDecider.get(row.decided_by) ?? {
      label: row.decided_by,
      allow: 0,
      deny: 0,
      cancelled: 0,
      meanWait: {},
    }
    const answer = row.answer === "allow" || row.answer === "deny" ? row.answer : "cancelled"
    entry[answer] = row.permissions
    entry.meanWait[answer] = row.mean_wait_ms
    byDecider.set(row.decided_by, entry)
  }
  return [...byDecider.values()]
}

function toolsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.tools(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/tools", { params: { query: filter } })),
    select: (response) => ({
      tools: toolRows(response.tools),
      models: toolModelRows(response.models),
      permissions: permissionRows(response.permissions),
    }),
  })
}

export function ToolsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(toolsQueryOptions(filter))
  const tools = stats.data?.tools ?? []
  const models = stats.data?.models ?? []
  const permissions = stats.data?.permissions ?? []
  // Three independent facts, so each can be empty on its own: a permission
  // can be denied with no completed tool call behind it, which would leave
  // `tools` empty while `permissions` still has rows to chart.
  const empty = tools.length === 0 && models.length === 0 && permissions.length === 0

  return (
    <section aria-label="Tools" className="flex flex-col gap-3">
      <StatSectionHeading>Tools</StatSectionHeading>
      <StatQueryState query={stats} errorTitle="Could not load the tool stats">
        {empty ? (
          <EmptyState emphasis="quiet" title="No tool calls in this span" />
        ) : (
          <>
            {tools.length === 0 ? null : (
              <StatBarChart
                data={[...tools].sort((a, b) => b.ok + b.errors - (a.ok + a.errors))}
                config={TOOLS_CONFIG}
                bars={[["ok", "errors"]]}
                caption="Calls per tool, errors included"
                columns={[
                  { header: "Tool", render: (row) => row.label },
                  { header: "Calls", render: (row) => row.ok + row.errors },
                  { header: "Errors", render: (row) => row.errors },
                  { header: "Median", render: (row) => ms(row.median_duration_ms) },
                  { header: "P90", render: (row) => ms(row.p90_duration_ms) },
                ]}
                tooltip={(row) => (
                  <div className="flex flex-col gap-1">
                    <p className="font-medium">{row.label}</p>
                    <p>Calls: {row.ok + row.errors}</p>
                    <p>Errors: {row.errors}</p>
                    <p>Median: {ms(row.median_duration_ms)}</p>
                    <p>P90: {ms(row.p90_duration_ms)}</p>
                  </div>
                )}
              />
            )}
            {permissions.length === 0 ? null : (
              <StatBarChart
                data={[...permissions].sort(
                  (a, b) => b.allow + b.deny + b.cancelled - (a.allow + a.deny + a.cancelled),
                )}
                config={PERMISSIONS_CONFIG}
                bars={[["allow", "deny", "cancelled"]]}
                caption="Permission answers per decider"
                columns={[
                  { header: "Decided by", render: (row) => row.label },
                  { header: "Allow", render: (row) => row.allow },
                  { header: "Deny", render: (row) => row.deny },
                  { header: "Cancelled", render: (row) => row.cancelled },
                ]}
                tooltip={(row) => (
                  <div className="flex flex-col gap-1">
                    <p className="font-medium">{row.label}</p>
                    <p>
                      Allow: {row.allow}
                      {row.meanWait.allow !== undefined ? ` (${ms(row.meanWait.allow)})` : ""}
                    </p>
                    <p>
                      Deny: {row.deny}
                      {row.meanWait.deny !== undefined ? ` (${ms(row.meanWait.deny)})` : ""}
                    </p>
                    <p>
                      Cancelled: {row.cancelled}
                      {row.meanWait.cancelled !== undefined
                        ? ` (${ms(row.meanWait.cancelled)})`
                        : ""}
                    </p>
                  </div>
                )}
              />
            )}
            {models.length === 0 ? null : (
              <StatBarChart
                data={[...models].sort((a, b) => b.calls - a.calls)}
                config={TOOL_MODELS_CONFIG}
                bars={["calls"]}
                caption="Calls per model"
                columns={[
                  { header: "Model", render: (row) => row.label },
                  { header: "Calls", render: (row) => row.calls },
                  { header: "Mean", render: (row) => ms(row.mean_duration_ms) },
                ]}
                tooltip={(row) => (
                  <div className="flex flex-col gap-1">
                    <p className="font-medium">{row.label}</p>
                    <p>Calls: {row.calls}</p>
                    <p>Mean: {ms(row.mean_duration_ms)}</p>
                  </div>
                )}
              />
            )}
          </>
        )}
      </StatQueryState>
    </section>
  )
}
