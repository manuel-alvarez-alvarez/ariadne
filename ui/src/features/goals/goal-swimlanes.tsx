/**
 * The goals list as a board: one column per pipeline stage, one horizontal
 * swimlane per goal, every task of every shown goal in its cell.
 *
 * The lanes share a single task-list query (`GET /v1/tasks`), which the SSE
 * dispatcher invalidates on `task_created`/`task_updated`, so cards move
 * between cells without polling — same as the per-goal board.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { Link } from "react-router-dom"

import { ApiError, type GoalDto, type TaskDto, type TaskStatus } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Skeleton } from "@/components/ui/skeleton"
import {
  BOARD_STATUSES,
  OFF_BOARD_STATUSES,
  primaryStatus,
  TASK_STATUS_META,
  TaskCard,
  taskListQueryOptions,
} from "@/features/tasks"
import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"
import { formatRelative } from "./format"
import { GoalStatusBadge } from "./goal-status-badge"

/** One template for the header row and every lane, so the columns line up. */
const COLUMNS_GRID = "grid grid-cols-[repeat(5,minmax(13rem,1fr))] gap-3"

export function GoalSwimlanes({ goals }: { goals: GoalDto[] }) {
  const tasks = useQuery(taskListQueryOptions({}))
  const byGoal = useMemo(() => groupByGoal(tasks.data ?? []), [tasks.data])

  if (tasks.error) {
    return (
      <Alert variant="destructive">
        <AlertTitle>Could not load the tasks</AlertTitle>
        <AlertDescription>
          {ApiError.is(tasks.error) ? tasks.error.message : String(tasks.error)}
        </AlertDescription>
      </Alert>
    )
  }

  if (tasks.isPending) {
    return (
      <div className="flex flex-col gap-2 rounded-lg border p-4">
        {[0, 1, 2].map((row) => (
          <Skeleton key={row} className="h-16 w-full" />
        ))}
      </div>
    )
  }

  return (
    <div className="overflow-x-auto rounded-lg border">
      <div className="min-w-[72rem]">
        <div className={cn(COLUMNS_GRID, "border-b bg-muted/30 px-3 py-2")}>
          {BOARD_STATUSES.map((status) => {
            const meta = TASK_STATUS_META[status]
            const count = goals.reduce(
              (sum, goal) => sum + (byGoal.get(goal.id)?.columns[status].length ?? 0),
              0,
            )
            return (
              <div key={status} className="flex items-center gap-2" title={meta.hint}>
                <span className={cn("size-1.5 rounded-full", meta.dot)} />
                <h2 className="text-xs font-medium">{meta.label}</h2>
                <span className="text-xs text-muted-foreground">{count}</span>
              </div>
            )
          })}
        </div>
        {goals.map((goal) => (
          <Lane key={goal.id} goal={goal} tasks={byGoal.get(goal.id)} />
        ))}
      </div>
    </div>
  )
}

function Lane({ goal, tasks }: { goal: GoalDto; tasks?: GoalTasks }) {
  const total = tasks?.all.length ?? 0
  return (
    <section className="border-b last:border-b-0">
      <header className="flex flex-wrap items-center gap-2 px-3 pt-2.5">
        <Link
          to={paths.goal(goal.id)}
          className="text-sm font-medium underline-offset-4 hover:underline"
          title={goal.repos.map((repo) => `${repo.path} [${repo.base_branch}]`).join("\n")}
        >
          {goal.title}
        </Link>
        <GoalStatusBadge status={goal.status} />
        <span className="text-xs text-muted-foreground">
          {total} {total === 1 ? "task" : "tasks"}
        </span>
        <span className="ml-auto text-xs text-muted-foreground">
          created {formatRelative(goal.created_at)}
        </span>
      </header>

      {total === 0 ? (
        <p className="px-3 py-3 text-xs text-muted-foreground">
          {goal.status === "planning"
            ? "No tasks yet — the planner is still working."
            : "No tasks."}
        </p>
      ) : (
        <div className={cn(COLUMNS_GRID, "px-3 py-2.5")}>
          {BOARD_STATUSES.map((status) => {
            const cell = tasks?.columns[status] ?? []
            return (
              <div key={status} className="flex flex-col gap-2">
                {cell.length === 0 ? (
                  <div className="min-h-10 rounded-lg border border-dashed border-border/60" />
                ) : (
                  cell.map((task) => <TaskCard key={task.id} task={task} />)
                )}
              </div>
            )
          })}
        </div>
      )}

      {tasks && tasks.offBoard.length > 0 && (
        <div className="mx-3 mb-2.5 space-y-2 rounded-lg border border-dashed bg-muted/20 p-2">
          <h3 className="text-xs font-medium text-muted-foreground">Off the pipeline</h3>
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {tasks.offBoard.map((task) => (
              <TaskCard key={task.id} task={task} showStatus />
            ))}
          </div>
        </div>
      )}
    </section>
  )
}

interface GoalTasks {
  all: TaskDto[]
  /** Board cells, keyed by primary status. */
  columns: Record<TaskStatus, TaskDto[]>
  /** Cancelled and failed tasks: off the pipeline, but not out of sight. */
  offBoard: TaskDto[]
}

function groupByGoal(tasks: TaskDto[]): Map<string, GoalTasks> {
  const lanes = new Map<string, GoalTasks>()
  for (const task of tasks) {
    let lane = lanes.get(task.goal_id)
    if (!lane) {
      lane = {
        all: [],
        columns: Object.fromEntries(
          BOARD_STATUSES.map((status) => [status, [] as TaskDto[]]),
        ) as Record<TaskStatus, TaskDto[]>,
        offBoard: [],
      }
      lanes.set(task.goal_id, lane)
    }
    lane.all.push(task)
    if ((OFF_BOARD_STATUSES as readonly TaskStatus[]).includes(task.status)) {
      lane.offBoard.push(task)
    } else {
      lane.columns[primaryStatus(task.status)]?.push(task)
    }
  }
  return lanes
}
