/**
 * What is stuck, in one query, for the strip above the board.
 *
 * The orchestrator's first question — "which tasks are stalled or failed, which
 * agents died or are waiting on me?" — used to need every goal opened one by
 * one. It is three list queries, all of them shared keys the SSE dispatcher
 * already invalidates, so the answer stays live without anything here
 * subscribing to events.
 *
 * All three are read unfiltered on purpose: the board's status filter narrows
 * the lanes, never this — a stuck task the filter hides is exactly the one the
 * strip exists to keep in sight.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { type SessionAttention, sessionAttention } from "@/features/sessions/session-display"
import { taskListQueryOptions } from "@/features/tasks"

import { goalsQueryOptions } from "./queries"

/** Why a task is on the list, strongest first. */
export type AttentionReason = "failed" | "changes_requested" | "stalled"

/**
 * Whether this task wants the user, and what for.
 *
 * `stalled` is checked last because it is a flag *on top of* a status: the
 * daemon sets it when an agent went idle without advancing the task and clears
 * it on the next transition, so a task that also failed is reported as failed.
 */
export function taskAttentionReason(task: TaskDto): AttentionReason | null {
  if (task.status === "failed") return "failed"
  if (task.status === "changes_requested") return "changes_requested"
  return task.stalled ? "stalled" : null
}

/** What every row carries, whichever kind it is. */
interface AttentionRow {
  /** The row's own id, which is also what its panel link opens. */
  id: string
  goalId: string
  /** The goal itself, when the goals list has it — the row names it either way. */
  goal: GoalDto | undefined
  /** When this row last moved; the list is ordered by it, newest first. */
  at: string
}

export interface AttentionTaskItem extends AttentionRow {
  kind: "task"
  task: TaskDto
  reason: AttentionReason
}

export interface AttentionSessionItem extends AttentionRow {
  kind: "session"
  session: SessionDto
  /** Why the session is here — its `attention_reason`, or `failed`. */
  reason: SessionAttention
}

export type AttentionItem = AttentionTaskItem | AttentionSessionItem

export interface Attention {
  /** Tasks and sessions in one list, most recently updated first. */
  items: AttentionItem[]
  isPending: boolean
  /** The first of the three queries that failed, if any. */
  error: unknown
  /**
   * Something failed, but the queries that answered still produced items.
   *
   * The three lists are independent — a failed `GET /v1/tasks` says nothing
   * about the sessions — so the usual case for a list this size is one that
   * rendered with a hole in it rather than one that failed. Reporting both as
   * the same "could not load" is what made a readable list look empty.
   */
  partial: boolean
  refetch: () => void
}

export function useAttention(): Attention {
  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(taskListQueryOptions())
  // Unfiltered, and narrowed by `sessionAttention` below rather than by the
  // daemon: the rule is "flagged for attention *or* dead", and `GET
  // /v1/sessions` ANDs `attention` with `status`, so no single request spells
  // it. Filtering here is also what keeps the rule in one place for both this
  // strip and `ariadne attention`. The key is the one the sessions screen
  // already holds, so the extra rows usually cost no extra request.
  const sessions = useQuery(sessionsQueryOptions())

  const items = useMemo(
    () => collectAttention(goals.data, tasks.data, sessions.data),
    [goals.data, tasks.data, sessions.data],
  )

  const error = goals.error ?? tasks.error ?? sessions.error

  return {
    items,
    isPending: goals.isPending || tasks.isPending || sessions.isPending,
    error,
    partial: error !== null && items.length > 0,
    refetch: () => {
      void goals.refetch()
      void tasks.refetch()
      void sessions.refetch()
    },
  }
}

/**
 * The two kinds interleaved into one list, newest first.
 *
 * Which sessions belong is {@link sessionAttention}'s call, not this list's,
 * so the strip and `ariadne attention` include and label the same ones.
 *
 * A goal the goals list did not carry does not drop its rows: a task or a
 * session can outlive its goal falling out of a filtered list, and hiding it
 * would hide exactly the row the strip exists for — the row names the goal by
 * its short id instead.
 */
export function collectAttention(
  goals: GoalDto[] | undefined,
  tasks: TaskDto[] | undefined,
  sessions: SessionDto[] | undefined,
): AttentionItem[] {
  const goalsById = new Map((goals ?? []).map((goal) => [goal.id, goal]))
  const items: AttentionItem[] = []

  for (const task of tasks ?? []) {
    const reason = taskAttentionReason(task)
    if (!reason) continue
    items.push({
      kind: "task",
      id: task.id,
      goalId: task.goal_id,
      goal: goalsById.get(task.goal_id),
      at: task.updated_at,
      task,
      reason,
    })
  }

  for (const session of sessions ?? []) {
    const reason = sessionAttention(session)
    if (!reason) continue
    items.push({
      kind: "session",
      id: session.id,
      goalId: session.goal_id,
      goal: goalsById.get(session.goal_id),
      // When the reason was raised is what the row is about, so it comes
      // first: a session that has been waiting on a permission prompt for an
      // hour says so, rather than reporting when it started. A failed
      // session's last move is its death; `created_at` is only the fallback
      // for one the daemon has not stamped an end on yet.
      at: session.attention_since ?? session.ended_at ?? session.created_at,
      session,
      reason,
    })
  }

  return items.sort((a, b) => b.at.localeCompare(a.at))
}
