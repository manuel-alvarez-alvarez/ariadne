/**
 * The sessions screen: every agent Ariadne has run, newest activity first.
 *
 * The panels answer "what is this task's author doing"; this screen answers
 * the question no panel can — "what is running right now", and "which agent
 * failed while I was away". A row opens the session's own panel (`?session=`)
 * over this screen rather than navigating anywhere, so the list — and the
 * filters that produced it — stays behind it with the picked row still marked.
 *
 * `GET /v1/sessions` answers whole (see `queries.ts`). `status`, `seat`,
 * `goal` and `task` are asked of the daemon, or narrowed by its answer; the
 * agent, directory, activity window and search are applied to the rows here
 * (see {@link matchesBrowse}).
 */

import { useQuery } from "@tanstack/react-query"
import { ListChecksIcon, type LucideIcon, SearchIcon, TargetIcon, XIcon } from "lucide-react"
import { useMemo } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"

import { ApiError, type GoalDto, type SessionDto, type TaskDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { ScrollableTable } from "@/components/scroll-edge"
import { TokenFigure } from "@/components/token-figure"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { goalsQueryOptions } from "@/features/goals/queries"
import { parseModelRef, pinLabel } from "@/features/models/model-ref"
import { PullRequestLink } from "@/features/pull-requests/pull-request-link"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { sessionColumn } from "@/features/tasks/steps"
import { cn, SEAT_LABELS, shortId } from "@/lib/format"
import { paths, sessionPanelFrom } from "@/routes/paths"
import { BROWSE_FILTER_PARAMS, type BrowseFilters, useBrowseFilters } from "./browse-filters"
import { ActivityFilter, AgentFilter, DirectoryFilter, FilterMenu, TypedFilter } from "./filter-bar"
import {
  ALL,
  ATTENTION,
  type FilterParam,
  GOAL_PARAM,
  LIVE,
  SEAT_PARAM,
  SEATS,
  STATUS_PARAM,
  STATUSES,
  seatFilterLabel,
  statusFilters,
  statusLabel,
  TASK_PARAM,
} from "./filters"
import { byId, type SessionListFilters, sessionsQueryOptions } from "./queries"
import {
  SESSION_STATUS_META,
  SessionAttentionBadge,
  SessionStatusBadge,
  shownAttention,
} from "./session-display"
import { useSessionFilters } from "./use-session-filters"

const LOADING_ROWS = ["first", "second", "third"]
const SESSION_COLUMNS = [
  { key: "title", header: "Title", className: "min-w-56" },
  { key: "status", header: "Status" },
  { key: "work", header: "Work", className: "min-w-48" },
  { key: "agent", header: "Agent" },
  { key: "tokens", header: "Tokens", className: "text-right" },
  { key: "activity", header: "Last activity", className: "text-right" },
]
const NETWORK_ERROR_DESCRIPTION =
  "The daemon is not answering. Check the URL in settings and that it is listening on TCP."

function errorDescription(error: unknown): string | undefined {
  return ApiError.is(error) && error.isNetworkError ? NETWORK_ERROR_DESCRIPTION : undefined
}

function movedAt(session: SessionDto): string {
  return session.last_activity_at ?? session.created_at
}

/** What a row is about: the task it ran, the goal it planned, or the request it reviewed. */
function ariadneTitle(
  session: SessionDto,
  goalsById: Map<string, GoalDto>,
  tasksById: Map<string, TaskDto>,
): string {
  if (session.task_id) return tasksById.get(session.task_id)?.title ?? shortId(session.task_id)
  if (session.goal_id) return goalsById.get(session.goal_id)?.title ?? shortId(session.goal_id)
  return session.pull_request_id
    ? `Pull request ${shortId(session.pull_request_id)}`
    : shortId(session.id)
}

/** Whether a session matches the agent, directory, activity and search filters. */
function matchesBrowse(
  session: SessionDto,
  title: string,
  { agent, dir, since, until, q }: BrowseFilters,
): boolean {
  if (agent && parseModelRef(session.model)?.agent !== agent) return false
  if (dir && !(session.worktree_path ?? "").startsWith(dir)) return false
  const at = movedAt(session)
  if (since && new Date(at) < new Date(since)) return false
  if (until && new Date(at) > new Date(until)) return false
  if (q && !title.toLowerCase().includes(q.toLowerCase())) return false
  return true
}

export function SessionsPage() {
  const [search] = useSearchParams()
  const navigate = useNavigate()
  const {
    status,
    seat,
    goal,
    task,
    filterBy,
    clearFilters: clearSessionFilters,
  } = useSessionFilters()
  const browse = useBrowseFilters()

  const filters: SessionListFilters = {
    ...statusFilters(status),
    seat: seat ?? undefined,
    goal: goal ?? undefined,
    task: task ?? undefined,
  }
  const sessions = useQuery(sessionsQueryOptions(filters))

  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(taskListQueryOptions())
  const goalsById = byId(goals.data)
  const tasksById = byId(tasks.data)

  const rows = useMemo(
    () =>
      (sessions.data ?? [])
        .filter((session) =>
          matchesBrowse(session, ariadneTitle(session, goalsById, tasksById), browse.filters),
        )
        .sort((a, b) => movedAt(b).localeCompare(movedAt(a))),
    [sessions.data, browse.filters, goalsById, tasksById],
  )

  const filtered =
    status !== null || seat !== null || BROWSE_FILTER_PARAMS.some((param) => browse.values[param])
  const clearFilters = () => clearSessionFilters(BROWSE_FILTER_PARAMS)

  function handleSelect(session: SessionDto) {
    void navigate(sessionPanelFrom(paths.sessions(), search, session.id))
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Sessions"
        description="Every agent Ariadne has run. Pick one to watch its console."
      />

      {/* The search first, since finding one conversation by what it was
          about is what the bar is used for most, with the count at the other end
          of its row. Under it, one compact trigger per
          filter, each naming what it filters and what it is set to. The two
          free-form ones — the directory and the activity window — open a
          popover rather than sitting in the bar as bare fields: an empty date
          field in WebKit shows today's date, which read as a filter that was
          set. */}
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative w-full max-w-md min-w-48 flex-1">
            <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <TypedFilter
              label="Search titles"
              placeholder="Search titles"
              value={browse.values.q}
              onSettle={browse.filterBy}
              param="q"
              className="pl-8"
            />
          </div>

          <div className="ml-auto flex items-center gap-3">
            <p className="text-sm text-muted-foreground tabular-nums">
              {rows.length} of {sessions.data?.length ?? 0} sessions
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <AgentFilter
            value={browse.values.agent}
            onSelect={(agent) => browse.filterBy("agent", agent)}
          />
          <FilterMenu
            what="Status"
            value={status ?? ALL}
            valueLabel={status ? statusLabel(status) : null}
            onSelect={(value) => filterBy(STATUS_PARAM, value)}
            lead={[
              { value: LIVE, label: "Live" },
              { value: ATTENTION, label: "Needs attention" },
            ]}
            options={STATUSES.map((known) => ({
              value: known,
              label: SESSION_STATUS_META[known].label,
            }))}
            allLabel="All statuses"
          />
          <FilterMenu
            what="Seat"
            value={seat ?? ALL}
            valueLabel={seat ? seatFilterLabel(seat) : null}
            onSelect={(value) => filterBy(SEAT_PARAM, value)}
            options={SEATS.map((known) => ({ value: known, label: SEAT_LABELS[known] }))}
            allLabel="All seats"
          />
          <DirectoryFilter value={browse.values.dir} onSettle={browse.filterBy} />
          <ActivityFilter
            since={browse.values.since}
            until={browse.values.until}
            onChange={browse.filterByMany}
          />

          {filtered ? (
            <Button variant="ghost" size="sm" onClick={clearFilters}>
              <XIcon />
              Clear filters
            </Button>
          ) : null}
        </div>
      </div>

      <ScopeChips goal={goal} task={task} onClear={filterBy} />

      {sessions.isError ? (
        <ErrorState
          title="Could not load sessions"
          error={sessions.error}
          description={errorDescription(sessions.error)}
          onRetry={() => void sessions.refetch()}
        />
      ) : null}
      <SessionsTable
        rows={rows}
        pending={sessions.isPending}
        goalsById={goalsById}
        tasksById={tasksById}
        onSelect={handleSelect}
        onScope={filterBy}
      />
    </div>
  )
}

function SessionsTable({
  rows,
  pending,
  goalsById,
  tasksById,
  onSelect,
  onScope,
}: {
  rows: SessionDto[]
  pending: boolean
  goalsById: Map<string, GoalDto>
  tasksById: Map<string, TaskDto>
  onSelect: (session: SessionDto) => void
  onScope: (param: FilterParam, id: string) => void
}) {
  return (
    <ScrollableTable className="rounded-xl border">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          {SESSION_COLUMNS.map((column) => (
            <TableHead key={column.key} className={column.className}>
              {column.header}
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {pending ? (
          <SessionLoadingRows />
        ) : rows.length ? (
          rows.map((session) => (
            <SessionRow
              key={session.id}
              session={session}
              title={ariadneTitle(session, goalsById, tasksById)}
              goalTitle={session.goal_id ? goalsById.get(session.goal_id)?.title : undefined}
              taskTitle={session.task_id ? tasksById.get(session.task_id)?.title : undefined}
              column={sessionColumn(
                session,
                tasksById.get(session.task_id ?? ""),
                goalsById.get(session.goal_id ?? ""),
              )}
              onSelect={() => onSelect(session)}
              onScope={onScope}
            />
          ))
        ) : (
          <TableRow className="hover:bg-transparent">
            <TableCell colSpan={SESSION_COLUMNS.length} className="p-0">
              <p className="px-4 py-8 text-center text-sm text-muted-foreground">
                No sessions found.
              </p>
            </TableCell>
          </TableRow>
        )}
      </TableBody>
    </ScrollableTable>
  )
}

function SessionLoadingRows() {
  return LOADING_ROWS.map((row) => (
    <TableRow key={row} className="hover:bg-transparent">
      {SESSION_COLUMNS.map((column) => (
        <TableCell key={column.key}>
          <Skeleton className="h-4 w-full" />
        </TableCell>
      ))}
    </TableRow>
  ))
}

function SessionRow({
  session,
  title,
  goalTitle,
  taskTitle,
  column,
  onSelect,
  onScope,
}: {
  session: SessionDto
  title: string
  goalTitle: string | undefined
  taskTitle: string | undefined
  /** The workflow column a step agent staffs, which names it in place of its seat. */
  column: string | undefined
  onSelect: () => void
  /** Narrow the table to a goal or a task picked in the row. */
  onScope: (param: FilterParam, id: string) => void
}) {
  const directory = session.worktree_path
  return (
    <TableRow
      tabIndex={0}
      className="cursor-pointer bg-background focus-visible:outline-none focus-visible:-outline-offset-2 focus-visible:ring-2 focus-visible:ring-ring/50"
      aria-label={title}
      onClick={(event) => {
        const target = event.target as Element
        if (event.currentTarget.contains(target) && !target.closest("button, a")) onSelect()
      }}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault()
          onSelect()
        }
      }}
    >
      <TableCell className="max-w-72">
        <span className="block truncate" title={title}>
          {title}
        </span>
        {/* Where the agent ran: a goal's or a task's worktree. It rides under
            the title rather than taking a column of its own. */}
        {directory ? (
          <span
            className="block truncate font-mono text-xs text-muted-foreground"
            title={directory}
          >
            {directory}
          </span>
        ) : null}
      </TableCell>
      <TableCell>
        <div className="flex flex-wrap items-center gap-1.5">
          <SessionStatusBadge status={session.status} />
          <RowAttention session={session} />
        </div>
      </TableCell>
      <TableCell className="max-w-64">
        <WorkCell
          session={session}
          goalTitle={goalTitle}
          taskTitle={taskTitle}
          column={column}
          onScope={onScope}
        />
      </TableCell>
      <TableCell className="max-w-40 truncate text-xs text-muted-foreground">
        {pinLabel(session.model, session.effort)}
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        <TokenFigure usage={session.usage} />
      </TableCell>
      <TableCell className="text-right tabular-nums text-muted-foreground">
        <When at={movedAt(session)} format="age" label="last activity" />
      </TableCell>
    </TableRow>
  )
}

/** The reason badge of a row, in the words the attention list uses. */
function RowAttention({ session }: { session: SessionDto }) {
  const attention = shownAttention(session)
  return attention ? <SessionAttentionBadge attention={attention} /> : null
}

/**
 * What an Ariadne session works on, in one cell: the seat it holds, the goal
 * it is under, and the task under that — a step agent's seat named by the
 * workflow column it staffs. The seat is a badge of one width, so
 * the goals and tasks of every row line up beside it. Picking the goal or the
 * task narrows the table to its sessions, the way the chips above it do. A
 * pull request session has neither: its request stands in their place, by
 * title, linked to the forge.
 */
function WorkCell({
  session,
  goalTitle,
  taskTitle,
  column,
  onScope,
}: {
  session: SessionDto
  goalTitle: string | undefined
  taskTitle: string | undefined
  column: string | undefined
  onScope: (param: FilterParam, id: string) => void
}) {
  const { goal_id: goalId, task_id: taskId, seat, pull_request_id: pullRequestId } = session
  if (!goalId && !taskId && !seat && !pullRequestId) return <Dash />
  return (
    <div className="flex min-w-0 items-start gap-2 text-xs">
      {seat ? (
        <Badge
          variant="outline"
          className="w-22 shrink-0 truncate font-normal text-muted-foreground"
        >
          {column ?? SEAT_LABELS[seat]}
        </Badge>
      ) : null}
      <div className="flex min-w-0 flex-col gap-0.5 pt-px">
        {goalId ? (
          <WorkLink
            icon={TargetIcon}
            name={goalTitle ?? shortId(goalId)}
            what="goal"
            onClick={() => onScope(GOAL_PARAM, goalId)}
          />
        ) : null}
        {taskId ? (
          <WorkLink
            icon={ListChecksIcon}
            name={taskTitle ?? shortId(taskId)}
            what="task"
            onClick={() => onScope(TASK_PARAM, taskId)}
            className="text-muted-foreground"
          />
        ) : null}
        {pullRequestId ? <PullRequestLink id={pullRequestId} /> : null}
      </div>
    </div>
  )
}

/** A goal or a task in the Work cell: its icon and name, narrowing to it. */
function WorkLink({
  icon: Icon,
  name,
  what,
  onClick,
  className,
}: {
  icon: LucideIcon
  name: string
  what: "goal" | "task"
  onClick: () => void
  className?: string
}) {
  return (
    <button
      type="button"
      title={`${what === "goal" ? "Goal" : "Task"}: ${name}`}
      aria-label={`Show the sessions of the ${what} ${name}`}
      onClick={onClick}
      className={cn(
        "flex min-w-0 items-center gap-1 rounded-sm text-left underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
    >
      <Icon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 truncate">{name}</span>
    </button>
  )
}

function Dash() {
  return <span>—</span>
}

/**
 * What the screen is narrowed to, and the way out of it.
 *
 * A goal or a task is a filter with no menu behind it — there is no list of
 * every goal worth putting in a dropdown, and a scope arrives as a link from
 * the work itself (`#/sessions?goal=<id>`) or as one deep link kept between
 * visits. So it shows as a chip: the thing's own name where the app knows it,
 * its short id where it does not, and one click to drop it.
 */
function ScopeChips({
  goal,
  task,
  onClear,
}: {
  goal: string | null
  task: string | null
  onClear: (param: FilterParam, value: string) => void
}) {
  const goals = useQuery({ ...goalsQueryOptions(), enabled: goal !== null })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: task !== null })

  if (!goal && !task) return null
  return (
    <div className="flex flex-wrap items-center gap-2">
      {goal ? (
        <ScopeChip
          what="Goal"
          name={goals.data?.find((one) => one.id === goal)?.title ?? shortId(goal)}
          onClear={() => onClear(GOAL_PARAM, ALL)}
        />
      ) : null}
      {task ? (
        <ScopeChip
          what="Task"
          name={tasks.data?.find((one) => one.id === task)?.title ?? shortId(task)}
          onClear={() => onClear(TASK_PARAM, ALL)}
        />
      ) : null}
    </div>
  )
}

function ScopeChip({
  what,
  name,
  onClear,
}: {
  what: "Goal" | "Task"
  name: string
  onClear: () => void
}) {
  return (
    <Badge variant="outline" className="max-w-80 gap-1.5 pr-1 pl-2.5">
      <span className="text-muted-foreground">{what}</span>
      <span className="min-w-0 truncate" title={name}>
        {name}
      </span>
      <Button
        variant="ghost"
        size="icon"
        aria-label={`Show sessions for every ${what.toLowerCase()}`}
        onClick={onClear}
        className="size-6 rounded-full text-muted-foreground hover:text-foreground"
      >
        <XIcon className="size-4" />
      </Button>
    </Badge>
  )
}
