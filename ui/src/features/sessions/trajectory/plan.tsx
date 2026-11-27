/**
 * The plans of the agent in the turn console (040, rule 32).
 *
 * While a plan has an open entry, one plan line shows in the dock above the
 * composer: the count of the done entries, and the current entry. The line
 * is a disclosure that opens to the checklist. A plan with every entry done
 * shows in the turn that last changed it, as the same line, closed.
 *
 * A removed plan shows nowhere, and nor does a plan with no entries, such as
 * a `markdown` plan.
 */

import { ChevronDownIcon, ChevronRightIcon, ListChecksIcon } from "lucide-react"

import type { TrajectorySnapshotDto } from "@/api"
import { cn } from "@/lib/format"

export type Plan = TrajectorySnapshotDto["plans"][number]
type Entry = Plan["entries"][number]

function isDone(entry: Entry): boolean {
  return entry.status === "completed"
}

function hasEntries(plan: Plan): boolean {
  return !plan.removed && plan.entries.length > 0
}

/** The plan of the dock: the newest one with an open entry. */
export function openPlan(plans: readonly Plan[]): Plan | undefined {
  return plans
    .filter((plan) => hasEntries(plan) && !plan.entries.every(isDone))
    .sort((a, b) => (a.updated_at < b.updated_at ? -1 : a.updated_at > b.updated_at ? 1 : 0))
    .at(-1)
}

/** The finished plans, by the turn that last changed each. */
export function finishedPlans(plans: readonly Plan[]): Map<string, Plan[]> {
  const byTurn = new Map<string, Plan[]>()
  for (const plan of plans) {
    if (!hasEntries(plan) || !plan.entries.every(isDone) || !plan.turn_id) continue
    byTurn.set(plan.turn_id, [...(byTurn.get(plan.turn_id) ?? []), plan])
  }
  return byTurn
}

/** The fold id of a plan's checklist, beside the turn ids in the console's folds. */
export function planFoldId(plan: Plan): string {
  return `plan:${plan.plan_id}`
}

/** The entry the agent works on now: the first in progress, else the first not done. */
function currentEntry(plan: Plan): Entry | undefined {
  return (
    plan.entries.find((entry) => entry.status === "in_progress") ??
    plan.entries.find((entry) => !isDone(entry))
  )
}

const MARKS: Record<string, { mark: string; tone: string }> = {
  completed: { mark: "✓", tone: "text-status-done-fg" },
  in_progress: { mark: "●", tone: "text-status-active-fg" },
}

/** One plan as a line, `Plan 2/5 · <current entry>`, that opens to its checklist. */
export function PlanLine({
  plan,
  open,
  onToggle,
}: {
  plan: Plan
  open: boolean
  onToggle: () => void
}) {
  const done = plan.entries.filter(isDone).length
  const current = currentEntry(plan)
  const Chevron = open ? ChevronDownIcon : ChevronRightIcon
  return (
    <section aria-label="Plan" data-plan-id={plan.plan_id} className="font-mono text-xs">
      <button
        type="button"
        aria-expanded={open}
        onClick={onToggle}
        className="flex w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-muted-foreground outline-none hover:bg-muted/60 hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <Chevron aria-hidden className="size-3.5 shrink-0" />
        <ListChecksIcon aria-hidden className="size-3.5 shrink-0" />
        <span className="shrink-0">
          Plan {done}/{plan.entries.length}
        </span>
        {current ? (
          <>
            <span aria-hidden>·</span>
            <span className="min-w-0 truncate text-foreground">{current.content}</span>
          </>
        ) : (
          <>
            <span aria-hidden>·</span>
            <span>done</span>
          </>
        )}
      </button>
      {open ? (
        <ul aria-label="Plan entries" className="ml-6 space-y-0.5 py-1">
          {plan.entries.map((entry, index) => {
            const look = MARKS[entry.status] ?? { mark: "○", tone: "text-muted-foreground" }
            return (
              // Entries have no id of their own: a plan is replaced whole.
              // biome-ignore lint/suspicious/noArrayIndexKey: the order is the plan's
              <li key={index} data-status={entry.status} className="flex min-w-0 gap-2">
                <span aria-hidden className={cn("shrink-0", look.tone)}>
                  {look.mark}
                </span>
                <span className={cn("min-w-0 break-words", isDone(entry) && "line-through")}>
                  {entry.content}
                </span>
              </li>
            )
          })}
        </ul>
      ) : null}
    </section>
  )
}
