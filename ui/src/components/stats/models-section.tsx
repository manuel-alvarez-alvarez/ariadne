/**
 * The Models section of the Stats screen: which model does the job? It reads
 * `GET /v1/stats/models` under the screen's filter and compares each seat —
 * a summary card of its totals, then a ranked bar chart of its own models,
 * rather than the full comparison table a model count once needed: a
 * model's exact tokens, time, messages and finished count ride in the bar's
 * tooltip and its `sr-only` table instead of a column each.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, type ModelStatsDto, qk, type StatsFilter, unwrap } from "@/api"
import { formatDuration, formatTokens } from "@/lib/format"
import { MODELS_AGENT_CONFIG, MODELS_ORCHESTRATOR_CONFIG } from "./chart-configs"
import { StatBarChart } from "./stat-bar-chart"
import { StatSection } from "./stat-section"
import { StatTile, StatTiles } from "./stat-tiles"

/** `GET /v1/stats/models`, narrowed by the screen's filter. */
function modelsStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.models(filter),
    queryFn: (): Promise<ModelStatsDto> =>
      unwrap(api().GET("/v1/stats/models", { params: { query: filter } })),
  })
}

type Model = ModelStatsDto["items"][number]

/** One seat's rows, ranked by its own primary count, heaviest first. */
function rankedRows(items: Model[], seat: string, primary: (row: Model) => number) {
  return items
    .filter((row) => row.seat === seat)
    .sort((a, b) => primary(b) - primary(a) || a.model.localeCompare(b.model))
}

/** A seat's rows, summed into the one card of totals above its chart. */
function sum(rows: Model[], pick: (row: Model) => number): number {
  return rows.reduce((total, row) => total + pick(row), 0)
}

export function ModelsSection({ filter, className }: { filter: StatsFilter; className?: string }) {
  const stats = useQuery(modelsStatsQueryOptions(filter))
  return (
    <StatSection
      title="Models"
      className={className}
      description="Which model does the job?"
      query={stats}
      isEmpty={(data) =>
        !data.items?.some((row) => row.seat === "agent" || row.seat === "orchestrator")
      }
      empty="No model ran in this span."
    >
      {(data) => {
        const agents = rankedRows(data.items, "agent", (row) => row.tasks)
        const orchestrators = rankedRows(data.items, "orchestrator", (row) => row.goals)
        return (
          <div className="flex flex-1 flex-col gap-4 md:flex-row">
            {agents.length ? <AgentSeat rows={agents} /> : null}
            {orchestrators.length ? <OrchestratorSeat rows={orchestrators} /> : null}
          </div>
        )
      }}
    </StatSection>
  )
}

function AgentSeat({ rows }: { rows: Model[] }) {
  const chartRows = rows.map((row) => ({
    label: row.model,
    tasks: row.tasks,
    tokens: row.tokens,
    time_secs: row.time_secs,
    messages: row.messages,
    tasks_finished: row.tasks_finished,
  }))
  return (
    <div className="flex flex-1 flex-col gap-2">
      <h3 className="text-xs font-medium">Agents</h3>
      <StatTiles>
        <StatTile
          label="Agent models"
          value={rows.length}
          explain="Agent models: the count of distinct models that ran as an agent in this span."
        />
        <StatTile
          label="Agent tasks"
          value={sum(rows, (row) => row.tasks)}
          explain="Agent tasks: the count of distinct tasks an agent model worked in this span."
        />
        <StatTile
          label="Agent tokens"
          value={formatTokens(sum(rows, (row) => row.tokens))}
          explain="Agent tokens: input and output tokens every agent model used in this span."
        />
        <StatTile
          label="Agent time"
          value={formatDuration(sum(rows, (row) => row.time_secs))}
          explain="Agent time: the summed session time every agent model ran in this span."
        />
      </StatTiles>
      <StatBarChart
        data={chartRows}
        config={MODELS_AGENT_CONFIG}
        bars={["tasks"]}
        legendExtra={["tokens", "time_secs", "messages", "tasks_finished"]}
        caption="Agent models ranked by tasks worked"
        tooltip={(row) => (
          <>
            <p className="font-medium">{row.label}</p>
            <p>Tasks: {row.tasks}</p>
            <p>Tokens: {formatTokens(row.tokens)}</p>
            <p>Time: {formatDuration(row.time_secs)}</p>
            <p>Messages: {row.messages}</p>
            <p>Finished: {row.tasks_finished}</p>
          </>
        )}
        columns={[
          { header: "Model", render: (row) => row.label },
          { header: "Tasks", render: (row) => row.tasks },
          { header: "Tokens", render: (row) => formatTokens(row.tokens) },
          { header: "Time", render: (row) => formatDuration(row.time_secs) },
          { header: "Messages", render: (row) => row.messages },
          { header: "Finished", render: (row) => row.tasks_finished },
        ]}
      />
    </div>
  )
}

function OrchestratorSeat({ rows }: { rows: Model[] }) {
  const chartRows = rows.map((row) => ({
    label: row.model,
    goals: row.goals,
    tokens: row.tokens,
    time_secs: row.time_secs,
    messages: row.messages,
  }))
  return (
    <div className="flex flex-1 flex-col gap-2">
      <h3 className="text-xs font-medium">Orchestrators</h3>
      <StatTiles>
        <StatTile
          label="Orchestrator models"
          value={rows.length}
          explain="Orchestrator models: the count of distinct models that ran as an orchestrator in this span."
        />
        <StatTile
          label="Orchestrator goals"
          value={sum(rows, (row) => row.goals)}
          explain="Orchestrator goals: the count of distinct goals an orchestrator model ran in this span."
        />
        <StatTile
          label="Orchestrator tokens"
          value={formatTokens(sum(rows, (row) => row.tokens))}
          explain="Orchestrator tokens: input and output tokens every orchestrator model used in this span."
        />
        <StatTile
          label="Orchestrator time"
          value={formatDuration(sum(rows, (row) => row.time_secs))}
          explain="Orchestrator time: the summed session time every orchestrator model ran in this span."
        />
      </StatTiles>
      <StatBarChart
        data={chartRows}
        config={MODELS_ORCHESTRATOR_CONFIG}
        bars={["goals"]}
        legendExtra={["tokens", "time_secs", "messages"]}
        caption="Orchestrator models ranked by goals run"
        tooltip={(row) => (
          <>
            <p className="font-medium">{row.label}</p>
            <p>Goals: {row.goals}</p>
            <p>Tokens: {formatTokens(row.tokens)}</p>
            <p>Time: {formatDuration(row.time_secs)}</p>
            <p>Messages: {row.messages}</p>
          </>
        )}
        columns={[
          { header: "Model", render: (row) => row.label },
          { header: "Goals", render: (row) => row.goals },
          { header: "Tokens", render: (row) => formatTokens(row.tokens) },
          { header: "Time", render: (row) => formatDuration(row.time_secs) },
          { header: "Messages", render: (row) => row.messages },
        ]}
      />
    </div>
  )
}
