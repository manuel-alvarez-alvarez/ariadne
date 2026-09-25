/**
 * The sessions screen: every agent Ariadne has run, and every conversation an
 * ACP agent stored on its own that Ariadne did not start — one table over the
 * two of them, newest activity first.
 *
 * The panels answer "what is this task's author doing"; this screen answers
 * the question no panel can — "what is running right now", "which agent
 * failed while I was away", and "is there a conversation out there worth
 * picking back up". A row opens the session's own panel (`?session=`) over
 * this screen rather than navigating anywhere, so the list — and the filters
 * that produced it — stays behind it with the picked row still marked. An
 * outside row has no session yet to open: picking one resumes it first (see
 * {@link handleSelect}), which is what turns it into one.
 *
 * The daemon answers the two kinds over two endpoints — `GET /v1/sessions`
 * whole, `GET /v1/outside-sessions` a page at a time — so this screen fetches
 * both and merges them client-side into the one table the task asks for.
 * `status`, `seat`, `goal` and `task` only ever narrow the Ariadne half, being
 * things an outside session has none of; a goal, a task or a real status
 * (`live` and `attention` included) therefore leaves nothing an outside row
 * could match, and its half of the request is skipped rather than asked for
 * an empty answer. `kind` picks which half is shown at all, and `agent`,
 * `dir`, `since`, `until` and `q` are asked of the outside endpoint the way
 * they always were and applied to the Ariadne rows here, so a filter reads as
 * one filter over the whole table rather than one that only works on half of
 * it.
 *
 * Paging follows from the same split: the Ariadne half arrives whole, so only
 * the outside half grows under Load more, and the count line — shown only
 * while that half is part of the mix — adds what has not loaded yet to what
 * has.
 */

