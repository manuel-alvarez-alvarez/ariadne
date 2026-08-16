import type { GoalStatus } from "@/api"
import { Badge } from "@/components/ui/badge"
import { cn } from "@/lib/utils"

/** The four goal statuses, in lifecycle order — also the order of the filter. */
export const GOAL_STATUSES: readonly GoalStatus[] = [
  "planning",
  "active",
  "completed",
  "cancelled",
] as const

/** Statuses a goal cannot leave: no actions apply to them. */
export function isTerminalGoalStatus(status: GoalStatus): boolean {
  return status === "completed" || status === "cancelled"
}

/**
 * Colour carries the meaning here, so it is spelled out per status rather than
 * mapped onto the badge variants, which only cover neutral/primary/destructive.
 */
const STATUS_CLASSES: Record<GoalStatus, string> = {
  planning: "bg-amber-500/15 text-amber-700 dark:text-amber-400",
  active: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
  completed: "bg-sky-500/15 text-sky-700 dark:text-sky-400",
  cancelled: "bg-muted text-muted-foreground",
}

export function GoalStatusBadge({ status, className }: { status: GoalStatus; className?: string }) {
  return (
    <Badge variant="secondary" className={cn(STATUS_CLASSES[status], className)}>
      {status}
    </Badge>
  )
}
