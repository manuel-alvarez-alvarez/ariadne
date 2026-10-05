/**
 * The Models section of the Stats screen: which model does the job? It reads
 * `GET /v1/stats/models` under the screen's filter and compares each seat.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"

import { api, type ModelStatsDto, qk, type StatsFilter, unwrap } from "@/api"
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
type Column = {
  header: string
  numeric?: boolean
  explain: string
  render: (row: Model) => ReactNode
}
const mean = (value: number | null | undefined) => (value ?? 0).toFixed(1)
const model: Column = {
  header: "MODEL",
  explain: "MODEL: the name of the model this row is about.",
  render: (row) => row.model,
}
const work: Column[] = [
  {
    header: "TOKENS",
    numeric: true,
    explain:
      "TOKENS: input and output tokens this model used in this seat, cache not counted twice.",
    render: (row) => formatTokens(row.tokens),
  },
  {
    header: "TIME",
    numeric: true,
    explain: "TIME: the summed session time this model ran in this seat.",
    render: (row) => formatDuration(row.time_secs),
  },
  {
    header: "MESSAGES",
    numeric: true,
    explain: "MESSAGES: the count of messages this model sent in this seat.",
    render: (row) => row.messages,
  },
]
const tasks: Column = {
  header: "TASKS",
  numeric: true,
  explain: "TASKS: the count of distinct tasks this model worked as this seat.",
  render: (row) => row.tasks,
}
const groups: { seat: string; title: string; columns: Column[]; count: (row: Model) => number }[] =
  [
    {
      seat: "author",
      title: "Authors",
      count: (row) => row.tasks,
      columns: [
        model,
        tasks,
        ...work,
        {
          header: "ROUNDS/TASK",
          numeric: true,
          explain: "ROUNDS/TASK: the mean number of review requests on each finished task.",
          render: (row) => mean(row.rounds_per_task),
        },
      ],
    },
    {
      seat: "reviewer",
      title: "Reviewers",
      count: (row) => row.tasks,
      columns: [
        model,
        tasks,
        ...work,
        {
          header: "CHANGES/TASK",
          numeric: true,
          explain:
            "CHANGES/TASK: the mean number of changes-requested verdicts on each task this model reviewed.",
          render: (row) => mean(row.changes_per_task),
        },
      ],
    },
    {
      seat: "orchestrator",
      title: "Orchestrators",
      count: (row) => row.goals,
      columns: [
        model,
        {
          header: "GOALS",
          numeric: true,
          explain: "GOALS: the count of distinct goals this model orchestrated.",
          render: (row) => row.goals,
        },
        ...work,
      ],
    },
  ]