import { useInfiniteQuery, useQuery } from "@tanstack/react-query"
import {
  ListChecksIcon,
  type LucideIcon,
  PlusIcon,
  RefreshCwIcon,
  SearchIcon,
  TargetIcon,
  XIcon,
} from "lucide-react"
import { useMemo, useRef, useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"
import { toast } from "sonner"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { DataTable } from "@/components/data-table"
import { PageHeader } from "@/components/page-header"
import { TokenFigure } from "@/components/token-figure"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { goalsQueryOptions } from "@/features/goals/queries"
import { parseModelRef, pinLabel } from "@/features/models/model-ref"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { cn, describeError, SEAT_LABELS, shortId } from "@/lib/format"
import { paths, sessionPanelFrom, sessionTerminalFrom } from "@/routes/paths"
import {
  ActivityFilter,
  AgentFilter,
  DirectoryFilter,
  FilterMenu,
  TypedFilter,
  WindowFilter,
} from "./filter-bar"
import {
  ALL,
  ATTENTION,
  type FilterParam,
  GOAL_PARAM,
  LIVE,
  ROLE_PARAM,
  ROLES,
  roleLabel,
  STATUS_PARAM,
  STATUSES,
  statusFilters,
  statusLabel,
  TASK_PARAM,
} from "./filters"
import { NewSessionDialog } from "./new-session-dialog"
import {
  DEFAULT_WINDOW,
  KIND_LABELS,
  OUTSIDE_FILTER_PARAMS,
  useOutsideSessionFilters,
  WINDOW_PARAM,
} from "./outside-filters"
import {
  byId,
  type OutsideSessionDto,
  outsideSessionsQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
  useResumeOutsideSession,
} from "./queries"
import { SESSION_STATUS_META, SessionAttentionBadge, SessionStatusBadge } from "./session-display"
import { useSessionFilters } from "./use-session-filters"

/** One row of the merged table, told apart by which listing it came from. */
type Row =
  | { kind: "ariadne"; session: SessionDto }
  | { kind: "outside"; session: OutsideSessionDto }

function rowKey(row: Row): string {
  return row.kind === "ariadne"
    ? row.session.id
    : `${row.session.agent_id}:${row.session.internal_session_id}`
}

function movedAt(row: Row): string {
  if (row.kind === "outside") return row.session.last_activity_at
  return row.session.last_activity_at ?? row.session.created_at
}

/**
 * What an Ariadne row is about: the task it ran, the goal it planned, or a
 * loose session's own title.
 */
function ariadneTitle(
  session: SessionDto,
  goalsById: Map<string, GoalDto>,
  tasksById: Map<string, TaskDto>,
): string {
  if (session.task_id) return tasksById.get(session.task_id)?.title ?? shortId(session.task_id)
  if (session.goal_id) return goalsById.get(session.goal_id)?.title ?? shortId(session.goal_id)
  return session.title ?? "Loose session"
}

/**
 * Whether an Ariadne row matches the filters the daemon only takes for the
 * outside listing — applied here so one filter reads as one filter over the
 * whole table, not one that only works on half of it.
 */
function matchesBrowse(
  session: SessionDto,
  title: string,
  {
    agent,
    dir,
    since,
    until,
    q,
  }: { agent?: string; dir?: string; since?: string; until?: string; q?: string },
): boolean {
  if (agent && parseModelRef(session.model)?.agent !== agent) return false
  if (dir && !(session.worktree_path ?? "").startsWith(dir)) return false
  const at = session.last_activity_at ?? session.created_at
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
  const browse = useOutsideSessionFilters()

  const ariadneFilters: SessionListFilters = {
    ...statusFilters(status),
    seat: seat ?? undefined,
    goal: goal ?? undefined,
    task: task ?? undefined,
  }
  const ariadneSessions = useQuery(sessionsQueryOptions(ariadneFilters))

  // A goal or a task is a structural impossibility for an outside session —
  // it has neither — so its half of the request is skipped rather than asked
  // for an answer that can only be empty. Picking a kind of "Ariadne" does
  // the same, on purpose. A status or a seat is not: they still reach the
  // outside endpoint's own filters (`agent`, `dir`, `since`, `until`, `q`),
  // which keep answering — it is only the merged *rows* that leave an
  // outside session out while one of those is set, since none has a status
  // or a seat to match it with (see `outsideEligible` below).
  const outsideFetchEnabled = browse.kind !== "ariadne" && !goal && !task
  const outsideEligible = outsideFetchEnabled && !status && !seat
  const refreshWanted = useRef(false)
  const outsideSessions = useInfiniteQuery({
    ...outsideSessionsQueryOptions(browse.filters, () => {
      const wanted = refreshWanted.current
      refreshWanted.current = false
      return wanted
    }),
    enabled: outsideFetchEnabled,
  })

  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(taskListQueryOptions())
  const goalsById = byId(goals.data)
  const tasksById = byId(tasks.data)

  const outsidePages = outsideSessions.data?.pages
  const outsideRows = useMemo(
    () => (outsidePages ?? []).flatMap((page) => page.sessions),
    [outsidePages],
  )

  const ariadneRows = useMemo(() => {
    if (browse.kind === "outside") return []
    return (ariadneSessions.data ?? []).filter((session) =>
      matchesBrowse(session, ariadneTitle(session, goalsById, tasksById), browse.filters),
    )
  }, [ariadneSessions.data, browse.kind, browse.filters, goalsById, tasksById])

  const rows = useMemo(() => {
    const merged: Row[] = [
      ...ariadneRows.map((session): Row => ({ kind: "ariadne", session })),
      ...(outsideEligible ? outsideRows.map((session): Row => ({ kind: "outside", session })) : []),
    ]
    return merged.sort((a, b) => movedAt(b).localeCompare(movedAt(a)))
  }, [ariadneRows, outsideEligible, outsideRows])

  const outsideTotal = outsidePages?.at(-1)?.total ?? outsideRows.length
  const total = ariadneRows.length + (outsideEligible ? outsideTotal : 0)

  const filtered =
    status !== null ||
    seat !== null ||
    browse.window !== DEFAULT_WINDOW ||
    OUTSIDE_FILTER_PARAMS.some((param) => browse.values[param])
  // One search-param update, window included, rather than one call per hook:
  // two separate `setSearch` calls in the same handler would each start from
  // the same pre-click params, so the second would undo the first's clears.
  const clearFilters = () => clearSessionFilters([...OUTSIDE_FILTER_PARAMS, WINDOW_PARAM])

  const [starting, setStarting] = useState(false)
  const [resumingKey, setResumingKey] = useState<string | null>(null)
  const resume = useResumeOutsideSession()

  function handleSelect(row: Row) {
    if (row.kind === "ariadne") {
      void navigate(sessionPanelFrom(paths.sessions(), search, row.session.id))
      return
    }
    const key = rowKey(row)
    if (resumingKey) return
    setResumingKey(key)
    resume.mutate(
      { agent_id: row.session.agent_id, internal_session_id: row.session.internal_session_id },
      {
        onSuccess: (live) => {
          setResumingKey(null)
          void navigate(sessionPanelFrom(paths.sessions(), search, live.id))
        },
        onError: (error) => {
          setResumingKey(null)
          toast.error("Could not resume", { description: describeError(error) })
        },
      },
    )
  }

  const isPending = ariadneSessions.isPending || (outsideEligible && outsideSessions.isPending)
  const isError = ariadneSessions.isError || outsideSessions.isError
  const error = ariadneSessions.error ?? outsideSessions.error

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Sessions"
        description="Every agent Ariadne has run, and every conversation an ACP agent stored on its own. Pick one to watch its console."
        actions={
          <Button onClick={() => setStarting(true)}>
            <PlusIcon />
            New session
          </Button>
        }
      />

      <NewSessionDialog
        open={starting}
        onOpenChange={setStarting}
        onStarted={(session) =>
          // Straight into its console: it is waiting for the first prompt.
          void navigate(sessionTerminalFrom(paths.sessions(), search, session.id))
        }
      />

      {/* The search first, since finding one conversation by what it was
          about is what the bar is used for most, with the count and Refresh
          at the other end of its row. Under it, one compact trigger per
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
            {outsideEligible ? (
              <p className="text-sm text-muted-foreground tabular-nums">{`${rows.length} of ${total} sessions`}</p>
            ) : null}
            <Button
              variant="outline"
              disabled={!outsideFetchEnabled}
              // Busy while the first page is in flight, its own refetch included:
              // a refetch asked for over a request that is already running is the
              // running one, which carries no `refresh`, and the flag this button
              // set would then be spent on whatever asked next. A page loading
              // underneath is Load more's spinner rather than this one's.
              pending={outsideSessions.isFetching && !outsideSessions.isFetchingNextPage}
              onClick={() => {
                refreshWanted.current = true
                void outsideSessions.refetch()
              }}
            >
              <RefreshCwIcon />
              Refresh
            </Button>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <FilterMenu
            what="Kind"
            value={browse.values.kind || ALL}
            valueLabel={browse.kind ? KIND_LABELS[browse.kind] : null}
            onSelect={(value) => browse.filterBy("kind", value)}
            options={[
              { value: "ariadne", label: KIND_LABELS.ariadne },
              { value: "outside", label: KIND_LABELS.outside },
            ]}
            allLabel="All kinds"
          />
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
            what="Role"
            value={seat ?? ALL}
            valueLabel={seat ? roleLabel(seat) : null}
            onSelect={(value) => filterBy(ROLE_PARAM, value)}
            options={ROLES.map((known) => ({ value: known, label: SEAT_LABELS[known] }))}
            allLabel="All roles"
          />
          <WindowFilter value={browse.window} onSelect={browse.filterWindow} />
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

      <DataTable
        query={{
          data: isPending ? undefined : rows,
          isPending,
          isError,
          error,
          refetch: () => {
            void ariadneSessions.refetch()
            void outsideSessions.refetch()
          },
        }}
        errorTitle="Could not load sessions"
        columns={[
          { header: "Title", className: "min-w-56" },
          { header: "Status" },
          { header: "Work", className: "min-w-48" },
          { header: "Agent" },
          { header: "Tokens", className: "text-right" },
          { header: "Last activity", className: "text-right" },
        ]}
        empty={
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">No sessions found.</p>
        }
        rowKey={rowKey}
        renderRow={(row) => (
          <SessionRow
            key={rowKey(row)}
            row={row}
            title={
              row.kind === "ariadne"
                ? ariadneTitle(row.session, goalsById, tasksById)
                : row.session.first_prompt
            }
            goalTitle={
              row.kind === "ariadne" && row.session.goal_id
                ? goalsById.get(row.session.goal_id)?.title
                : undefined
            }
            taskTitle={
              row.kind === "ariadne" && row.session.task_id
                ? tasksById.get(row.session.task_id)?.title
                : undefined
            }
            resuming={resumingKey === rowKey(row)}
            onSelect={() => handleSelect(row)}
            onScope={filterBy}
          />
        )}
      />
      {outsideEligible && outsideSessions.hasNextPage ? (
        <Button
          variant="outline"
          className="self-center"
          pending={outsideSessions.isFetchingNextPage}
          onClick={() => void outsideSessions.fetchNextPage()}
        >
          Load more
        </Button>
      ) : null}
    </div>
  )
}

function SessionRow({
  row,
  title,
  goalTitle,
  taskTitle,
  resuming,
  onSelect,
  onScope,
}: {
  row: Row
  title: string
  goalTitle: string | undefined
  taskTitle: string | undefined
  resuming: boolean
  onSelect: () => void
  /** Narrow the table to a goal or a task picked in the row. */
  onScope: (param: FilterParam, id: string) => void
}) {
  const directory =
    row.kind === "outside" ? row.session.working_directory : row.session.worktree_path
  return (
    <TableRow
      className="cursor-pointer"
      aria-busy={resuming}
      onClick={(event) => {
        const target = event.target as Element
        if (event.currentTarget.contains(target) && !target.closest("button, a")) onSelect()
      }}
    >
      <TableCell className="max-w-72">
        <span className="block truncate" title={title}>
          {title}
        </span>
        {/* Where the agent ran, for every kind of row alike: a goal's or a
            task's worktree, a loose session's directory, an outside
            conversation's. It rides under the title rather than taking a
            column of its own. */}
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
        {resuming ? (
          <span className="text-xs text-muted-foreground">Resuming…</span>
        ) : row.kind === "outside" ? (
          <Dash />
        ) : (
          <div className="flex flex-wrap items-center gap-1.5">
            <SessionStatusBadge status={row.session.status} />
            {row.session.attention_reason ? (
              <SessionAttentionBadge attention={row.session.attention_reason} />
            ) : null}
          </div>
        )}
      </TableCell>
      <TableCell className="max-w-64">
        {row.kind === "ariadne" ? (
          <WorkCell
            session={row.session}
            goalTitle={goalTitle}
            taskTitle={taskTitle}
            onScope={onScope}
          />
        ) : (
          <Dash />
        )}
      </TableCell>
      <TableCell className="max-w-40 truncate text-xs text-muted-foreground">
        {row.kind === "ariadne"
          ? pinLabel(row.session.model, row.session.effort)
          : (row.session.model && pinLabel(row.session.model, row.session.effort)) ||
            row.session.agent_id}
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        {row.session.usage ? <TokenFigure usage={row.session.usage} /> : <Dash />}
      </TableCell>
      <TableCell className="text-right tabular-nums text-muted-foreground">
        <When at={movedAt(row)} format="age" label="last activity" />
      </TableCell>
    </TableRow>
  )
}

/**
 * What an Ariadne session works on, in one cell: the seat it holds, the goal
 * it is under, and the task under that. The seat is a badge of one width, so
 * the goals and tasks of every row line up beside it. Picking the goal or the
 * task narrows the table to its sessions, the way the chips above it do. A
 * loose session holds no seat and has neither.
 */
function WorkCell({
  session,
  goalTitle,
  taskTitle,
  onScope,
}: {
  session: SessionDto
  goalTitle: string | undefined
  taskTitle: string | undefined
  onScope: (param: FilterParam, id: string) => void
}) {
  const { goal_id: goalId, task_id: taskId, seat } = session
  if (!goalId && !taskId && !seat) return <Dash />
  return (
    <div className="flex min-w-0 items-start gap-2 text-xs">
      {seat ? (
        <Badge variant="outline" className="w-22 shrink-0 font-normal text-muted-foreground">
          {SEAT_LABELS[seat]}
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
        size="icon-xs"
        aria-label={`Show sessions for every ${what.toLowerCase()}`}
        onClick={onClear}
        className="size-4 rounded-full text-muted-foreground hover:text-foreground"
      >
        <XIcon />
      </Button>
    </Badge>
  )
}
