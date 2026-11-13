/**
 * The goals screen as a compact list: one row per goal, with its status, how
 * many of its tasks are done, how many things want a person, and the
 * repositories it works in. A row is a link to the goal's panel; tasks are
 * not expanded here.
 *
 * It reads the same task and session queries as the board, so it stays live
 * through the same events, and a failed refresh keeps the cached rows under
 * the same warning.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { GoalDto, TaskDto } from "@/api"
import { ErrorState, RefreshWarning } from "@/components/error-state"
import { taskListQueryOptions } from "@/features/tasks"
import { type BoardAttention, taskAttentionReason, useBoardAttention } from "./attention"
import { BoardSkeleton } from "./goal-swimlanes"
import { laneCounts, orderLanes } from "./lanes"
import { GOAL_STATUS_META } from "./status"

/** What in a goal wants a person: its orchestrator, and each task that is stuck. */
function attentionCount(goal: GoalDto, tasks: readonly TaskDto[], attention: BoardAttention) {
  const stuck = tasks.filter(
    (task) => attention.byTask.has(task.id) || taskAttentionReason(task) !== null,
  ).length
  return stuck + (attention.byGoal.has(goal.id) ? 1 : 0)
}

/** The current params with this goal's panel open, so the status filter survives. */
function goalPanelSearch(current: URLSearchParams, goalId: string): string {
  const next = new URLSearchParams(current)
  next.set("goal", goalId)
  return `?${next}`
}

export function GoalList({ goals }: { goals: GoalDto[] }) {
  const tasks = useQuery(taskListQueryOptions({}))
  const [search] = useSearchParams()
  const attention = useBoardAttention()
  const byGoal = useMemo(() => {
    const map = new Map<string, TaskDto[]>()
    for (const task of tasks.data ?? []) {
      const list = map.get(task.goal_id)
      if (list) list.push(task)
      else map.set(task.goal_id, [task])
    }
    return map
  }, [tasks.data])
  const rows = useMemo(
    () =>
      orderLanes(goals, (goal) => attentionCount(goal, byGoal.get(goal.id) ?? [], attention) > 0),
    [goals, byGoal, attention],
  )

  const stale = tasks.error && tasks.data !== undefined
  if (tasks.error && !stale) {
    return (
      <ErrorState
        title="Could not load tasks"
        error={tasks.error}
        onRetry={() => void tasks.refetch()}
      />
    )
  }
  if (tasks.isPending) return <BoardSkeleton />

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      {stale ? (
        <RefreshWarning
          what="tasks"
          error={tasks.error}
          onRetry={() => void tasks.refetch()}
          className="shrink-0"
        />
      ) : null}
      <ul
        aria-label="Goals list"
        className="min-h-0 flex-1 divide-y overflow-auto rounded-lg border"
      >
        {rows.map((goal) => {
          const goalTasks = byGoal.get(goal.id) ?? []
          const counts = laneCounts(goalTasks)
          const status = GOAL_STATUS_META[goal.status]
          const needs = attentionCount(goal, byGoal.get(goal.id) ?? [], attention)
          return (
            <li key={goal.id}>
              <Link
                to={{ search: goalPanelSearch(search, goal.id) }}
                className="flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/50 focus-visible:bg-muted/50 focus-visible:outline-none"
              >
                <span className="min-w-0 flex-1 truncate font-medium">{goal.title}</span>
                <span className="flex items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground">
                  <span className={`size-1.5 rounded-full ${status.dot}`} aria-hidden />
                  {status.label}
                </span>
                <span className="whitespace-nowrap text-xs text-muted-foreground">
                  {counts.finished}/{goalTasks.length} tasks
                </span>
                <span
                  className={`whitespace-nowrap text-xs ${needs > 0 ? "text-destructive" : "text-muted-foreground"}`}
                >
                  {needs > 0 ? `${needs} needs attention` : "No attention"}
                </span>
                <span className="max-w-[30%] truncate text-xs text-muted-foreground">
                  {goal.repos.map((repo) => repo.path.split("/").pop() || repo.path).join(", ") ||
                    "No repositories"}
                </span>
              </Link>
            </li>
          )
        })}
      </ul>
    </div>
  )
}
