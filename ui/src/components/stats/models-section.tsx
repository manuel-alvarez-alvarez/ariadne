/**
 * The Models section of the Stats screen: which model does the job? It reads
 * `GET /v1/stats/models` under the screen's filter and compares each seat.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"

import { api, type ModelStatsDto, qk, type StatsFilter, unwrap } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatDuration, formatTokens } from "@/lib/format"
import { StatSection } from "./stat-section"
import { StatTable } from "./stat-table"

/** `GET /v1/stats/models`, narrowed by the screen's filter. */
function modelsStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.models(filter),
    queryFn: (): Promise<ModelStatsDto> =>
      unwrap(api().GET("/v1/stats/models", { params: { query: filter } })),
  })
}

export function ModelsSection({ filter, className }: { filter: StatsFilter; className?: string }) {
  const stats = useQuery(modelsStatsQueryOptions(filter))
  return (
    <StatSection
      title="Models"
      className={className}
      description="Which model does the job?"
      query={stats}
      isEmpty={(data) => !data.items?.some((row) => groups.some(({ seat }) => row.seat === seat))}
      empty="No model ran in this span."
    >
      {(data) => (
        <>
          {groups.map(({ seat, title, columns, count }) => {
            const rows = data.items
              .filter((row) => row.seat === seat)
              .sort((a, b) => count(b) - count(a) || a.model.localeCompare(b.model))
            if (!rows.length) return null
            return (
              <div key={seat} className="flex flex-col gap-2">
                <h3 className="text-xs font-medium">{title}</h3>
                <StatTable
                  rows={rows}
                  rowKey={(row) => row.model}
                  columns={columns}
                  caption={title}
                />
              </div>
            )
          })}
        </>
      )}
    </StatSection>
  )
}

type Model = ModelStatsDto["items"][number]
type Column = { header: string; numeric?: boolean; render: (row: Model) => ReactNode }
const percent = (rate: number) => `${(rate * 100).toFixed(1)}%`
const tasks = (row: Model) =>
  (row.author?.tasks_finished ?? 0) +
  (row.author?.tasks_failed ?? 0) +
  (row.author?.tasks_cancelled ?? 0)
const model: Column = { header: "MODEL", render: (row) => row.model }
/** The total of a row's interventions, with its breakdown behind a hover or a focus. */
function Interventions({ row }: { row: Model }) {
  const { permissions, questions, stalls, total, person_secs } = row.interventions
  return (
    <Tooltip>
      <TooltipTrigger render={<span />}>{total}</TooltipTrigger>
      <TooltipContent>
        <dl className="grid grid-cols-[auto_auto] gap-x-3 tabular-nums">
          <dt>Permissions</dt>
          <dd className="text-right">{permissions}</dd>
          <dt>Questions</dt>
          <dd className="text-right">{questions}</dd>
          <dt>Stalls</dt>
          <dd className="text-right">{stalls}</dd>
          <dt>Person time</dt>
          <dd className="text-right">{formatDuration(person_secs)}</dd>
        </dl>
      </TooltipContent>
    </Tooltip>
  )
}

const failures: Column[] = [
  { header: "FAILED", numeric: true, render: (row) => row.failed_sessions },
  { header: "EXHAUSTED", numeric: true, render: (row) => row.exhaustions },
]
const time: Column[] = [
  { header: "INTERVENTIONS", numeric: true, render: (row) => <Interventions row={row} /> },
  { header: "TOTAL_TIME", numeric: true, render: (row) => formatDuration(row.total_lifetime_secs) },
]
const groups: { seat: string; title: string; columns: Column[]; count: (row: Model) => number }[] =
  [
    {
      seat: "author",
      title: "Authors",
      count: tasks,
      columns: [
        model,
        { header: "TASKS", numeric: true, render: tasks },
        {
          header: "FINISH_RATE",
          numeric: true,
          render: (row) => percent(row.author?.finish_rate ?? 0),
        },
        {
          header: "FIRST_PASS",
          numeric: true,
          render: (row) => percent(row.author?.first_pass_rate ?? 0),
        },
        {
          header: "ROUNDS",
          numeric: true,
          render: (row) => (row.author?.mean_review_rounds ?? 0).toFixed(1),
        },
        { header: "WIN_RATE", numeric: true, render: (row) => percent(row.author?.win_rate ?? 0) },
        {
          header: "TOKENS/TASK",
          numeric: true,
          render: (row) => formatTokens(row.author?.tokens_per_finished_task ?? 0),
        },
        {
          header: "INTERVENTIONS/TASK",
          numeric: true,
          render: (row) => row.author?.interventions_per_finished_task?.toFixed(1) ?? "-",
        },
        ...failures,
        ...time,
      ],
    },
    {
      seat: "reviewer",
      title: "Reviewers",
      count: (row) => row.reviewer?.verdicts ?? 0,
      columns: [
        model,
        { header: "VERDICTS", numeric: true, render: (row) => row.reviewer?.verdicts ?? 0 },
        {
          header: "APPROVE",
          numeric: true,
          render: (row) => percent(row.reviewer?.approve_share ?? 0),
        },
        {
          header: "LATENCY",
          numeric: true,
          render: (row) => formatDuration(row.reviewer?.mean_latency_secs ?? 0),
        },
        ...failures,
        ...time,
      ],
    },
    {
      seat: "orchestrator",
      title: "Orchestrators",
      count: (row) => row.sessions,
      columns: [
        model,
        { header: "SESSIONS", numeric: true, render: (row) => row.sessions },
        {
          header: "TOKENS",
          numeric: true,
          render: (row) => formatTokens(row.usage.input_tokens + row.usage.output_tokens),
        },
        {
          header: "LIFETIME",
          numeric: true,
          render: (row) => formatDuration(row.mean_lifetime_secs),
        },
        ...failures,
        ...time,
      ],
    },
  ]
