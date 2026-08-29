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
export const BOARD_STATUSES = [
  "pending",
  "in_progress",
  "under_review",
  "approved",
  "merged",
] as const satisfies readonly TaskStatus[]

/**
 * Statuses that leave the pipeline; shown apart from the board.
 *
 * A failure is *not* one of them any more: it is a retry candidate, so it
 * belongs where the retry would put it — the Pending column, outlined in
 * danger (see {@link StatusMeta.border}). Only a cancelled task is genuinely
 * off the pipeline: nobody is coming back to it.
 */
export const OFF_BOARD_STATUSES = ["cancelled"] as const satisfies readonly TaskStatus[]

/**
 * The daemon statuses the UI folds into a primary one: `ready` is a phase of
 * `pending` and `changes_requested` a phase of `in_progress`. The raw status
 * stays visible as a sub-status badge.
 *
 * `ready` is folded because the daemon spawns the engineer in the same
 * reconcile pass that made the task ready — nothing queues there, so a column
 * of its own was empty by design. A task that *does* linger in it (paused
 * goal, daemon down, engineer spawn failing, a failure just retried) is
 * exactly what the badge on the Pending card says.
 *
 * `approved` is not folded: it is the whole of the landing stage now, and a
 * task sits in it for as long as its engineer takes to squash the change onto
 * the base branch — or for as long as a published request waits on a human.
 */
const SUB_STATUS_OF = {
  ready: "pending",
  changes_requested: "in_progress",
} as const satisfies Partial<Record<TaskStatus, TaskStatus>>

/** The column a status belongs to: itself, unless it is a sub-status. */
export function primaryStatus(status: TaskStatus): TaskStatus {
  return (SUB_STATUS_OF as Partial<Record<TaskStatus, TaskStatus>>)[status] ?? status
}

/** The refining meta ("Ready", …) when `status` is a sub-status, else undefined. */
export function subStatus(status: TaskStatus): StatusMeta | undefined {
  return status in SUB_STATUS_OF ? TASK_STATUS_META[status] : undefined
}

/** "Pending · Ready" for a sub-status, the plain primary label otherwise. */
export function displayLabel(status: TaskStatus): string {
  const sub = subStatus(status)
  const primary = TASK_STATUS_META[primaryStatus(status)].label
  return sub ? `${primary} · ${sub.label}` : primary
}

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
 * Which step of the ramp each status takes. The pipeline reads left to right —
 * pending grey, in progress accent, review violet, approved teal-green,
 * merged green — with the folded sub-statuses on the step of the column they
 * sit in (`ready` teal, one shade off its grey column, because a task parked
 * there is not simply waiting). Approved takes the step between the review and
 * the merge it is on its way to, which is what the last active stage of the
 * pipeline should look like. The two statuses that mean "something is wrong"
 * (changes requested, failed) are the only warm ones, so a stalled task is
 * never mistaken for a waiting one.
 */
export const TASK_STATUS_META: Record<TaskStatus, StatusMeta> = {
  pending: {
    label: "Pending",
    hint: "Not started yet: waiting for its dependencies to merge, or ready and waiting for an engineer session.",
    badge: "bg-status-pending-soft text-status-pending-fg",
    dot: "bg-status-pending",
  },
  ready: {
    label: "Ready",
    hint: "Dependencies merged, and still waiting for an engineer session — the daemon normally starts one at once, so a task sitting here means its goal is paused, the daemon is down, the spawn is failing, or it was just retried.",
    badge: "bg-status-ready-soft text-status-ready-fg",
    dot: "bg-status-ready",
  },
  in_progress: {
    label: "In progress",
    hint: "An engineer session is working on the task: implementing, or applying review feedback.",
    badge: "bg-status-active-soft text-status-active-fg",
    dot: "bg-status-active",
  },
  under_review: {
    label: "Under review",
    hint: "Review requested: reviewer sessions are reading the branch and voting on it.",
    badge: "bg-status-review-soft text-status-review-fg",
    dot: "bg-status-review",
  },
  changes_requested: {
    label: "Changes requested",
    hint: "A reviewer asked for changes this round.",
    badge: "bg-status-warn-soft text-status-warn-fg",
    dot: "bg-status-warn",
  },
  approved: {
    label: "Approved",
    hint: "Enough approvals collected; its engineer is landing it — squashing it onto the base branch, or waiting on the pull request it published to be merged.",
    badge: "bg-status-approved-soft text-status-approved-fg",
    dot: "bg-status-approved",
  },
  merged: {
    label: "Merged",
    hint: "Merge verified on the base branch.",
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
 * A failure is waiting for a decision, so it leads. Approved comes next
 * because it is the one stage whose next step can be a *person's*: an
 * engineer that published the change as a pull request has done all it can
 * itself, and the task sits there until somebody merges it. Then what the
 * agents are still working on, then what has not started, then what is done
 * with.
 */
const ATTENTION_RANK = {
  failed: 0,
  approved: 1,
  changes_requested: 2,
  under_review: 3,
  in_progress: 4,
  ready: 5,
  pending: 6,
  merged: 7,
  cancelled: 8,
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
 * that decision is still a task somebody may say something to.
 */
export function isTerminalTaskStatus(status: TaskStatus): boolean {
  return status === "merged" || status === "cancelled"
}

/** The user may cancel anything that has not reached a terminal status. */
export function canCancel(status: TaskStatus): boolean {
  return !isTerminalTaskStatus(status)
}

/** Retry is the one transition the user actor owns besides cancel: failed -> ready. */
export function canRetry(status: TaskStatus): boolean {
  return status === "failed"
}

/** Editing (title, description, reviewers, dependencies) is pre-start only. */
export function canEdit(status: TaskStatus): boolean {
  return status === "pending" || status === "ready"
}
