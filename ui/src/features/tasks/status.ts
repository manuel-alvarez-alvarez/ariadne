/**
 * The task status vocabulary in one place: the order the board lays statuses
 * out in, how each one is labelled and coloured, and what the *user* actor is
 * allowed to do from it.
 *
 * `canCancel` / `canRetry` mirror the transition table in
 * `crates/ariadne-core/src/state_machine.rs` for `Actor::User`: cancel from any
 * non-terminal status, retry only from `failed`. Offering a button the daemon
 * answers with `illegal_transition` is worse than not offering it at all.
 */

import type { TaskStatus } from "@/api"

/** Pipeline columns, in the order a task moves through them. */
export const BOARD_STATUSES = [
  "pending",
  "ready",
  "in_progress",
  "under_review",
  "merged",
] as const satisfies readonly TaskStatus[]

/** Statuses that leave the pipeline; shown apart from the board. */
export const OFF_BOARD_STATUSES = ["cancelled", "failed"] as const satisfies readonly TaskStatus[]

/**
 * The daemon statuses the UI folds into a primary one: `changes_requested`
 * and `merging` are phases of `in_progress`, `approved` is a phase of
 * `under_review`. The raw status stays visible as a sub-status badge.
 */
const SUB_STATUS_OF = {
  changes_requested: "in_progress",
  merging: "in_progress",
  approved: "under_review",
} as const satisfies Partial<Record<TaskStatus, TaskStatus>>

/** The column a status belongs to: itself, unless it is a sub-status. */
export function primaryStatus(status: TaskStatus): TaskStatus {
  return (SUB_STATUS_OF as Partial<Record<TaskStatus, TaskStatus>>)[status] ?? status
}

/** The refining meta ("Merging", …) when `status` is a sub-status, else undefined. */
export function subStatus(status: TaskStatus): StatusMeta | undefined {
  return status in SUB_STATUS_OF ? TASK_STATUS_META[status] : undefined
}

/** "In progress · Merging" for a sub-status, the plain primary label otherwise. */
export function displayLabel(status: TaskStatus): string {
  const sub = subStatus(status)
  const primary = TASK_STATUS_META[primaryStatus(status)].label
  return sub ? `${primary} · ${sub.label}` : primary
}

interface StatusMeta {
  label: string
  /** What the status means, for tooltips and empty columns. */
  hint: string
  /** Badge classes; tinted so light and dark both keep the text readable. */
  badge: string
  /** Solid dot classes, for the board column headers. */
  dot: string
}

export const TASK_STATUS_META: Record<TaskStatus, StatusMeta> = {
  pending: {
    label: "Pending",
    hint: "Waiting for its dependencies to merge.",
    badge: "bg-zinc-500/12 text-zinc-700 dark:bg-zinc-400/15 dark:text-zinc-300",
    dot: "bg-zinc-400 dark:bg-zinc-500",
  },
  ready: {
    label: "Ready",
    hint: "Dependencies merged; waiting for an engineer session.",
    badge: "bg-amber-500/12 text-amber-700 dark:bg-amber-400/15 dark:text-amber-300",
    dot: "bg-amber-500",
  },
  in_progress: {
    label: "In progress",
    hint: "An engineer session is working on the task: implementing, applying review feedback, or merging.",
    badge: "bg-blue-500/12 text-blue-700 dark:bg-blue-400/15 dark:text-blue-300",
    dot: "bg-blue-500",
  },
  under_review: {
    label: "Under review",
    hint: "Review requested: reviewer sessions are active, or the task is approved and waiting to merge.",
    badge: "bg-violet-500/12 text-violet-700 dark:bg-violet-400/15 dark:text-violet-300",
    dot: "bg-violet-500",
  },
  changes_requested: {
    label: "Changes requested",
    hint: "A reviewer asked for changes this round.",
    badge: "bg-orange-500/12 text-orange-700 dark:bg-orange-400/15 dark:text-orange-300",
    dot: "bg-orange-500",
  },
  approved: {
    label: "Approved",
    hint: "Enough approvals collected; waiting to be told to merge.",
    badge: "bg-teal-500/12 text-teal-700 dark:bg-teal-400/15 dark:text-teal-300",
    dot: "bg-teal-500",
  },
  merging: {
    label: "Merging",
    hint: "The engineer is merging into the base branch.",
    badge: "bg-cyan-500/12 text-cyan-700 dark:bg-cyan-400/15 dark:text-cyan-300",
    dot: "bg-cyan-500",
  },
  merged: {
    label: "Merged",
    hint: "Merge verified on the base branch.",
    badge: "bg-emerald-500/12 text-emerald-700 dark:bg-emerald-400/15 dark:text-emerald-300",
    dot: "bg-emerald-500",
  },
  cancelled: {
    label: "Cancelled",
    hint: "Cancelled by the user.",
    badge: "bg-muted text-muted-foreground",
    dot: "bg-muted-foreground/60",
  },
  failed: {
    label: "Failed",
    hint: "Unrecoverable failure; the user can retry it.",
    badge: "bg-destructive/12 text-destructive dark:bg-destructive/20",
    dot: "bg-destructive",
  },
}

export function statusLabel(status: TaskStatus): string {
  return TASK_STATUS_META[primaryStatus(status)].label
}

/** Terminal statuses are frozen: nothing, and nobody, moves a task out of them. */
export function isTerminal(status: TaskStatus): boolean {
  return status === "merged" || status === "cancelled"
}

/** The user may cancel anything that has not reached a terminal status. */
export function canCancel(status: TaskStatus): boolean {
  return !isTerminal(status)
}

/** Retry is the one transition the user actor owns besides cancel: failed -> ready. */
export function canRetry(status: TaskStatus): boolean {
  return status === "failed"
}
