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
  "plan_ready",
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
   * planner's violet, an active goal is the accent, completed is done. A plan
   * waiting to be approved takes the warm step: it is the one status of the
   * five that is waiting on the *user*, and warm is how the board says so
   * everywhere else.
   */
  badge: string
}

export const GOAL_STATUS_META: Record<GoalStatus, GoalStatusMeta> = {
  planning: {
    label: "Planning",
    badge: "bg-status-review-soft text-status-review-fg",
  },
  plan_ready: {
    label: "Plan ready",
    badge: "bg-status-warn-soft text-status-warn-fg",
  },
  active: {
    label: "Active",
    badge: "bg-status-active-soft text-status-active-fg",
  },
  completed: {
    label: "Completed",
    badge: "bg-status-done-soft text-status-done-fg",
  },
  cancelled: {
    label: "Cancelled",
    badge: "bg-muted text-muted-foreground",
  },
}

/**
 * Whether the goal's plan is still the user's to approve — its tasks are held
 * back until it is, whether they are `pending` or `ready`, and the board keeps
 * them all in its first column to say so.
 */
export function awaitsPlanApproval(status: GoalStatus): boolean {
  return status === "planning" || status === "plan_ready"
}

/** Statuses a goal cannot leave: no actions apply to them. */
export function isTerminalGoalStatus(status: GoalStatus): boolean {
  return status === "completed" || status === "cancelled"
}
