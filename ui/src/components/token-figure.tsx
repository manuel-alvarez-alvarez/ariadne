/**
 * What was spent, and by whom — the one way tokens are shown anywhere.
 *
 * A figure is always the pair: what went to the agent and what came back, as
 * `↑1.2M ↓45k`. Two numbers rather than one because they are two different
 * costs and they move independently, and an arrow rather than a word because
 * this sits in a lane header, a table cell and a panel column, none of which
 * has room for "in" and "out" spelled out on every row. The arrows point the
 * way the tokens went: up is what was sent, down is what came back.
 *
 * Everything the compact form drops is in the hint behind it — the exact
 * counts, the cached share of the input, and, where the figure is a whole
 * goal's or a whole task's, one line per agent that spent part of it. That
 * breakdown used to be a card above the sessions list; it is a hover now,
 * because it answers a question that is asked once and read past forever.
 *
 * The figures are the daemon's own: the task and goal DTOs carry the split
 * already aggregated, and nothing here adds anything up. A total that
 * disagreed with the rows under it would be this component inventing the
 * disagreement.
 */

import { ArrowDownIcon, ArrowUpIcon } from "lucide-react"

import type { GoalUsage, TaskUsage, TokenUsage } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn, formatTokens, shortId, usageSummary } from "@/lib/format"

/** One line of a total's breakdown: who spent it, and how much. */
interface UsageRow {
  /** Stable across renders: a profile id where there is one, the role otherwise. */
  key: string
  /** Who spent it — a role ("Planner", "Engineers") or one agent's name. */
  label: string
  usage: TokenUsage
}

/** The layout of a figure: the two halves in a row, digits that line up. */
const FIGURE = "inline-flex items-center gap-1.5 tabular-nums"

/**
 * One entity's tokens: a session's, a task's, a goal's.
 *
 * Nothing renders a figure any other way, so a count reads the same in a table
 * cell as it does in a panel — and the pair is always both halves, zeros
 * included. A session that has reported nothing has spent nothing, which is a
 * number; a dash would say the daemon has no answer.
 */
export function TokenFigure({
  usage,
  rows,
  className,
}: {
  usage: TokenUsage
  /** The agents behind a total, listed under the exact counts in the hint. */
  rows?: UsageRow[]
  className?: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className={cn(FIGURE, className)} />}>
        <Halves usage={usage} />
      </TooltipTrigger>
      <TooltipContent className="flex-col items-start gap-0.5">
        <span>{usageSummary(usage)}</span>
        {rows ? (
          // Full width against the sentence above, which is always the widest
          // line of the hint, so the figures line up down the block whatever
          // the names beside them are.
          <dl className="w-full text-background/70">
            {rows.map((row) => (
              <div key={row.key} className="flex items-center gap-3">
                <dt className="truncate">{row.label}</dt>
                <dd className={cn(FIGURE, "ml-auto")}>
                  <Halves usage={row.usage} />
                </dd>
              </div>
            ))}
          </dl>
        ) : null}
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * The pair itself, without a hint of its own: it is also what the rows of a
 * hint are made of, and a tooltip inside a tooltip is not a thing.
 *
 * The arrows are decoration — the words they stand for are there for a screen
 * reader, so the element reads as "1.2M in, 45k out" and its text comes out as
 * that sentence rather than as two bare numbers.
 */
function Halves({ usage }: { usage: TokenUsage }) {
  return (
    <>
      <span className="inline-flex items-center gap-0.5">
        <ArrowUpIcon className="size-3 shrink-0 opacity-60" aria-hidden />
        {formatTokens(usage.input_tokens)}
        <span className="sr-only">{" in, "}</span>
      </span>
      <span className="inline-flex items-center gap-0.5">
        <ArrowDownIcon className="size-3 shrink-0 opacity-60" aria-hidden />
        {formatTokens(usage.output_tokens)}
        <span className="sr-only">{" out"}</span>
      </span>
    </>
  )
}

/**
 * A goal's three roles, in the order the work goes through them: the planner
 * that wrote the tasks, the engineers that did them, the reviewers that read
 * them. No names, because past the planner each role is as many agents as the
 * goal has tasks — the task panels are where those are.
 *
 * A role that has spent nothing is still a line: "the reviewers have used
 * nothing yet" is an answer, and a list that drops its empty lines makes the
 * reader work out which ones are missing.
 */
export function goalUsageRows(usage: GoalUsage): UsageRow[] {
  return [
    { key: "planner", label: "Planner", usage: usage.planner },
    { key: "engineers", label: "Engineers", usage: usage.engineers },
    { key: "reviewers", label: "Reviewers", usage: usage.reviewers },
  ]
}

/**
 * A task's agents: its engineer, then each reviewer the daemon has usage for —
 * which is every reviewer that has been spawned, and only those. A reviewer
 * replaced after it had already run keeps its line, since what it spent is
 * still in the total above it; the daemon sends the name it ran under, and the
 * tail of its profile id stands in where it sent none.
 */
export function taskUsageRows(usage: TaskUsage): UsageRow[] {
  return [
    { key: "engineer", label: "Engineer", usage: usage.engineer },
    ...usage.reviewers.map((reviewer) => ({
      key: reviewer.profile_id,
      label: reviewer.profile_name ?? shortId(reviewer.profile_id),
      usage: reviewer.usage,
    })),
  ]
}
