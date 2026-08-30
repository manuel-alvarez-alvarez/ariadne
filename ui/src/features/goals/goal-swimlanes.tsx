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
import { ScrollEdge } from "@/components/scroll-edge"
import { StatusBadge } from "@/components/status-badge"
import { goalUsageRows, TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import { type SessionAttention, SessionAttentionBadge } from "@/features/sessions/session-display"
import {
  BOARD_STATUSES,
  OFF_BOARD_STATUSES,
  primaryStatus,
  TASK_STATUS_META,
  TaskCard,
  taskListQueryOptions,
} from "@/features/tasks"
import { useHorizontalOverflow } from "@/hooks/use-scroll-overflow"
import { cn, folderName } from "@/lib/format"
import { paths } from "@/routes/paths"
import { type BoardAttention, taskAttentionReason, useBoardAttention } from "./attention"
import { useCollapsedLanes } from "./collapsed-lanes"
import { laneSummary, orderLanes } from "./lanes"
import { GOAL_STATUS_META, isStillPlanning, isTerminalGoalStatus } from "./status"

/**
 * One template for the header row and every lane, so the columns line up: one
 * per pipeline stage, and none of them narrower than a card is readable at.
 *
 * Two floors, because a 1280px laptop is the machine this is used on: 13rem is
 * what a card wants, 11rem is what it still reads at, and below `xl` the
 * second one is what keeps the fifth column on screen instead of past the
 * right edge.
 */
const COLUMNS_GRID =
  "grid grid-cols-[repeat(5,minmax(11rem,1fr))] gap-3 xl:grid-cols-[repeat(5,minmax(13rem,1fr))]"

/**
 * What the lanes are laid out at before the board gives up and scrolls: the
 * grid's own floor (five columns and four 0.75rem gaps) plus the padding
 * either side of a lane, rounded up — 60rem at the narrow floor, 72rem at the
 * wide one. It sits on the block *inside* the scrollport, which is what makes
 * a narrow window scroll the board rather than squeeze its columns past
 * reading.
 */
const BOARD_WIDTH = "min-w-[60rem] xl:min-w-[72rem]"

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

  if (tasks.error) {
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
    <div className={BOARD_FRAME}>
      {/* A named region the keyboard can put focus into: the board scrolls both
          ways, and a scrollport nothing can focus only scrolls under a pointer
          — the columns and lanes past its edges would be mouse-only. */}
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
          {lanes.map((goal) => {
            // A finished goal opens folded: five columns of cards that all sit
            // in Merged is a 270px box that is 85% empty, and on a 900px
            // screen the cards are scrolled off it entirely — so it reads as
            // an empty lane. Its header says what happened instead. The user's
            // own answer, either way, wins over this.
            const collapsed = isCollapsed(goal.id, isTerminalGoalStatus(goal.status))
            return (
              <Lane
                key={goal.id}
                goal={goal}
                tasks={byGoal.get(goal.id)}
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
  )
}

function Lane({
  goal,
  tasks,
  attention,
  collapsed,
  onToggle,
}: {
  goal: GoalDto
  tasks?: GoalTasks
  /** The whole board's index; the lane takes its own rows out of it. */
  attention: BoardAttention
  collapsed: boolean
  onToggle: () => void
}) {
  const total = tasks?.all.length ?? 0
  const repos = goal.repos.map((repo) => `${repo.path} [${repo.base_branch}]`).join("\n")
  // Folder name, not the full path: the header is a line among five other
  // cells, and the path in full is what the tooltip on the title already
  // gives — this is only the part that tells lanes apart at a glance.
  const repoSummary = goal.repos
    .map((repo) => `${folderName(repo.path)} [${repo.base_branch}]`)
    .join(", ")
  // A planner belongs to no task, so it has no card to be flagged on: the lane
  // header is the only place its goal is named, and so the only place it can
  // ask for a person. It is shown collapsed too — a lane folded away is
  // exactly where a stuck planner would otherwise go unseen.
  const planner: SessionAttention | undefined = attention.byGoal.get(goal.id)

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
          {/* The title first, because the lane header truncates it and this is
              the only place the rest of it is readable; the repositories are
              what the lane is *about*, so they follow it. */}
          <TooltipContent className="flex-col items-start gap-0.5">
            <span className="font-medium">{goal.title}</span>
            <span className="whitespace-pre-line text-background/70">
              {repos || "Open the goal"}
            </span>
          </TooltipContent>
        </Tooltip>
        {repoSummary && (
          <span className="min-w-0 truncate text-xs text-muted-foreground">{repoSummary}</span>
        )}
        <StatusBadge
          box="badge"
          label={GOAL_STATUS_META[goal.status].label}
          tone={GOAL_STATUS_META[goal.status].badge}
        />
        {planner ? <SessionAttentionBadge attention={planner} /> : null}
        <span className="flex items-baseline gap-1 whitespace-nowrap text-xs text-muted-foreground">
          {/* Where the lane is up to, and the whole of what a folded lane
              says: how far through the pipeline it is, and what is stuck or
              waiting in it. "N tasks" was the one number about a goal that
              stops changing the moment the planner is done. */}
          {laneSummary(tasks?.all ?? [])} · created <When at={goal.created_at} label="created" /> ·{" "}
          {/* What the whole goal has cost — planner, engineers and reviewers —
              which is the one number the board can show without opening
              anything. The hint behind it names the halves and splits the
              total between the three roles. */}
          <TokenFigure usage={goal.usage.total} rows={goalUsageRows(goal.usage)} />
        </span>
      </header>

      {collapsed ? null : total === 0 ? (
        <p className="sticky left-0 w-fit px-3 pt-1 pb-3 text-xs text-muted-foreground">
          {goal.status === "planning" ? "No tasks yet — the planner is still working" : "No tasks"}
        </p>
      ) : (
        <div className={cn(COLUMNS_GRID, "px-3 pt-1 pb-2.5")}>
          {BOARD_STATUSES.map((status) => (
            // An empty cell is empty: whitespace already says the column has
            // nothing in it, and a board of placeholders says nothing at all.
            <div key={status} className="flex flex-col gap-2">
              {(tasks?.columns[status] ?? []).map((task) => (
                <TaskCard
                  key={task.id}
                  task={task}
                  // A failure in the Pending column has to say so: the outline
                  // is what catches the eye, the badge is what names it.
                  showStatus={task.status === "failed"}
                  attention={attention.byTask.get(task.id)}
                />
              ))}
            </div>
          ))}
        </div>
      )}

      {/* Only the cancelled are down here now. A failed task is a retry
          candidate, so it stays in the Pending column where the retry would
          put it back; a cancelled one is nobody's next move. */}
      {!collapsed && tasks && tasks.offBoard.length > 0 && (
        <div className="mx-3 mb-2.5 space-y-2 rounded-lg border border-dashed bg-muted/20 p-2">
          <h3 className="text-xs font-medium text-muted-foreground">Cancelled</h3>
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {tasks.offBoard.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                showStatus
                attention={attention.byTask.get(task.id)}
              />
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
    <div className={BOARD_FRAME} aria-hidden>
      <div className={cn(BOARD_BOX, "overflow-hidden")}>
        <div className={BOARD_WIDTH}>
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
    </div>
  )
}

interface GoalTasks {
  all: TaskDto[]
  /** Board cells, keyed by primary status. */
  columns: Record<TaskStatus, TaskDto[]>
  /** Cancelled tasks: off the pipeline, but not out of sight. */
  offBoard: TaskDto[]
}

/**
 * Whether this lane is asking for a person, which is what puts it above every
 * other lane on the board.
 *
 * Three ways it can: its planner is blocked (which has no card to show it on),
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

/**
 * The tasks of each lane, in the cell each one belongs in.
 *
 * A goal still being planned holds all of its tasks in the first column,
 * `pending` and `ready` alike: nothing under it has been handed to an
 * engineer, so a card further down the pipeline would say a task is moving
 * when it is waiting on the planner. That is the goal's status talking, which
 * is why the goals are an argument here.
 *
 * A failed task lands in that first column too, whatever the goal is doing:
 * the one thing anybody does with a failure is retry it, and a retry puts it
 * back exactly there. Its card is outlined in danger and badged `Failed`, so
 * the column holds "not started" and "started and did not survive it" without
 * the two being mistaken for each other.
 */
function groupByGoal(tasks: TaskDto[], goals: GoalDto[]): Map<string, GoalTasks> {
  const lanes = new Map<string, GoalTasks>()
  const held = new Set(goals.filter((goal) => isStillPlanning(goal.status)).map((goal) => goal.id))
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
    } else if (task.status === "failed" || held.has(task.goal_id)) {
      lane.columns[BOARD_STATUSES[0]].push(task)
    } else {
      lane.columns[primaryStatus(task.status)]?.push(task)
    }
  }
  return lanes
}
