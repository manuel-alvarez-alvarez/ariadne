/** Goal lanes and their task cards. */

import { useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, ChevronRightIcon } from "lucide-react"
import { useMemo } from "react"
import { Link } from "react-router-dom"

import type { GoalDto, TaskDto } from "@/api"
import { ErrorState, RefreshWarning } from "@/components/error-state"
import { ScrollEdge } from "@/components/scroll-edge"
import { goalUsageRows, TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { type SessionAttention, SessionAttentionBadge } from "@/features/sessions/session-display"
import { TaskCard, taskListQueryOptions } from "@/features/tasks"
import { stepTitle } from "@/features/tasks/steps"
import { useHorizontalOverflow } from "@/hooks/use-scroll-overflow"
import { cn, formatAbsolute } from "@/lib/format"
import { paths } from "@/routes/paths"
import { type BoardAttention, taskAttentionReason, useBoardAttention } from "./attention"
import { useCollapsedLanes } from "./collapsed-lanes"
import { laneCounts, laneSummary, orderLanes } from "./lanes"
import { GOAL_STATUS_META, isTerminalGoalStatus } from "./status"

const BOARD_WIDTH = "min-w-[20rem]"

/** The grid a cell lays its cards out on: as many 18rem-floor columns as fit. */
const CELL_GRID =
  "grid grid-cols-[repeat(auto-fill,minmax(min(18rem,100%),1fr))] content-start gap-2"

/**
 * The board's own scrollport: sticky only works against the box that scrolls.
 * It fills {@link BOARD_FRAME}, which is what the edge fades are positioned
 * against — they have to sit outside the thing that scrolls under them.
 *
 * `contain-paint` is what keeps that scrolling to *this* box. Without it the
 * lanes hidden past the bottom edge still counted as overflow for the shell's
 * `<main>`, which scrolls the whole screen: measured in the Tauri window,
 * `main.scrollHeight` came back 4418px against a 712px port — with the board
 * itself correctly 249px tall — so the page scrolled behind a board that was
 * already scrolling, two vertical scrollbars, and the page header and the
 * attention strip slid away under the first one. Chromium counts it the same
 * way (4470 against 657), so this is not a WebKit workaround: it is the box
 * saying what it already means, that it draws nothing outside itself, which is
 * what stops the overflow reaching an ancestor scroller.
 *
 * It costs the board nothing: paint containment does not size anything, so
 * every height here is what it was, sticky still sticks to this scrollport,
 * and the focus ring below is this box's own rather than a descendant's — the
 * one thing containment would have clipped.
 */
const BOARD_BOX = "h-full rounded-lg border contain-paint"

/**
 * The board's slot on the screen: the height it gets, and the fades' anchor.
 *
 * The height is flex's, from the fixed-height column in `goals-list-page.tsx`,
 * and it lands the same in both engines — measured in the Tauri window, this
 * frame is what is left of `<main>` to the pixel. What differed there is only
 * what escapes the scrollport inside it; see {@link BOARD_BOX}.
 */
const BOARD_FRAME = "relative min-h-0 flex-1"

export function GoalSwimlanes({
  goals,
  activeOnly = false,
}: {
  goals: GoalDto[]
  activeOnly?: boolean
}) {
  const tasks = useQuery(taskListQueryOptions({}))
  const byGoal = useMemo(() => groupByGoal(tasks.data ?? [], goals), [tasks.data, goals])
  // Which cards — and which lanes — are asking for a person, off the sessions
  // query the attention strip above the board already holds. The board is
  // where the work is, and a card that says nothing about its blocked agent is
  // what made the strip the only way to find one.
  const attention = useBoardAttention()
  const { isCollapsed, setCollapsed } = useCollapsedLanes()
  const board = useHorizontalOverflow<HTMLElement>()
  // Active work first: what is asking for a person, then what is still
  // running, then what is done with. An old active goal used to sit below
  // every goal finished after it, which is the wrong way round on a board
  // that is open to answer "what now".
  const lanes = useMemo(
    () => orderLanes(goals, (goal) => laneNeedsAttention(goal.id, byGoal.get(goal.id), attention)),
    [goals, byGoal, attention],
  )
  const visible = (task: TaskDto) =>
    !activeOnly || (task.status !== "finished" && task.status !== "cancelled")
  const total = goals.reduce(
    (count, goal) => count + (byGoal.get(goal.id)?.all.filter(visible).length ?? 0),
    0,
  )

  const staleTasks = tasks.error && tasks.data !== undefined
  if (tasks.error && !staleTasks) {
    return (
      <ErrorState
        title="Could not load tasks"
        error={tasks.error}
        onRetry={() => void tasks.refetch()}
      />
    )
  }

  if (tasks.isPending) {
    return <BoardSkeleton />
  }

  return (
    <>
      {/* Outside the frame: the board fills the frame's whole height, so a
          warning inside it would push the board past the frame's bottom edge. */}
      {staleTasks ? (
        <RefreshWarning
          what="tasks"
          error={tasks.error}
          onRetry={() => void tasks.refetch()}
          className="shrink-0"
        />
      ) : null}
      <div className={BOARD_FRAME}>
        {/* A named region the keyboard can put focus into: the board scrolls both
          ways, and a scrollport nothing can focus only scrolls under a pointer
          — the lanes past its edges would be mouse-only. */}
        <section
          ref={board.ref}
          aria-label="Goals board"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className={cn(
            BOARD_BOX,
            "overflow-auto focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
          )}
        >
          <div className={BOARD_WIDTH}>
            <ColumnHeaders total={total} />
            {lanes.map((goal) => {
              // Finished goals open folded unless the user chose otherwise.
              const collapsed = isCollapsed(goal.id, isTerminalGoalStatus(goal.status))
              return (
                <Lane
                  key={goal.id}
                  goal={goal}
                  tasks={byGoal.get(goal.id)}
                  visible={visible}
                  attention={attention}
                  collapsed={collapsed}
                  onToggle={() => setCollapsed(goal.id, !collapsed)}
                />
              )
            })}
          </div>
        </section>
        <ScrollEdge side="start" show={board.overflow.start} />
        <ScrollEdge side="end" show={board.overflow.end} />
      </div>
    </>
  )
}

function ColumnHeaders({ total }: { total: number }) {
  return (
    <div className="sticky top-0 z-20 mx-2 border-x border-background bg-background py-2">
      <h2 className="px-3 text-sm font-semibold">Tasks ({total})</h2>
    </div>
  )
}

function Lane({
  goal,
  tasks,
  attention,
  visible,
  collapsed,
  onToggle,
}: {
  goal: GoalDto
  tasks?: GoalTasks
  /** The whole board's index; the lane takes its own rows out of it. */
  attention: BoardAttention
  visible: (task: TaskDto) => boolean
  collapsed: boolean
  onToggle: () => void
}) {
  const counts = laneCounts(tasks?.all ?? [])
  const summary = laneSummary(tasks?.all ?? [])
  const repos = goal.repos.map((repo) => `${repo.path} [${repo.base_branch}]`).join("\n")
  const status = GOAL_STATUS_META[goal.status]
  // An orchestrator belongs to no task, so it has no card to be flagged on: the lane
  // header is the only place its goal is named, and so the only place it can
  // ask for a person. It is shown collapsed too — a lane folded away is
  // exactly where a stuck orchestrator would otherwise go unseen.
  const orchestrator: SessionAttention | undefined = attention.byGoal.get(goal.id)
  const empty = (tasks?.all.length ?? 0) === 0

  return (
    <section className="mx-2 mb-2 rounded-sm border border-dotted border-border/70 bg-background">
      <header className={cn("flex items-center gap-2 px-3", collapsed ? "py-1.5" : "pt-2 pb-1")}>
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
          {/* The title first, because the lane header truncates it and this is
              the only place the rest of it is readable; the repositories are
              what the lane is *about*, so they follow it. */}
          <TooltipContent className="flex-col items-start gap-0.5">
            <span className="font-medium">{goal.title}</span>
            <span className="whitespace-pre-line text-background/70">
              {repos || "Open the goal"}
            </span>
            <span className="text-background/70">created {formatAbsolute(goal.created_at)}</span>
            <span className="text-background/70">
              {counts.finished}/{counts.pipeline} done · {counts.failed} failed · {counts.stalled}{" "}
              stalled
            </span>
          </TooltipContent>
        </Tooltip>
        {/* A dot, not a badge: the lane is one line. Its text is for a screen
            reader, which would otherwise hear nothing of the goal's status. */}
        <span className={cn("size-1.5 shrink-0 rounded-full", status.dot)} aria-hidden />
        <span className="sr-only">{status.label}</span>
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          {counts.finished}/{counts.pipeline}
        </span>
        {orchestrator ? <SessionAttentionBadge attention={orchestrator} /> : null}
        {collapsed ? (
          <span className="min-w-0 truncate whitespace-nowrap text-xs text-muted-foreground">
            {summary}
          </span>
        ) : null}
        {/* What the whole goal has cost — the orchestrator and every
            column's agents — which is the one number the board can show
            without opening anything. The hint behind it names the halves
            and splits the total by who spent it. */}
        <TokenFigure
          className="ml-auto whitespace-nowrap text-xs text-muted-foreground"
          usage={goal.usage.total}
          rows={goalUsageRows(goal.usage, (step) => stepTitle(goal.steps, step))}
        />
      </header>

      {collapsed ? null : (
        // biome-ignore lint/a11y/useSemanticElements: this group is not a form control
        <div
          className={cn(CELL_GRID, "px-3 pt-1 pb-2.5")}
          role="group"
          aria-label={`${goal.title}: Tasks`}
        >
          {empty ? (
            <p className="col-span-full text-xs text-muted-foreground">
              {goal.status === "planning"
                ? "No tasks yet — the orchestrator is still working"
                : "No tasks"}
            </p>
          ) : null}
          {(tasks?.all ?? []).filter(visible).map((task) => (
            <TaskCard
              key={task.id}
              task={task}
              steps={goal.steps}
              attention={attention.byTask.get(task.id)}
              lastActivityAt={attention.lastActivity.get(task.id)}
            />
          ))}
        </div>
      )}
    </section>
  )
}

/** The loading board follows the shape of its lanes. */
export function BoardSkeleton() {
  return (
    <div className={BOARD_FRAME} aria-hidden>
      <div className={cn(BOARD_BOX, "overflow-hidden")}>
        <div className={BOARD_WIDTH}>
          <div className="border-b py-2">
            <Skeleton className="mx-3 h-3 w-20" />
          </div>
          {[0, 1, 2].map((lane) => (
            <div key={lane} className="border-b py-2.5 last:border-b-0">
              <Skeleton className="mx-3 h-4 w-48" />
              <Skeleton className="mx-3 mt-2 h-14 w-48" />
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

interface GoalTasks {
  all: TaskDto[]
}

/**
 * Whether this lane is asking for a person, which is what puts it above every
 * other lane on the board.
 *
 * Three ways it can: its orchestrator is blocked (which has no card to show it on),
 * one of its agents is (which does), or one of its tasks failed or stalled —
 * the same rule the attention strip lists a task by, read here so a lane and
 * the strip above it never disagree about what is stuck.
 */
function laneNeedsAttention(
  goalId: string,
  tasks: GoalTasks | undefined,
  attention: BoardAttention,
): boolean {
  if (attention.byGoal.has(goalId)) return true
  return (tasks?.all ?? []).some(
    (task) => attention.byTask.has(task.id) || taskAttentionReason(task) !== null,
  )
}

/** Group the listed goals’ tasks for their lanes. */
function groupByGoal(tasks: TaskDto[], goals: GoalDto[]): Map<string, GoalTasks> {
  const lanes = new Map<string, GoalTasks>()
  const goalsById = new Map(goals.map((goal) => [goal.id, goal]))
  for (const task of tasks) {
    // Only the goals the board shows get a lane.
    const goal = goalsById.get(task.goal_id)
    if (!goal) continue
    let lane = lanes.get(task.goal_id)
    if (!lane) {
      lane = { all: [] }
      lanes.set(task.goal_id, lane)
    }
    lane.all.push(task)
  }
  return lanes
}
