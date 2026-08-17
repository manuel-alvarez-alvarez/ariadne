/**
 * The goals list as a board: one column per pipeline stage, one horizontal
 * swimlane per goal, every task of every shown goal in its cell.
 *
 * The board scrolls in both directions inside its own box, which is what makes
 * the two headers stick: the column row to the top, each goal's name to the
 * left edge. With ten goals on screen neither axis loses its labels.
 *
 * The lanes share a single task-list query (`GET /v1/tasks`), which the SSE
 * dispatcher invalidates on `task_created`/`task_updated`, so cards move
 * between cells without polling — same as the per-goal board.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, ChevronRightIcon } from "lucide-react"
import { useMemo } from "react"
import { Link } from "react-router-dom"

import type { GoalDto, TaskDto, TaskStatus } from "@/api"
import { ErrorState } from "@/components/error-state"
import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import {
  BOARD_STATUSES,
  OFF_BOARD_STATUSES,
  primaryStatus,
  TASK_STATUS_META,
  TaskCard,
  taskListQueryOptions,
} from "@/features/tasks"
import { plural } from "@/lib/plural"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"
import { useCollapsedLanes } from "./collapsed-lanes"
import { GOAL_STATUS_META } from "./status"

/** One template for the header row and every lane, so the columns line up. */
const COLUMNS_GRID = "grid grid-cols-[repeat(5,minmax(13rem,1fr))] gap-3"

/** The board's own scrollport: sticky only works against the box that scrolls. */
const BOARD_BOX = "min-h-0 flex-1 rounded-lg border"

/** Opaque, because the lanes scroll underneath it. */
const HEADER_ROW = "sticky top-0 z-20 border-b bg-muted px-3 py-2"

/**
 * Left-pinned and opaque for the same reason, one layer below the column row
 * so the two cross cleanly. `w-fit` is what lets it slide: a full-width block
 * has nowhere to stick to.
 */
const LANE_HEADER = "sticky left-0 z-10 flex w-fit max-w-full items-center gap-2 bg-background px-3"

export function GoalSwimlanes({ goals }: { goals: GoalDto[] }) {
  const tasks = useQuery(taskListQueryOptions({}))
  const byGoal = useMemo(() => groupByGoal(tasks.data ?? []), [tasks.data])
  const { collapsed, toggle } = useCollapsedLanes()

  if (tasks.error) {
    return (
      <ErrorState
        title="Could not load the tasks"
        error={tasks.error}
        onRetry={() => void tasks.refetch()}
      />
    )
  }

  if (tasks.isPending) {
    return <BoardSkeleton />
  }

  return (
    <div className={cn(BOARD_BOX, "overflow-auto")}>
      <div className="min-w-[72rem]">
        <div className={cn(COLUMNS_GRID, HEADER_ROW)}>
          {BOARD_STATUSES.map((status) => {
            const meta = TASK_STATUS_META[status]
            const count = goals.reduce(
              (sum, goal) => sum + (byGoal.get(goal.id)?.columns[status].length ?? 0),
              0,
            )
            return (
              <div key={status} className="flex items-center gap-2">
                <span className={cn("size-1.5 rounded-full", meta.dot)} />
                <h2 className="text-xs font-medium">
                  <Tooltip>
                    <TooltipTrigger render={<span />}>{meta.label}</TooltipTrigger>
                    <TooltipContent>{meta.hint}</TooltipContent>
                  </Tooltip>
                </h2>
                <span className="text-xs text-muted-foreground">{count}</span>
              </div>
            )
          })}
        </div>
        {goals.map((goal) => (
          <Lane
            key={goal.id}
            goal={goal}
            tasks={byGoal.get(goal.id)}
            collapsed={collapsed.has(goal.id)}
            onToggle={() => toggle(goal.id)}
          />
        ))}
      </div>
    </div>
  )
}

function Lane({
  goal,
  tasks,
  collapsed,
  onToggle,
}: {
  goal: GoalDto
  tasks?: GoalTasks
  collapsed: boolean
  onToggle: () => void
}) {
  const total = tasks?.all.length ?? 0
  const repos = goal.repos.map((repo) => `${repo.path} [${repo.base_branch}]`).join("\n")

  return (
    <section className="border-b last:border-b-0">
      <header className={cn(LANE_HEADER, collapsed ? "py-1.5" : "pt-2.5 pb-1")}>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-expanded={!collapsed}
          aria-label={`${collapsed ? "Expand" : "Collapse"} ${goal.title}`}
          onClick={onToggle}
        >
          {collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
        </Button>
        <Tooltip>
          <TooltipTrigger
            render={
              <Link
                to={paths.goal(goal.id)}
                className="min-w-0 truncate text-sm font-medium underline-offset-4 hover:underline"
              />
            }
          >
            {goal.title}
          </TooltipTrigger>
          <TooltipContent className="whitespace-pre-line">
            {repos || "Open the goal"}
          </TooltipContent>
        </Tooltip>
        <StatusBadge
          box="badge"
          label={GOAL_STATUS_META[goal.status].label}
          tone={GOAL_STATUS_META[goal.status].badge}
        />
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          {plural(total, "task")} · created{" "}
          <time dateTime={goal.created_at} title={formatAbsolute(goal.created_at)}>
            {formatRelative(goal.created_at)}
          </time>
        </span>
      </header>

      {collapsed ? null : total === 0 ? (
        <p className="sticky left-0 w-fit px-3 pt-1 pb-3 text-xs text-muted-foreground">
          {goal.status === "planning"
            ? "No tasks yet — the planner is still working."
            : "No tasks."}
        </p>
      ) : (
        <div className={cn(COLUMNS_GRID, "px-3 pt-1 pb-2.5")}>
          {BOARD_STATUSES.map((status) => (
            // An empty cell is empty: whitespace already says the column has
            // nothing in it, and a board of placeholders says nothing at all.
            <div key={status} className="flex flex-col gap-2">
              {(tasks?.columns[status] ?? []).map((task) => (
                <TaskCard key={task.id} task={task} />
              ))}
            </div>
          ))}
        </div>
      )}

      {!collapsed && tasks && tasks.offBoard.length > 0 && (
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

/**
 * The loading board, shaped like the board it becomes: the column row and a
 * few lanes on the same grid. Shown by the page while the goals load and by
 * the board while the tasks do, so a cold start is one skeleton, not two
 * unrelated ones in sequence.
 */
export function BoardSkeleton() {
  return (
    <div className={cn(BOARD_BOX, "overflow-hidden")} aria-hidden>
      <div className="min-w-[72rem]">
        <div className={cn(COLUMNS_GRID, "border-b bg-muted px-3 py-2")}>
          {BOARD_STATUSES.map((status) => (
            // Tinted against the header's own `bg-muted`, which a plain
            // skeleton would disappear into.
            <Skeleton key={status} className="h-4 w-24 bg-muted-foreground/20" />
          ))}
        </div>
        {[0, 1, 2].map((lane) => (
          <div key={lane} className="border-b px-3 pt-2.5 pb-2.5 last:border-b-0">
            <Skeleton className="h-4 w-48" />
            <div className={cn(COLUMNS_GRID, "pt-3")}>
              {BOARD_STATUSES.map((status, column) => (
                <div key={status}>
                  {(lane + column) % 2 === 0 ? <Skeleton className="h-14 w-full" /> : null}
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
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
