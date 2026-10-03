/**
 * The models panel of the Stats screen: how each model did in each seat, one
 * row per `(model, seat)` of `GET /v1/stats/models`.
 *
 * Two charts: sessions per model and seat, split into ended, failed and
 * stalled, then tokens spent per model and seat. The figures are the
 * daemon's own aggregates of its stats ledger, and nothing here adds
 * anything up. The dispatcher invalidates the `stats` group on every task and
 * session update, so a session that ends shows up in its bar without a
 * refresh.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, type ModelStatDto, qk, type StatsFilter, unwrap } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { formatDuration, formatTokens } from "@/lib/format"
import { SESSIONS_BARS, SESSIONS_CONFIG, TOKENS_CONFIG } from "./chart-configs"
import { StatSectionHeading } from "./section-heading"
import { StatBarChart } from "./stat-bar-chart"
import { StatQueryState } from "./stat-query-state"

/** One `(model, seat)` row, shaped for the two charts below. */
interface ModelRow {
  label: string
  model: string
  seat: string | null
  ended: number
  failed: number
  stalled: number
  tokens: number
  mean_lifetime_secs: number
  skills: ModelStatDto["skills"]
}

/** `label`: the model, with its seat beside it where the row has one. */
function rowLabel(row: ModelStatDto): string {
  return row.seat ? `${row.model} (${row.seat})` : row.model
}

function toRows(items: ModelStatDto[]): ModelRow[] {
  return items.map((row) => ({
    label: rowLabel(row),
    model: row.model,
    seat: row.seat ?? null,
    // `stalled` is a flag a run carried as it ended, not a status of its own,
    // so a session can be both `failed` and `stalled` — subtracting both from
    // `sessions` would then count that one session twice. A session's status
    // is exactly `exited` or `failed` with no overlap, so `ended` is the
    // exact rest of `sessions` once `failed` is taken out; `stalled` stays
    // its own, possibly-overlapping bar rather than a second subtraction.
    ended: Math.max(0, row.sessions - row.failed),
    failed: row.failed,
    stalled: row.stalled,
    tokens: row.usage.input_tokens + row.usage.output_tokens,
    mean_lifetime_secs: row.mean_lifetime_secs,
    skills: row.skills,
  }))
}

/** `GET /v1/stats/models`, narrowed by the screen's filter. */
function modelStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.models(filter),
    queryFn: () => unwrap(api().GET("/v1/stats/models", { params: { query: filter } })),
    select: (response) => toRows(response.items),
  })
}

function skillsLabel(skills: ModelStatDto["skills"]): string {
  return skills.length === 0
    ? "none"
    : skills.map((skill) => `${skill.name} ${skill.sessions}`).join(", ")
}

export function ModelsPanel({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(modelStatsQueryOptions(filter))
  const rows = stats.data ?? []

  return (
    <section aria-label="Models" className="flex flex-col gap-3">
      <StatSectionHeading>Models</StatSectionHeading>
      <StatQueryState query={stats} errorTitle="Could not load the model stats">
        {rows.length === 0 ? (
          <EmptyState emphasis="quiet" title="No session has ended in this span" />
        ) : (
          <>
            <StatBarChart
              data={[...rows].sort((a, b) => b.ended + b.failed - (a.ended + a.failed))}
              config={SESSIONS_CONFIG}
              bars={SESSIONS_BARS}
              caption="Sessions per model and seat, by how they ended"
              columns={[
                { header: "Model", render: (row) => row.label },
                { header: "Ended", render: (row) => row.ended },
                { header: "Failed", render: (row) => row.failed },
                { header: "Stalled", render: (row) => row.stalled },
              ]}
              tooltip={(row) => (
                <div className="flex flex-col gap-1">
                  <p className="font-medium">{row.label}</p>
                  <p>Ended: {row.ended}</p>
                  <p>Failed: {row.failed}</p>
                  <p>Stalled: {row.stalled}</p>
                  <p>Mean lifetime: {formatDuration(row.mean_lifetime_secs)}</p>
                  <p>Skills: {skillsLabel(row.skills)}</p>
                </div>
              )}
            />
            <StatBarChart
              data={[...rows].sort((a, b) => b.tokens - a.tokens)}
              config={TOKENS_CONFIG}
              bars={["tokens"]}
              caption="Tokens spent per model and seat"
              columns={[
                { header: "Model", render: (row) => row.label },
                { header: "Tokens", render: (row) => row.tokens },
              ]}
              tooltip={(row) => (
                <div className="flex flex-col gap-1">
                  <p className="font-medium">{row.label}</p>
                  <p>Tokens: {formatTokens(row.tokens)}</p>
                  <p>Mean lifetime: {formatDuration(row.mean_lifetime_secs)}</p>
                  <p>Skills: {skillsLabel(row.skills)}</p>
                </div>
              )}
            />
          </>
        )}
      </StatQueryState>
    </section>
  )
}
