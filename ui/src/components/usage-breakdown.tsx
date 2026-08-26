/**
 * What was spent, and by whom.
 *
 * Two pieces, because every surface that shows tokens shows one of them: a
 * single figure — a session's, a task's, a goal's — and the breakdown of a
 * total into the agents behind it. A task's engineer and its reviewers and a
 * goal's planner, engineers and reviewers are the same block with different
 * rows, so they are the same block.
 *
 * The figures are the daemon's own: the task and goal DTOs carry the split
 * already aggregated, and nothing here adds anything up. A total that
 * disagreed with the rows under it would be this component inventing the
 * disagreement.
 *
 * Compact wherever it is read — `1.2M/45.3k` is what fits a table cell and a
 * panel column — with the sentence behind it in the hint, spelled out to the
 * digit so the numbers can be checked against what `ariadne … inspect` prints.
 */

import type { ReactNode } from "react"

import type { TokenUsage } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn, formatTokens, usageSummary } from "@/lib/format"

/**
 * One entity's tokens: `in/out`, or the whole sentence where there is room for
 * it. Cached input never makes the visible form — it is a subset of the input
 * beside it, so it explains a figure rather than being one — and rides in the
 * hint with the exact counts.
 */
export function TokenFigure({
  usage,
  summary = false,
  className,
}: {
  usage: TokenUsage
  /** Spell it out — `in 1.2M (cached 1.1M) · out 45.3k` — where the width allows. */
  summary?: boolean
  className?: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className={cn("truncate tabular-nums", className)} />}>
        {summary
          ? usageSummary(usage)
          : `${formatTokens(usage.input_tokens)}/${formatTokens(usage.output_tokens)}`}
      </TooltipTrigger>
      <TooltipContent>{usageSummary(usage, { exact: true })}</TooltipContent>
    </Tooltip>
  )
}

/** One line of a breakdown: a role, the agent that filled it, and its figure. */
export interface UsageRow {
  /** Stable across renders: a profile id where there is one, the role otherwise. */
  key: string
  /** The role that spent it — "Engineer", "Planner", "Reviewers". */
  role: string
  /**
   * Which agent, where one row is one of them: a `ProfileSummary`, so a
   * reviewer reads here the way it reads in the task's facts. Left out where
   * the row is a whole role of a goal, which is as many agents as it has tasks.
   */
  who?: ReactNode
  usage: TokenUsage
}

/**
 * The total and the agents under it, as the block above a sessions list.
 *
 * The total leads, since it is the number the panel is opened for, and the
 * rows are what it is made of. A role that has spent nothing is still a row:
 * "the reviewers have used nothing yet" is an answer, and a list that drops
 * its empty rows makes the reader count the ones that are missing.
 */
export function UsageBreakdown({ total, rows }: { total: TokenUsage; rows: UsageRow[] }) {
  return (
    <section className="rounded-lg border bg-card p-3 text-xs">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h3 className="font-medium">Tokens</h3>
        <TokenFigure usage={total} summary className="text-muted-foreground" />
      </div>
      <dl className="mt-2 space-y-1">
        {rows.map((row) => (
          <div key={row.key} className="flex min-w-0 items-baseline gap-2">
            <dt className="flex min-w-0 items-baseline gap-1.5 text-muted-foreground">
              <span className="shrink-0">{row.role}</span>
              {row.who}
            </dt>
            {/* Pushed to the right edge so the figures line up down the block
                whatever the names beside them are; `shrink-0` keeps a long
                profile line truncating instead of squeezing the number. */}
            <dd className="ml-auto shrink-0">
              <TokenFigure usage={row.usage} />
            </dd>
          </div>
        ))}
      </dl>
    </section>
  )
}
