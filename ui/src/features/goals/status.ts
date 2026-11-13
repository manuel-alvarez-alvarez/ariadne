/**
 * The goal status vocabulary in one place: the order the filter lists them in,
 * how each one is spelled on screen, and which of them a goal cannot leave.
 *
 * `GOAL_STATUS_META` is a total record over the generated enum, so a new goal
 * status in the daemon fails to compile here until it is named and coloured.
 */

import type { GoalStatus } from "@/api"

/** The goal statuses, in lifecycle order — also the order of the filter. */
export const GOAL_STATUSES: readonly GoalStatus[] = [
  "planning",
  "active",
  "completed",
  "cancelled",
] as const

interface GoalStatusMeta {
  label: string
  /**
   * Badge classes, from the status ramp in `index.css`: colour carries the
   * meaning here, so it is spelled out per status rather than mapped onto the
   * badge variants, which only cover neutral/primary/destructive. The ramp
   * carries dark mode, so the tint is never a light one left on a dark screen.
   *
   * The steps are the goal's counterparts of the task ones: planning is the
   * orchestrator's violet, an active goal is the accent, completed is done.
   */
  badge: string
  /** Solid dot classes, from the same ramp. */
  dot: string
}

export const GOAL_STATUS_META: Record<GoalStatus, GoalStatusMeta> = {
  planning: {
    label: "Planning",
    badge: "bg-status-review-soft text-status-review-fg",
    dot: "bg-status-review",
  },
  active: {
    label: "Active",
    badge: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
  },
  completed: {
    label: "Completed",
    badge: "bg-status-done-soft text-status-done-fg",
    dot: "bg-status-done",
  },
  cancelled: {
    label: "Cancelled",
    badge: "bg-muted text-muted-foreground",
    dot: "bg-muted-foreground/60",
  },
}

/** Statuses a goal cannot leave: no actions apply to them. */
export function isTerminalGoalStatus(status: GoalStatus): boolean {
  return status === "completed" || status === "cancelled"
}
