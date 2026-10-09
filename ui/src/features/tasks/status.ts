/**
 * The task status vocabulary in one place: the order the board lays statuses
 * out in, how each one is labelled and coloured, and what the *user* actor is
 * allowed to do from it.
 *
 * `canCancel` / `canRetry` mirror the transition table in
 * `crates/ariadne-core/src/state_machine.rs` for `Actor::User`: cancel from any
 * non-terminal status, retry only from `failed`. `canEdit` mirrors the store's
 * `update_task` / `set_task_dependencies` guards the same way. Offering a
 * button the daemon answers with `illegal_transition` (or a `409`) is worse
 * than not offering it at all.
 */

import type { TaskDto, TaskStatus } from "@/api"

/** Pipeline columns, in the order a task moves through them. */

interface StatusMeta {
  label: string
  /** What the status means, for tooltips and empty columns. */
  hint: string
  /** Badge classes, from the status ramp in `index.css`: it carries dark mode. */
  badge: string
  /** Solid dot classes, for the board column headers. */
  dot: string
  /**
   * Card border, for a status a card should be outlined by — the way
   * `STALLED_META.border` outlines a stalled task. Only a failure has one: it
   * sits in the Pending column beside tasks that have simply not started, and
   * the outline is what tells the two apart at a glance.
   */
  border?: string
}

/**
 * The workflow owns columns, while status says whether the task has started,
 * completed, stopped, or failed.
 */
export const TASK_STATUS_META: Record<TaskStatus, StatusMeta> = {
  pending: {
    label: "Pending",
    hint: "Waiting for its dependencies or workflow.",
    badge: "bg-status-pending-soft text-status-pending-fg",
    dot: "bg-status-pending",
  },
  ready: {
    label: "Ready",
    hint: "Ready for its workflow to start.",
    badge: "bg-status-ready-soft text-status-ready-fg",
    dot: "bg-status-ready",
  },
  in_progress: {
    label: "In progress",
    hint: "A workflow column is running.",
    badge: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
  },
  finished: {
    label: "Finished",
    hint: "The workflow completed.",
    badge: "bg-status-done-soft text-status-done-fg",
    dot: "bg-status-done",
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
    badge: "bg-status-danger-soft text-status-danger-fg",
    dot: "bg-status-danger",
    border: "border-status-danger/40",
  },
}

/**
 * How loudly each status asks for a person, lowest first.
 *
 * A failure is waiting for a decision, then active workflow work, then tasks
 * that have not started, and finally completed work.
 */
const ATTENTION_RANK = {
  failed: 0,
  in_progress: 1,
  ready: 2,
  pending: 3,
  finished: 4,
  cancelled: 5,
} as const satisfies Record<TaskStatus, number>

/**
 * Orders a list of tasks by how much they want to be looked at: what needs the
 * user first, most recently touched first within the same status.
 */
export function compareByAttention(a: TaskDto, b: TaskDto): number {
  // A stalled agent is the one thing that will not resolve itself, whatever
  // status the task is parked in.
  if (a.stalled !== b.stalled) return a.stalled ? -1 : 1
  const rank = ATTENTION_RANK[a.status] - ATTENTION_RANK[b.status]
  return rank !== 0 ? rank : Date.parse(b.updated_at) - Date.parse(a.updated_at)
}

/**
 * Terminal statuses are frozen: nothing, and nobody, moves a task out of them.
 *
 * `failed` is not one of them — the user can retry it, and a task waiting for
 * that decision is still one an agent may be started on again.
 */
function isTerminalTaskStatus(status: TaskStatus): boolean {
  return status === "finished" || status === "cancelled"
}

/** The user may cancel anything that has not reached a terminal status. */
export function canCancel(status: TaskStatus): boolean {
  return !isTerminalTaskStatus(status)
}

/** Retry is the one transition the user actor owns besides cancel: failed -> ready. */
export function canRetry(status: TaskStatus): boolean {
  return status === "failed"
}

/** Editing is available before workflow work begins. */
export function canEdit(status: TaskStatus): boolean {
  return status === "pending" || status === "ready"
}
