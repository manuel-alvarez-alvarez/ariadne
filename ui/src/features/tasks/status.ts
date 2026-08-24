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
  "integrating",
  "merged",
] as const satisfies readonly TaskStatus[]

/** Statuses that leave the pipeline; shown apart from the board. */
export const OFF_BOARD_STATUSES = ["cancelled", "failed"] as const satisfies readonly TaskStatus[]

/**
 * The daemon statuses the UI folds into a primary one: `ready` is a phase of
 * `pending`, `changes_requested` is a phase of `in_progress`, `approved` is a
 * phase of `integrating`. The raw status stays visible as a sub-status badge.
 *
 * `ready` is folded because the daemon spawns the engineer in the same
 * reconcile pass that made the task ready — nothing queues there, so a column
 * of its own was empty by design. A task that *does* linger in it (paused
 * goal, daemon down, engineer spawn failing, a failure just retried) is
 * exactly what the badge on the Pending card says.
 *
 * `approved` is folded for the same reason, and forwards rather than back: the
 * approvals are in, the reviewers are done, and the same reconcile pass hands
 * the task to its integrator — so it belongs to the landing, not to the review
 * it just left. A task parked there is one whose integrator never started, and
 * the badge on the Integrating card is what says so.
 */
const SUB_STATUS_OF = {
  ready: "pending",
  changes_requested: "in_progress",
  approved: "integrating",
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
}

/**
 * Which step of the ramp each status takes. The pipeline reads left to right —
 * pending grey, in progress accent, review violet, integrating teal-green,
 * merged green — with the folded sub-statuses on the step of the column they
 * sit in (`ready` teal, one shade off its grey column, because a task parked
 * there is not simply waiting). Integrating takes the step between the review
 * and the merge it is on its way to, which is what the last active stage of
 * the pipeline should look like. The two statuses that mean "something is
 * wrong" (changes requested, failed) are the only warm ones, so a stalled task
 * is never mistaken for a waiting one.
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
    hint: "Enough approvals collected; the integrator is about to take the task over.",
    badge: "bg-status-integrating-soft text-status-integrating-fg",
    dot: "bg-status-integrating",
  },
  integrating: {
    label: "Integrating",
    hint: "An integrator session has the task: rebasing it onto its base branch and landing it, or waiting on the pull request it opened to be merged.",
    badge: "bg-status-integrating-soft text-status-integrating-fg",
    dot: "bg-status-integrating",
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
  },
}

/**
 * How loudly each status asks for a person, lowest first.
 *
 * A failure is waiting for a decision, so it leads. Integrating comes next
 * because it is the one stage whose next step can be a *person's*: an
 * integrator that published the change as a pull request has done all it can,
 * and the task sits there until somebody merges it. `approved` follows it,
 * being the moment before the same thing. Then what the agents are still
 * working on, then what has not started, then what is done with.
 */
const ATTENTION_RANK = {
  failed: 0,
  integrating: 1,
  approved: 2,
  changes_requested: 3,
  under_review: 4,
  in_progress: 5,
  ready: 6,
  pending: 7,
  merged: 8,
  cancelled: 9,
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

/** Editing (title, description, reviewers, dependencies) is pre-start only. */
export function canEdit(status: TaskStatus): boolean {
  return status === "pending" || status === "ready"
}
