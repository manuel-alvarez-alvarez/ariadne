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
import { ChevronDownIcon, PlusIcon, RefreshCwIcon, XIcon } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"
import { toast } from "sonner"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { DataTable } from "@/components/data-table"
import { PageHeader } from "@/components/page-header"
import { TokenFigure } from "@/components/token-figure"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { acpAgentsQueryOptions } from "@/features/agents/queries"
import { goalsQueryOptions } from "@/features/goals/queries"
import { parseModelRef, pinLabel } from "@/features/models/model-ref"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { describeError, SEAT_LABELS, shortId } from "@/lib/format"
import { paths, sessionPanelFrom, sessionTerminalFrom } from "@/routes/paths"

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
import { KIND_LABELS, type OutsideFilterParam, useOutsideSessionFilters } from "./outside-filters"
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

/**
 * How long a typed filter waits for the next keystroke before the daemon is
 * asked. Both text fields wait: a path is typed as slowly as a search, and
 * neither is worth a request per character.
 */
const TYPING_SETTLES_MS = 250

export function SessionsPage() {
  const [search] = useSearchParams()
  const navigate = useNavigate()
  const { status, seat, goal, task, filterBy } = useSessionFilters()
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
          <>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by kind"
                    className="w-32 justify-between font-normal"
                  />
                }
              >
                {browse.kind ? KIND_LABELS[browse.kind] : "All kinds"}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-32">
                <DropdownMenuRadioGroup
                  value={browse.values.kind || ALL}
                  onValueChange={(value) => browse.filterBy("kind", value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All kinds</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuRadioItem value="ariadne">
                    {KIND_LABELS.ariadne}
                  </DropdownMenuRadioItem>
                  <DropdownMenuRadioItem value="outside">
                    {KIND_LABELS.outside}
                  </DropdownMenuRadioItem>
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>

            <AgentFilter
              value={browse.values.agent}
              onSelect={(agent) => browse.filterBy("agent", agent)}
            />

            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by status"
                    className="w-40 justify-between font-normal"
                  />
                }
              >
                {statusLabel(status)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuRadioGroup
                  value={status ?? ALL}
                  onValueChange={(value) => filterBy(STATUS_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All statuses</DropdownMenuRadioItem>
                  <DropdownMenuRadioItem value={LIVE}>Live</DropdownMenuRadioItem>
                  <DropdownMenuRadioItem value={ATTENTION}>Needs attention</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {STATUSES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {SESSION_STATUS_META[known].label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>

            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by seat"
                    className="w-36 justify-between font-normal"
                  />
                }
              >
                {roleLabel(seat)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-36">
                <DropdownMenuRadioGroup
                  value={seat ?? ALL}
                  onValueChange={(value) => filterBy(ROLE_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All roles</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {ROLES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {SEAT_LABELS[known]}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button onClick={() => setStarting(true)}>
              <PlusIcon />
              New session
            </Button>
          </>
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

      <div className="flex flex-wrap items-center gap-2">
        <TypedFilter
          label="Working directory"
          placeholder="/Users/me/dev"
          value={browse.values.dir}
          onSettle={browse.filterBy}
          param="dir"
          className="w-56 font-mono text-xs"
        />
        <Input
          type="date"
          aria-label="Active since"
          className="w-40"
          value={browse.values.since}
          onChange={(event) => browse.filterBy("since", event.target.value)}
        />
        <Input
          type="date"
          aria-label="Active until"
          className="w-40"
          value={browse.values.until}
          onChange={(event) => browse.filterBy("until", event.target.value)}
        />
        <TypedFilter
          label="Search titles"
          placeholder="Search titles"
          value={browse.values.q}
          onSettle={browse.filterBy}
          param="q"
          className="w-64"
        />
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

      <ScopeChips goal={goal} task={task} onClear={filterBy} />

      {outsideEligible ? (
        <p className="text-sm text-muted-foreground">{`${rows.length} of ${total}`}</p>
      ) : null}

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
          { header: "Goal", className: "min-w-32" },
          { header: "Task", className: "min-w-32" },
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
}: {
  row: Row
  title: string
  goalTitle: string | undefined
  taskTitle: string | undefined
  resuming: boolean
  onSelect: () => void
}) {
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
        {/* An outside row has no goal or task to place it — the working
            directory is what says where it ran instead, so it rides under
            the title rather than taking a column of its own the other kind
            never fills. */}
        {row.kind === "outside" ? (
          <span
            className="block truncate font-mono text-xs text-muted-foreground"
            title={row.session.working_directory}
          >
            {row.session.working_directory}
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
      <TableCell className="max-w-36 truncate text-xs text-muted-foreground">
        {row.kind === "ariadne" && row.session.goal_id ? (
          (goalTitle ?? shortId(row.session.goal_id))
        ) : (
          <Dash />
        )}
      </TableCell>
      <TableCell className="max-w-36 truncate text-xs text-muted-foreground">
        {row.kind === "ariadne" && row.session.task_id ? (
          (taskTitle ?? shortId(row.session.task_id))
        ) : (
          <Dash />
        )}
      </TableCell>
      <TableCell className="max-w-40 truncate text-xs text-muted-foreground">
        {row.kind === "ariadne"
          ? pinLabel(row.session.model, row.session.effort)
          : row.session.agent_id}
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        {row.kind === "ariadne" ? <TokenFigure usage={row.session.usage} /> : <Dash />}
      </TableCell>
      <TableCell className="text-right tabular-nums text-muted-foreground">
        <When at={movedAt(row)} format="age" label="last activity" />
      </TableCell>
    </TableRow>
  )
}

function Dash() {
  return <span>—</span>
}

/**
 * Which registry agent to show sessions of, out of the ACP registry
 * (`GET /v1/acp-agents`). An agent id is the whole of the choice: it is what
 * tells one ACP agent from another everywhere else on this screen.
 */
function AgentFilter({ value, onSelect }: { value: string; onSelect: (value: string) => void }) {
  const agents = useQuery(acpAgentsQueryOptions())

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="outline"
            aria-label="Filter by agent"
            className="w-48 justify-between font-normal"
          />
        }
      >
        {value || "All agents"}
        <ChevronDownIcon className="text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-48">
        <DropdownMenuRadioGroup value={value || ALL} onValueChange={onSelect}>
          <DropdownMenuRadioItem value={ALL}>All agents</DropdownMenuRadioItem>
          <DropdownMenuSeparator />
          {(agents.data ?? []).map((agent) => (
            <DropdownMenuRadioItem key={agent.id} value={agent.id}>
              {agent.id}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * A filter that is typed: the field shows every keystroke, and the daemon is
 * asked once the typing has settled (see {@link TYPING_SETTLES_MS}).
 */
function TypedFilter({
  label,
  placeholder,
  value,
  param,
  onSettle,
  className,
}: {
  label: string
  placeholder: string
  /** What the URL carries, which is what the field opens on. */
  value: string
  param: OutsideFilterParam
  onSettle: (param: OutsideFilterParam, value: string) => void
  className?: string
}) {
  const [text, setText] = useState(value)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const settle = useRef(onSettle)
  settle.current = onSettle

  useEffect(() => () => clearTimeout(timer.current ?? undefined), [])

  return (
    <Input
      value={text}
      aria-label={label}
      placeholder={placeholder}
      autoComplete="off"
      className={className}
      onChange={(event) => {
        const next = event.target.value
        setText(next)
        clearTimeout(timer.current ?? undefined)
        timer.current = setTimeout(() => settle.current(param, next), TYPING_SETTLES_MS)
      }}
    />
  )
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
