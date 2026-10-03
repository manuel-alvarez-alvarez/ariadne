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
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { TableCell, TableRow } from "@/components/ui/table"

const TOOLS = [
  { header: "Tool" },
  { header: "Calls", className: "text-right" },
  { header: "Errors", className: "text-right" },
  { header: "Median", className: "text-right" },
  { header: "P90", className: "text-right" },
]
const MODELS = [
  { header: "Model" },
  { header: "Calls", className: "text-right" },
  { header: "Mean", className: "text-right" },
]
const PERMISSIONS = [
  { header: "Decided by" },
  { header: "Answer" },
  { header: "Total", className: "text-right" },
  { header: "Mean wait", className: "text-right" },
]

function toolsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.tools(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/tools", { params: { query: filter } })),
  })
}

const ms = (value: number) => `${Math.round(value)}ms`

export function ToolsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(toolsQueryOptions(filter))
  const tools = { ...stats, data: stats.data?.tools }
  const models = { ...stats, data: stats.data?.models }
  const permissions = { ...stats, data: stats.data?.permissions }
  const empty = (
    <EmptyState
      title="No tool calls in this span"
      description="A row appears once a tool call ends."
    />
  )
  return (
    <section aria-label="Tools" className="flex flex-col gap-4">
      <h2 className="font-heading text-sm font-semibold">Tools</h2>
      <DataTable
        query={tools}
        errorTitle="Could not load the tool stats"
        columns={TOOLS}
        empty={empty}
        rowKey={(row) => row.tool_name}
        renderRow={(row) => <ToolRow row={row} />}
      />
      <DataTable
        query={models}
        errorTitle="Could not load the tool stats"
        columns={MODELS}
        empty={empty}
        rowKey={(row) => row.model}
        renderRow={(row) => <ModelRow row={row} />}
      />
      <DataTable
        query={permissions}
        errorTitle="Could not load the tool stats"
        columns={PERMISSIONS}
        empty={empty}
        rowKey={(row) => `${row.decided_by} ${row.answer}`}
        renderRow={(row) => <PermissionRow row={row} />}
      />
    </section>
  )
}

function ToolRow({ row }: { row: ToolStatDto }) {
  return (
    <TableRow>
      <TableCell className="font-mono text-xs">{row.tool_name}</TableCell>
      <TableCell className="text-right tabular-nums">{row.calls}</TableCell>
      <TableCell className="text-right tabular-nums">{row.errors}</TableCell>
      <TableCell className="text-right tabular-nums">{ms(row.median_duration_ms)}</TableCell>
      <TableCell className="text-right tabular-nums">{ms(row.p90_duration_ms)}</TableCell>
    </TableRow>
  )
}
function ModelRow({ row }: { row: ToolModelStatDto }) {
  return (
    <TableRow>
      <TableCell className="font-mono text-xs">{row.model}</TableCell>
      <TableCell className="text-right tabular-nums">{row.calls}</TableCell>
      <TableCell className="text-right tabular-nums">{ms(row.mean_duration_ms)}</TableCell>
    </TableRow>
  )
}
function PermissionRow({ row }: { row: PermissionStatDto }) {
  return (
    <TableRow>
      <TableCell>{row.decided_by}</TableCell>
      <TableCell>{row.answer}</TableCell>
      <TableCell className="text-right tabular-nums">{row.permissions}</TableCell>
      <TableCell className="text-right tabular-nums">{ms(row.mean_wait_ms)}</TableCell>
    </TableRow>
  )
}
