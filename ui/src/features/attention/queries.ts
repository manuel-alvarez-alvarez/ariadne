/**
 * What is stuck, in one query.
 *
 * The orchestrator's first question — "which tasks are stalled or failed, which
 * agents died?" — used to need every goal opened one by one. It is three list
 * queries, all of them shared keys the SSE dispatcher already invalidates, so
 * the answer stays live without anything here subscribing to events.
 *
 * The hook is used twice: by the attention screen and by the count on its
 * sidebar entry. Both read the same cache entries, so the second costs nothing.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { goalsQueryOptions } from "@/features/goals/queries"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"

/** Why a task is on the list, strongest first. */
export type AttentionReason = "failed" | "changes_requested" | "stalled"

export const ATTENTION_REASON_LABELS: Record<AttentionReason, string> = {
  failed: "Failed",
  changes_requested: "Changes requested",
  stalled: "Stalled",
}

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

export interface AttentionTask {
  task: TaskDto
  reason: AttentionReason
}

/** Everything one goal has that needs attention. Never empty. */
export interface AttentionGroup {
  goalId: string
  /** The goal itself, when the goals list has it. */
  goal: GoalDto | undefined
  tasks: AttentionTask[]
  /** Sessions of this goal that failed, its planner's included. */
  sessions: SessionDto[]
}

export interface Attention {
  groups: AttentionGroup[]
  /** Rows in total, for the sidebar badge. */
  count: number
  isPending: boolean
  /** The first of the three queries that failed, if any. */
  error: unknown
  refetch: () => void
}

export function useAttention(): Attention {
  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(taskListQueryOptions())
  const sessions = useQuery(sessionsQueryOptions({ status: "failed" }))

  const groups = useMemo(
    () => group(goals.data, tasks.data, sessions.data),
    [goals.data, tasks.data, sessions.data],
  )

  return {
    groups,
    count: groups.reduce((total, g) => total + g.tasks.length + g.sessions.length, 0),
    isPending: goals.isPending || tasks.isPending || sessions.isPending,
    error: goals.error ?? tasks.error ?? sessions.error,
    refetch: () => {
      void goals.refetch()
      void tasks.refetch()
      void sessions.refetch()
    },
  }
}

/**
 * Goals first, in the order the goals list gives them (newest first), then any
 * goal the list did not carry — a task or session can outlive its goal falling
 * out of a filtered list, and dropping it would hide exactly the row the screen
 * exists for.
 */
function group(
  goals: GoalDto[] | undefined,
  tasks: TaskDto[] | undefined,
  sessions: SessionDto[] | undefined,
): AttentionGroup[] {
  const byGoal = new Map<string, AttentionGroup>()
  const goalsById = new Map((goals ?? []).map((goal) => [goal.id, goal]))

  function groupFor(goalId: string): AttentionGroup {
    const existing = byGoal.get(goalId)
    if (existing) return existing
    const created: AttentionGroup = {
      goalId,
      goal: goalsById.get(goalId),
      tasks: [],
      sessions: [],
    }
    byGoal.set(goalId, created)
    return created
  }

  for (const task of tasks ?? []) {
    const reason = taskAttentionReason(task)
    if (reason) groupFor(task.goal_id).tasks.push({ task, reason })
  }
  for (const session of sessions ?? []) {
    groupFor(session.goal_id).sessions.push(session)
  }

  const order = new Map((goals ?? []).map((goal, index) => [goal.id, index]))
  return [...byGoal.values()].sort(
    (a, b) =>
      (order.get(a.goalId) ?? Number.MAX_SAFE_INTEGER) -
      (order.get(b.goalId) ?? Number.MAX_SAFE_INTEGER),
  )
}
