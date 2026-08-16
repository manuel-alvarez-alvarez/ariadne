/**
 * The board of a goal's tasks, grouped by status.
 *
 * Mounted by the goal detail screen. The columns are the pipeline a task walks
 * through, always all of them and always in that order, so a task keeps its
 * place on screen as it moves and an empty column reads as "nothing is here"
 * rather than disappearing. Cancelled and failed tasks have left the pipeline
 * and are shown apart from it.
 *
 * Nothing here polls: `task_created` and `task_updated` invalidate the task
 * lists in the event dispatcher, so the board follows the daemon by itself.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowUpRightIcon } from "lucide-react"
import { useMemo } from "react"
import { Link } from "react-router-dom"

import type { TaskDto, TaskStatus } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { describeError } from "./format"
import { tasksPath } from "./paths"
import { taskListQueryOptions } from "./queries"
import { BOARD_STATUSES, OFF_BOARD_STATUSES, TASK_STATUS_META } from "./status"
import { TaskCard } from "./task-card"

export function TaskBoard({ goalId, className }: { goalId: string; className?: string }) {
  const tasks = useQuery(taskListQueryOptions({ goal: goalId }))
  const columns = useMemo(() => groupByStatus(tasks.data ?? []), [tasks.data])

  return (
    <section className={cn("space-y-3", className)}>
      <header className="flex items-center gap-3">
        <h2 className="font-heading text-sm font-semibold">Tasks</h2>
        {tasks.data && (
          <span className="text-xs text-muted-foreground">
            {tasks.data.length} total
            {tasks.isFetching && " · refreshing"}
          </span>
        )}
        <Link
          to={tasksPath({ goal: goalId })}
          className="ml-auto flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
        >
          Open as a list
          <ArrowUpRightIcon className="size-3" />
        </Link>
      </header>

      {tasks.error ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load the tasks</AlertTitle>
          <AlertDescription>{describeError(tasks.error)}</AlertDescription>
        </Alert>
      ) : tasks.isPending ? (
        <BoardSkeleton />
      ) : tasks.data.length === 0 ? (
        <p className="rounded-lg border border-dashed px-4 py-8 text-center text-sm text-muted-foreground">
          The planner has not created any task for this goal yet.
        </p>
      ) : (
        <>
          <div className="-mx-1 flex gap-3 overflow-x-auto px-1 pb-2">
            {BOARD_STATUSES.map((status) => (
              <BoardColumn key={status} status={status} tasks={columns[status]} />
            ))}
          </div>
          <OffBoard columns={columns} />
        </>
      )}
    </section>
  )
}

function BoardColumn({ status, tasks }: { status: TaskStatus; tasks: TaskDto[] }) {
  const meta = TASK_STATUS_META[status]
  return (
    <div className="flex w-60 shrink-0 flex-col gap-2">
      <div className="flex items-center gap-2 px-0.5" title={meta.hint}>
        <span className={cn("size-1.5 rounded-full", meta.dot)} />
        <h3 className="text-xs font-medium">{meta.label}</h3>
        <span className="text-xs text-muted-foreground">{tasks.length}</span>
      </div>
      {tasks.length === 0 ? (
        <div className="rounded-lg border border-dashed px-2 py-4 text-center text-xs text-muted-foreground/70">
          empty
        </div>
      ) : (
        tasks.map((task) => <TaskCard key={task.id} task={task} />)
      )}
    </div>
  )
}

/** Cancelled and failed tasks: off the pipeline, but not out of sight. */
function OffBoard({ columns }: { columns: Record<TaskStatus, TaskDto[]> }) {
  const present = OFF_BOARD_STATUSES.filter((status) => columns[status].length > 0)
  if (present.length === 0) return null

  return (
    <div className="space-y-2 rounded-lg border border-dashed bg-muted/20 p-3">
      <h3 className="text-xs font-medium text-muted-foreground">Off the pipeline</h3>
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {present.flatMap((status) =>
          columns[status].map((task) => <TaskCard key={task.id} task={task} showStatus />),
        )}
      </div>
    </div>
  )
}

function BoardSkeleton() {
  return (
    <div className="flex gap-3 overflow-hidden">
      {BOARD_STATUSES.slice(0, 5).map((status) => (
        <div key={status} className="w-60 shrink-0 space-y-2">
          <Skeleton className="h-4 w-24" />
          <Skeleton className="h-16 w-full" />
        </div>
      ))}
    </div>
  )
}

/** Every status gets a bucket, so the columns never have to guard for absence. */
function groupByStatus(tasks: TaskDto[]): Record<TaskStatus, TaskDto[]> {
  const columns = Object.fromEntries(
    [...BOARD_STATUSES, ...OFF_BOARD_STATUSES].map((status) => [status, [] as TaskDto[]]),
  ) as Record<TaskStatus, TaskDto[]>
  for (const task of tasks) columns[task.status]?.push(task)
  return columns
}
