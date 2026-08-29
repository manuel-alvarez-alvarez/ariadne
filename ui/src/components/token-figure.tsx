/**
 * What was spent, and by whom — the one way tokens are shown anywhere.
 *
 * A figure is always the pair: what went to the agent and what came back, as
 * `↑1.2M 89% ↓45k`. Two numbers rather than one because they are two different
 * costs and they move independently, and an arrow rather than a word because
 * this sits in a lane header, a table cell and a panel column, none of which
 * has room for "in" and "out" spelled out on every row. The arrows point the
 * way the tokens went: up is what was sent, down is what came back.
 *
 * The percent rides on the input half because it is a property of it: what
 * share of everything sent the cache served. Around nineteen of every twenty
 * input tokens are cache reads, so it is the number that says whether a figure
 * that looks expensive actually was — and reading it off the raw counts
 * means dividing one eight-digit number by another.
 *
 * What the arrows leave unsaid is in the hint behind it — the two halves
 * named, Input and Output, in the same rounded form the figure shows — and,
 * where the figure is a whole goal's or a whole task's, one line per agent
 * that spent part of it. That breakdown used to be a card above the sessions
 * list; it is a hover now, because it answers a question that is asked once
 * and read past forever.
 *
 * The figures are the daemon's own: the task and goal DTOs carry the split
 * already aggregated, and nothing here adds anything up. A total that
 * disagreed with the rows under it would be this component inventing the
 * disagreement.
 */

import { ArrowDownIcon, ArrowUpIcon } from "lucide-react"

import type { GoalUsage, TaskUsage, TokenUsage } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cachedShare, cn, formatTokens, shortId } from "@/lib/format"

/** One line of a total's breakdown: who spent it, and how much. */
interface UsageRow {
  /** Stable across renders: a profile id where there is one, the role otherwise. */
  key: string
  /** Who spent it — a role ("Planner", "Engineers") or one agent's name. */
  label: string
  usage: TokenUsage
}

/**
 * The layout of a figure: the two halves in a row, digits that line up.
 *
 * The gap between the halves is wider than the one inside them, which is what
 * makes the share read as part of the input rather than as a figure of its
 * own — see {@link TokenHalves}.
 */
const FIGURE = "inline-flex items-center gap-2 tabular-nums"

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
  /** The agents behind a total, listed under its named halves in the hint. */
  rows?: UsageRow[]
  className?: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className={cn(FIGURE, className)} />}>
        <TokenHalves usage={usage} />
      </TooltipTrigger>
      <TooltipContent className="flex-col items-start gap-2">
        <NamedHalves usage={usage} />
        {rows ? (
          // Full width whatever the block above it measures, so the figures
          // hang off the same right edge down the list however long the names
          // beside them are. The gap over them is what keeps them read as a
          // breakdown of the total rather than more lines of it.
          <dl className="w-full text-background/70">
            {rows.map((row) => (
              <div key={row.key} className="flex items-center gap-3">
                <dt className="truncate">{row.label}</dt>
                <dd className={cn(FIGURE, "ml-auto")}>
                  <TokenHalves usage={row.usage} />
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
 * hint are made of — and what the sessions table puts inside one, where a panel
 * has no room for a figure of its own — and a tooltip inside a tooltip is not a
 * thing.
 *
 * The arrows are decoration — the words they stand for are there for a screen
 * reader, so the element reads as "1.2M in, 89% cached, 45k out" and its text
 * comes out as that sentence rather than as three bare numbers.
 *
 * The share sits inside the input half's own group, dimmed to the arrow's
 * weight and closer to the count than the two halves are to each other. That
 * spacing is the whole reading: loosen it and the figure becomes three numbers
 * in a row, one of which happens to be a percentage of another.
 */
export function TokenHalves({ usage }: { usage: TokenUsage }) {
  return (
    <>
      <span className="inline-flex items-center gap-0.5">
        <ArrowUpIcon className="size-3 shrink-0 opacity-60" aria-hidden />
        {formatTokens(usage.input_tokens)}
        <span className="sr-only">{" in, "}</span>
        <span className="ml-0.5 opacity-60">{cachedShare(usage)}</span>
        <span className="sr-only">{" cached, "}</span>
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
 * The total again, with the words the arrows stand in for: two labels down the
 * left, the counts right-aligned against each other. Same figures as the
 * trigger, so the hint never shows a number the thing it hangs off disagrees
 * with — it spells out which half is which, and what the rows below it add up
 * towards.
 *
 * The share stays where it is on the figure, beside the input count and dimmed
 * against it. It is a property of that half rather than a count of its own, so
 * it is never given a label and never given a line.
 */
function NamedHalves({ usage }: { usage: TokenUsage }) {
  return (
    <div className="grid grid-cols-[auto_1fr_auto] gap-x-4 tabular-nums">
      <span>Input</span>
      <span className="text-right">{formatTokens(usage.input_tokens)}</span>
      <span className="text-background/70">{cachedShare(usage)}</span>
      <span>Output</span>
      <span className="text-right">{formatTokens(usage.output_tokens)}</span>
      {/* The share's column, empty on the half that has none. */}
      <span />
    </div>
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
