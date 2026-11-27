/**
 * The goals board: one list of task rows, the rows of one goal adjacent under
 * the goal's own line, and a gap between one group and the next. Each row
 * has a rail segment at its left edge, and the segments of one goal join into
 * one rail. A task with no goal is a group of one row.
 *
 * A goal's facts are in its line, shown at rest, with no background, border
 * or frame. A folded goal is the line with its summary, and a goal with no
 * shown task the line and one muted line that says why.
 *
 * The board is one grid and every line a subgrid of it (see `task-row.tsx`),
 * so the goal's token total sits in the token column over its rows' figures.
 * From the 33.9rem floor up the board never scrolls sideways: a narrow board hides
 * whole columns. Below the floor it scrolls sideways and cuts none.
 */

import { ChevronDownIcon, ChevronRightIcon } from "lucide-react"
import {
  type CSSProperties,
  type ReactNode,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
} from "react"
import { Link } from "react-router-dom"

import { type GoalDto, type Host, type HostRow, type TaskDto, useHostLists } from "@/api"
import { RefreshWarning } from "@/components/error-state"
import { HostLabel } from "@/components/host-label"
import { goalUsageRows, TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { type SessionAttention, SessionAttentionBadge } from "@/features/sessions/session-display"
import {
  BOARD_COLUMNS,
  BOARD_CONTAINER,
  BOARD_LINE,
  boardDotsWidth,
  HOST_HIDES,
  TaskRow,
  TOKEN_CELL,
  taskListQueryOptions,
} from "@/features/tasks"
import { useDaemonIdentities, withoutWorkerCopies } from "@/features/tasks/source-host"
import { stepTitle } from "@/features/tasks/steps"
import { cn, formatAbsolute } from "@/lib/format"
import { paths } from "@/routes/paths"
import {
  type BoardAttention,
  NO_BOARD_ATTENTION,
  taskAttentionReason,
  useBoardAttention,
} from "./attention"
import { useCollapsedLanes } from "./collapsed-lanes"
import { type GroupOrder, goalCounts, goallessTasks, goalSummary, orderGroups } from "./groups"
import { GOAL_STATUS_META, isTerminalGoalStatus } from "./status"

/**
 * The board's own scrollport: sticky only works against the box that scrolls.
 * It fills {@link BOARD_FRAME}, which is what the edge fades are positioned
 * against — they have to sit outside the thing that scrolls under them.
 *
 * `contain-paint` is what keeps that scrolling to *this* box. Without it the
 * rows hidden past the bottom edge still counted as overflow for the shell's
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

/** The list: the board's columns, a gap between groups, none inside one. */
const BOARD_LIST = cn(BOARD_COLUMNS, "min-w-[33.9rem] gap-y-2 px-3 pb-3")

type BoardTask = HostRow<TaskDto & { executionHost: string }>

/** Every task of the hosts the board reads, goal-less ones too. */
function useBoardTasks(hosts: readonly Host[]) {
  return useHostLists(hosts, (host) => ({
    ...taskListQueryOptions(host, {}),
    select: (rows: TaskDto[]) =>
      rows.map((row) => ({ ...row, executionHost: row.host ?? "local" })),
  }))
}

/** One group of the list: a goal with its tasks, or one task with no goal. */
type BoardGroup =
  | { kind: "goal"; key: string; goal: HostRow<GoalDto>; tasks: TaskDto[] }
  | { kind: "task"; key: string; row: BoardTask }

export function GoalBoard({
  hosts,
  goals,
  activeOnly = false,
  query = "",
  showHost = false,
  empty,
}: {
  /** The hosts the board reads; each goal's tasks are read on its own host. */
  hosts: readonly Host[]
  goals: HostRow<GoalDto>[]
  activeOnly?: boolean
  /** The title search, which narrows the tasks with no goal too. */
  query?: string
  /** Name each goal's host: the board shows goals of more than one. */
  showHost?: boolean
  /** Drawn in place of the list once the board has read that it has nothing. */
  empty?: ReactNode
}) {
  const tasks = useBoardTasks(hosts)
  const byGoal = useMemo(() => groupByGoal(tasks.rows, goals), [tasks.rows, goals])
  // Which rows — and which goals — are asking for a person, off the sessions
  // query the attention strip above the board already holds.
  const attentionByHost = useBoardAttention(hosts)
  const attention = (host: Host) => attentionByHost.get(host) ?? NO_BOARD_ATTENTION
  // The hosts whose tasks are not on screen: failed with no last read, or not
  // answered yet. Their goals are not empty, only unread.
  const unread = useMemo(
    () =>
      new Set<Host>([
        ...tasks.errors.filter((one) => !one.cached).map((one) => one.host),
        ...tasks.pending,
      ]),
    [tasks.errors, tasks.pending],
  )
  const { isCollapsed, setCollapsed } = useCollapsedLanes()
  // Folding swaps the goal's rows for its summary, so the fold button is mounted
  // again: the goal it was on takes the keyboard back.
  const refocus = useRef<string | null>(null)
  const visible = (task: TaskDto) =>
    !activeOnly || (task.status !== "finished" && task.status !== "cancelled")
  // A far task with no goal is listed by its source and by its worker, both
  // with no goal: the board shows the source's copy alone.
  const daemonIds = useDaemonIdentities(true)
  const noGoal = useMemo(
    () => goallessTasks(withoutWorkerCopies(tasks.rows, daemonIds), activeOnly, query),
    [tasks.rows, daemonIds, activeOnly, query],
  )
  // Active work first: what is asking for a person, then what is still
  // running, then what is done with — goals and goal-less tasks by one rule.
  const groups = useMemo(() => {
    const all: BoardGroup[] = [
      ...goals.map((goal): BoardGroup => {
        const key = rowKey(goal.host, goal.id)
        return { kind: "goal", key, goal, tasks: byGoal.get(key) ?? [] }
      }),
      ...noGoal.map((row): BoardGroup => ({ kind: "task", key: rowKey(row.host, row.id), row })),
    ]
    return orderGroups(all, (group) =>
      groupOrder(group, attentionByHost.get(groupHost(group)) ?? NO_BOARD_ATTENTION),
    )
  }, [goals, byGoal, noGoal, attentionByHost])
  // The step column is as wide as the widest dots of any workflow it shows.
  const dotsFloor = boardDotsWidth([
    ...goals.map((goal) => goal.steps.length),
    ...noGoal.map((row) => row.steps?.length ?? 0),
  ])
  const total =
    goals.reduce(
      (count, goal) =>
        count + (byGoal.get(rowKey(goal.host, goal.id))?.filter(visible).length ?? 0),
      0,
    ) + noGoal.length

  const warnings = (
    <>
      {tasks.errors.map(({ host, error, cached }) => (
        <RefreshWarning
          key={host}
          what={
            <>
              tasks on <HostLabel host={host} />
            </>
          }
          cached={cached}
          error={error}
          onRetry={tasks.refetch}
          className="shrink-0"
        />
      ))}
    </>
  )

  // The empty state says the board read nothing: it waits for the task reads, and
  // a failed one keeps its warning and its Retry instead.
  if (groups.length === 0 && empty) {
    if (tasks.errors.length > 0) return warnings
    return tasks.isSettled ? <>{empty}</> : null
  }

  // A host whose tasks failed or are still loading keeps its goals and says
  // they are not loaded; a failure has a warning of its own, also when every
  // host failed.
  return (
    <>
      {/* Outside the frame: the board fills the frame's whole height, so a
          warning inside it would push the board past the frame's bottom edge. */}
      {warnings}
      <div className={BOARD_FRAME}>
        {/* A named region the keyboard can put focus into: a scrollport nothing
          can focus only scrolls under a pointer. It is also the container whose
          width decides which columns the rows show. */}
        <section
          aria-label="Goals board"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className={cn(
            BOARD_BOX,
            BOARD_CONTAINER,
            "overflow-x-auto overflow-y-auto focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
          )}
        >
          <div className="sticky top-0 z-30 bg-background px-5 py-2">
            <h2 className="text-sm font-semibold">Tasks ({total})</h2>
          </div>
          <div
            className={BOARD_LIST}
            style={{ "--board-dots": `${dotsFloor}rem` } as CSSProperties}
          >
            {groups.map((group) => {
              if (group.kind === "task") {
                const { row } = group
                const own = attention(row.host)
                return (
                  <TaskRow
                    key={group.key}
                    host={row.host}
                    task={{ ...row, host: row.executionHost }}
                    steps={row.steps ?? []}
                    attention={own.byTask.get(row.id)}
                    lastActivityAt={own.lastActivity.get(row.id)}
                  />
                )
              }
              const { goal } = group
              // Finished goals open folded unless the user chose otherwise.
              const collapsed = isCollapsed(goal.id, isTerminalGoalStatus(goal.status))
              return (
                <GoalGroup
                  key={group.key}
                  goal={goal}
                  tasks={group.tasks}
                  showHost={showHost}
                  unread={unread.has(goal.host)}
                  visible={visible}
                  attention={attention(goal.host)}
                  collapsed={collapsed}
                  refocus={refocus}
                  onToggle={() => setCollapsed(goal.id, !collapsed)}
                />
              )
            })}
          </div>
        </section>
      </div>
    </>
  )
}

function GoalGroup({
  goal,
  tasks,
  showHost,
  unread,
  attention,
  visible,
  collapsed,
  refocus,
  onToggle,
}: {
  goal: HostRow<GoalDto>
  tasks: TaskDto[]
  showHost: boolean
  /** The goal's host has not given its tasks: an empty goal is not "No tasks". */
  unread: boolean
  /** The whole host's index; the goal takes its own rows out of it. */
  attention: BoardAttention
  visible: (task: TaskDto) => boolean
  collapsed: boolean
  refocus: RefObject<string | null>
  onToggle: () => void
}) {
  // An orchestrator belongs to no task, so it has no row to be flagged on: the
  // goal's own line is the only place it can ask for a person.
  const orchestrator = attention.byGoal.get(goal.id)
  const shown = tasks.filter(visible)
  const counts = goalCounts(tasks)
  const label = `${goal.title}: Tasks`

  return (
    // biome-ignore lint/a11y/useSemanticElements: this group is not a form control
    <div role="group" aria-label={label} className={BOARD_LINE}>
      <GoalLine
        goal={goal}
        tasks={tasks}
        showHost={showHost}
        orchestrator={orchestrator}
        collapsed={collapsed}
        refocus={refocus}
        onToggle={onToggle}
      >
        {collapsed ? (
          <span className="min-w-0 truncate text-muted-foreground">{goalSummary(tasks)}</span>
        ) : (
          <span className="shrink-0 text-muted-foreground">
            {counts.finished}/{counts.pipeline}
          </span>
        )}
      </GoalLine>
      {collapsed ? null : shown.length === 0 ? (
        <p
          data-goal-placeholder
          className="col-span-full flex h-8 min-w-0 items-center pl-3 text-xs text-muted-foreground"
        >
          <span className="min-w-0 truncate">{placeholderText(goal, tasks, unread)}</span>
        </p>
      ) : (
        shown.map((task) => (
          <TaskRow
            key={task.id}
            host={goal.host}
            task={task}
            steps={goal.steps}
            attention={attention.byTask.get(task.id)}
            lastActivityAt={attention.lastActivity.get(task.id)}
          />
        ))
      )}
    </div>
  )
}

function FoldToggle({
  goal,
  collapsed,
  refocus,
  onToggle,
}: {
  goal: GoalDto
  collapsed: boolean
  refocus: RefObject<string | null>
  onToggle: () => void
}) {
  const button = useRef<HTMLButtonElement>(null)
  // Runs after each render: the button of the other state is the one mounted now.
  useEffect(() => {
    if (refocus.current !== goal.id) return
    refocus.current = null
    button.current?.focus()
  })
  return (
    <Button
      ref={button}
      variant="ghost"
      size="icon-xs"
      aria-expanded={!collapsed}
      aria-label={`${collapsed ? "Expand" : "Collapse"} ${goal.title}`}
      onClick={() => {
        refocus.current = goal.id
        onToggle()
      }}
    >
      {collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
    </Button>
  )
}

/** The goal's title as a link to its panel, with the rest of the goal in its tooltip. */
function GoalTitle({ goal, tasks }: { goal: HostRow<GoalDto>; tasks: TaskDto[] }) {
  const counts = goalCounts(tasks)
  const repos = goal.repos.map((repo) => `${repo.path} [${repo.base_branch}]`).join("\n")
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Link
            to={paths.goal(goal.id, goal.host)}
            className="min-w-0 truncate text-sm font-medium underline-offset-4 hover:underline"
          />
        }
      >
        {goal.title}
      </TooltipTrigger>
      {/* The title first, because the line truncates it and this is the only
          place the rest of it is readable; the repositories are what the goal
          is *about*, so they follow it. */}
      <TooltipContent className="flex-col items-start gap-0.5">
        <span className="font-medium">{goal.title}</span>
        <span className="whitespace-pre-line text-background/70">{repos || "Open the goal"}</span>
        <span className="text-background/70">created {formatAbsolute(goal.created_at)}</span>
        <span className="text-background/70">
          {counts.finished}/{counts.pipeline} done · {counts.failed} failed · {counts.stalled}{" "}
          stalled
        </span>
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * A goal's own line, shown at rest: the fold chevron, the title link, the
 * host, the status, what the caller puts in (a count or a summary) and the
 * orchestrator's flag, then the tokens of the whole goal in the token column,
 * over its rows' own figures.
 */
function GoalLine({
  goal,
  tasks,
  showHost,
  orchestrator,
  collapsed,
  refocus,
  onToggle,
  children,
}: {
  goal: HostRow<GoalDto>
  tasks: TaskDto[]
  showHost: boolean
  orchestrator?: SessionAttention
  collapsed: boolean
  refocus: RefObject<string | null>
  onToggle: () => void
  children: ReactNode
}) {
  const status = GOAL_STATUS_META[goal.status]
  return (
    <div
      data-goal-line
      data-goal-folded={collapsed ? "" : undefined}
      className={cn(BOARD_LINE, "h-8 text-xs")}
    >
      {/* Clipped, so a very narrow board never draws it over the token total;
          the padding keeps the chevron's and the link's focus rings inside. */}
      <div className="col-span-4 -ml-[3px] flex min-w-0 items-center gap-2 overflow-hidden py-[3px] pl-[3px] whitespace-nowrap">
        <FoldToggle goal={goal} collapsed={collapsed} refocus={refocus} onToggle={onToggle} />
        <GoalTitle goal={goal} tasks={tasks} />
        {showHost ? <HostLabel host={goal.host} className={HOST_HIDES} /> : null}
        <span className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
          <span className={cn("size-1.5 shrink-0 rounded-full", status.dot)} aria-hidden />
          {status.label}
        </span>
        {children}
        {orchestrator ? <SessionAttentionBadge size="sm" attention={orchestrator} /> : null}
      </div>
      {/* What the whole goal has cost — the orchestrator and every column's
          agents. The hint behind it names the halves and splits the total by
          who spent it. */}
      <span data-column="tokens" className={cn(TOKEN_CELL, "text-muted-foreground")}>
        <TokenFigure
          usage={goal.usage.total}
          rows={goalUsageRows(goal.usage, (step) => stepTitle(goal.steps, step))}
        />
      </span>
    </div>
  )
}

/** What a goal with no shown task says in place of its rows. */
function placeholderText(goal: GoalDto, tasks: TaskDto[], unread: boolean): string {
  if (tasks.length === 0 && unread) return "Tasks are not loaded"
  if (tasks.length > 0) return "No active tasks"
  return goal.status === "planning"
    ? "No tasks yet — the orchestrator is still working"
    : "No tasks"
}

/** The loading board follows the shape of its rows. */
export function BoardSkeleton() {
  return (
    <div className={BOARD_FRAME} aria-hidden>
      <div className={cn(BOARD_BOX, "overflow-hidden")}>
        <div className="px-5 py-2">
          <Skeleton className="h-3 w-20" />
        </div>
        {[0, 1, 2].map((group) => (
          <div key={group} className="flex flex-col gap-1 px-3 pb-2">
            <Skeleton className="h-6 w-1/2" />
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
          </div>
        ))}
      </div>
    </div>
  )
}

function groupHost(group: BoardGroup): Host {
  return group.kind === "goal" ? group.goal.host : group.row.host
}

/**
 * Where a group sorts. A goal asks for a person when its orchestrator is
 * blocked, one of its agents is, or one of its tasks failed or stalled — the
 * same rule the attention strip lists a task by, so a row and the strip above
 * it never disagree about what is stuck. A task with no goal takes the same
 * rule by its own state.
 */
function groupOrder(group: BoardGroup, attention: BoardAttention): GroupOrder {
  const flagged = (task: TaskDto) =>
    attention.byTask.has(task.id) || taskAttentionReason(task) !== null
  if (group.kind === "task") {
    const { row } = group
    return {
      needsAttention: flagged(row),
      terminal: row.status === "finished" || row.status === "cancelled",
      updatedAt: row.updated_at,
      id: row.id,
    }
  }
  const { goal, tasks } = group
  return {
    needsAttention: attention.byGoal.has(goal.id) || tasks.some(flagged),
    terminal: isTerminalGoalStatus(goal.status),
    updatedAt: goal.updated_at,
    id: goal.id,
  }
}

/** A row's own key: ids are only unique on the host that minted them. */
function rowKey(host: Host, id: string): string {
  return `${host}\n${id}`
}

/** Group the listed goals' tasks, each on its goal's host. */
function groupByGoal(tasks: BoardTask[], goals: HostRow<GoalDto>[]): Map<string, TaskDto[]> {
  const groups = new Map<string, TaskDto[]>()
  const shown = new Set(goals.map((goal) => rowKey(goal.host, goal.id)))
  for (const task of tasks) {
    // A task with no goal is a group of its own.
    if (!task.goal_id) continue
    const key = rowKey(task.host, task.goal_id)
    // Only the goals the board shows get a group.
    if (!shown.has(key)) continue
    let group = groups.get(key)
    if (!group) {
      group = []
      groups.set(key, group)
    }
    group.push({ ...task, host: task.executionHost })
  }
  return groups
}
