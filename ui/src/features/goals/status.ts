/**
 * The goal status vocabulary in one place: the order the filter lists them in,
 * how each one is spelled on screen, and which of them a goal cannot leave.
 *
 * `GOAL_STATUS_META` is a total record over the generated enum, so a new goal
 * status in the daemon fails to compile here until it is named and coloured.
 */

import type { GoalStatus } from "@/api"

/** The four goal statuses, in lifecycle order — also the order of the filter. */
export const GOAL_STATUSES: readonly GoalStatus[] = [
  "planning",
  "active",
  "completed",
  "cancelled",
] as const

interface GoalStatusMeta {
  label: string
  /**
   * Badge classes; colour carries the meaning here, so it is spelled out per
   * status rather than mapped onto the badge variants, which only cover
   * neutral/primary/destructive.
   */
  badge: string
}

export const GOAL_STATUS_META: Record<GoalStatus, GoalStatusMeta> = {
  planning: {
    label: "Planning",
    badge: "bg-amber-500/15 text-amber-700 dark:text-amber-400",
  },
  active: {
    label: "Active",
    badge: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
  },
  completed: {
    label: "Completed",
    badge: "bg-sky-500/15 text-sky-700 dark:text-sky-400",
  },
  cancelled: {
    label: "Cancelled",
    badge: "bg-muted text-muted-foreground",
  },
}

/** Statuses a goal cannot leave: no actions apply to them. */
export function isTerminalGoalStatus(status: GoalStatus): boolean {
  return status === "completed" || status === "cancelled"
}
