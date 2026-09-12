/**
 * The stored sessions of every ACP agent that can list them, none of them
 * started by Ariadne. The user can make one the author of a ready task here, which is
 * the desktop equivalent of `ariadne session discover` and `ariadne session
 * adopt`.
 *
 * The daemon answers one filtered page of its own snapshot, so the screen is a
 * filter bar over an infinite query: every filter is a URL param and a query
 * parameter (`outside-filters.ts`), the table grows by a page under Load more,
 * and the count line says how much of the total is on screen. Refresh is the
 * one control that is not a filter — it asks every agent again before the
 * daemon answers.
 */

import { useInfiniteQuery, useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, RefreshCwIcon } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"

import type { OutsideSessionDto, TaskDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { DataTable } from "@/components/data-table"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
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
import { parseModelRef } from "@/features/models/model-ref"
import { taskAuthor } from "@/features/tasks/agents"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { cn } from "@/lib/format"

import { ALL } from "./filters"
import { type OutsideFilterParam, useOutsideSessionFilters } from "./outside-filters"
import { outsideSessionsQueryOptions, useAdoptOutsideSession } from "./queries"

/**
 * How long a typed filter waits for the next keystroke before the daemon is
 * asked. Both text fields wait: a path is typed as slowly as a search, and
 * neither is worth a request per character.
 */
const TYPING_SETTLES_MS = 250

/**
 * The agent cell and the dialog's wording name the agent by its registry id,
 * which is what tells one ACP agent from another.
 */
function sessionAgentLabel(session: OutsideSessionDto): string {
  return session.agent_id
}

export function OutsideSessionsPage() {
  const { values, filters, filterBy } = useOutsideSessionFilters()
  // Spent on the next request the query makes, which on a refetch is the first
  // page: Refresh asks the agents again, and the pages behind it follow from
  // the snapshot that answer took.
  const refreshWanted = useRef(false)
  const sessions = useInfiniteQuery(
    outsideSessionsQueryOptions(filters, () => {
      const wanted = refreshWanted.current
      refreshWanted.current = false
      return wanted
    }),
  )
  const [selected, setSelected] = useState<OutsideSessionDto | null>(null)

  const pages = sessions.data?.pages
  const rows = useMemo(() => (pages ?? []).flatMap((page) => page.sessions), [pages])
  // From the page that answered last, which is the freshest count of what the
  // filters leave.
  const total = pages?.at(-1)?.total ?? rows.length

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Outside sessions"
        description="Conversations stored by an ACP agent that Ariadne can continue as a task author."
      />

      <div className="flex flex-wrap items-center gap-2">
        <AgentFilter value={values.agent} onSelect={(agent) => filterBy("agent", agent)} />
        <TypedFilter
          label="Working directory"
          placeholder="/Users/me/dev"
          value={values.dir}
          onSettle={filterBy}
          param="dir"
          className="w-56 font-mono text-xs"
        />
        <Input
          type="date"
          aria-label="Active since"
          className="w-40"
          value={values.since}
          onChange={(event) => filterBy("since", event.target.value)}
        />
        <Input
          type="date"
          aria-label="Active until"
          className="w-40"
          value={values.until}
          onChange={(event) => filterBy("until", event.target.value)}
        />
        <TypedFilter
          label="Search first prompts"
          placeholder="Search first prompts"
          value={values.q}
          onSettle={filterBy}
          param="q"
          className="w-64"
        />
        <Button
          variant="outline"
          // Busy while the first page is in flight, its own refetch included:
          // a refetch asked for over a request that is already running is the
          // running one, which carries no `refresh`, and the flag this button
          // set would then be spent on whatever asked next. A page loading
          // underneath is Load more's spinner rather than this one's.
          pending={sessions.isFetching && !sessions.isFetchingNextPage}
          onClick={() => {
            refreshWanted.current = true
            void sessions.refetch()
          }}
        >
          <RefreshCwIcon />
          Refresh
        </Button>
      </div>

      {pages ? (
        <p className="text-sm text-muted-foreground">{`${rows.length} of ${total}`}</p>
      ) : null}

      <DataTable
        query={{
          data: pages ? rows : undefined,
          isPending: sessions.isPending,
          isError: sessions.isError,
          error: sessions.error,
          refetch: () => void sessions.refetch(),
        }}
        errorTitle="Could not load outside sessions"
        columns={[
          { header: "Agent" },
          { header: "Working directory", className: "min-w-56" },
          { header: "Last activity", className: "text-right" },
          { header: "First prompt", className: "min-w-72" },
          {},
        ]}
        empty={
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">
            No outside sessions found.
          </p>
        }
        rowKey={(session) => `${session.agent_id}:${session.internal_session_id}`}
        renderRow={(session) => (
          <TableRow>
            <TableCell>{sessionAgentLabel(session)}</TableCell>
            <TableCell
              className="max-w-sm truncate font-mono text-xs"
              title={session.working_directory}
            >
              {session.working_directory}
            </TableCell>
            <TableCell className="text-right">
              <When at={session.last_activity_at} format="age" label="last activity" />
            </TableCell>
            <TableCell className="max-w-xl truncate" title={session.first_prompt}>
              {session.first_prompt}
            </TableCell>
            <TableCell className="text-right">
              <Button
                variant="outline"
                size="sm"
                aria-label={`Adopt ${sessionAgentLabel(session)} session`}
                onClick={() => setSelected(session)}
              >
                Adopt
              </Button>
            </TableCell>
          </TableRow>
        )}
      />
      {sessions.hasNextPage ? (
        <Button
          variant="outline"
          className="self-center"
          pending={sessions.isFetchingNextPage}
          onClick={() => void sessions.fetchNextPage()}
        >
          Load more
        </Button>
      ) : null}
      <UnavailableAcpAgents />
      <AdoptOutsideSessionDialog session={selected} onClose={() => setSelected(null)} />
    </div>
  )
}

/**
 * Which ACP agent's stored sessions to show, out of the registry the table
 * below already reads (`GET /v1/acp-agents`), so the menu costs no request of
 * its own. An agent id is the whole of the choice: it is what tells one ACP
 * agent from another everywhere else on this screen.
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
 *
 * The URL is written on the same beat, so a narrowed screen is still the link
 * it says it is — one entry per settled word rather than one per character.
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
  // The handler of the beat it fires on rather than the one the keystroke saw:
  // it applies its filter over the params as they are then, and the field
  // beside this one may have settled in between.
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
 * Why an ACP agent's sessions are missing from the table above: rejected
 * outright, or ready but without the `session_list` capability. Quiet where
 * every registered agent can list its own — which is also the case for
 * nobody having configured one at all.
 */
function UnavailableAcpAgents() {
  const agents = useQuery(acpAgentsQueryOptions())
  const unavailable = (agents.data ?? []).filter((agent) => !agent.capabilities.session_list)
  if (unavailable.length === 0) return null

  return (
    <div className="rounded-md border border-dashed p-3 text-sm text-muted-foreground">
      <p className="font-medium text-foreground">Adoption unavailable for some ACP agents</p>
      <ul className="mt-1 list-inside list-disc">
        {unavailable.map((agent) => (
          <li key={agent.id}>
            <span className="font-mono text-xs">{agent.id}</span>:{" "}
            {agent.rejection_reason ?? "the agent does not support listing sessions"}
          </li>
        ))}
      </ul>
    </div>
  )
}

function AdoptOutsideSessionDialog({
  session,
  onClose,
}: {
  session: OutsideSessionDto | null
  onClose: () => void
}) {
  const tasks = useQuery({
    ...taskListQueryOptions({ status: "ready" }),
    enabled: session !== null,
  })
  const adopt = useAdoptOutsideSession()
  const [taskId, setTaskId] = useState<string | null>(null)
  const readyTasks = useMemo(
    () => (session ? (tasks.data ?? []).filter((task) => taskUsesAgent(task, session)) : []),
    [session, tasks.data],
  )

  function close() {
    setTaskId(null)
    adopt.reset()
    onClose()
  }

  if (!session) return null
  const selectedTask = readyTasks.find((task) => task.id === taskId)
  const label = sessionAgentLabel(session)

  return (
    <ConfirmDialog
      open
      onClose={close}
      title="Adopt this session?"
      description={`Ariadne will continue this ${label} conversation as the task author.`}
      confirmLabel="Adopt session"
      confirmDisabled={!selectedTask}
      pending={adopt.isPending}
      error={adopt.error}
      errorTitle="Could not adopt session"
      onConfirm={() => {
        if (!selectedTask) return
        adopt.mutate(
          { taskId: selectedTask.id, ...session },
          {
            onSuccess: () => {
              toast.success("Session adopted", { description: selectedTask.title })
              close()
            },
          },
        )
      }}
    >
      {tasks.isError ? (
        <ErrorState
          title="Could not load ready tasks"
          error={tasks.error}
          onRetry={() => void tasks.refetch()}
        />
      ) : tasks.isPending ? (
        <p className="text-sm text-muted-foreground">Loading ready tasks…</p>
      ) : readyTasks.length === 0 ? (
        <p className="text-sm text-muted-foreground">No ready task uses {label} as its author.</p>
      ) : (
        <div className="grid gap-2">
          <p className="text-sm font-medium">Choose a ready task</p>
          {readyTasks.map((task) => (
            <Button
              key={task.id}
              type="button"
              variant="outline"
              aria-pressed={task.id === taskId}
              className={cn("justify-start", task.id === taskId && "border-primary")}
              onClick={() => setTaskId(task.id)}
            >
              Use {task.title}
            </Button>
          ))}
        </div>
      )}
    </ConfirmDialog>
  )
}

/**
 * Whether a ready task's author runs the same agent this session belongs to:
 * the registry id before the first `:` of its pin.
 */
function taskUsesAgent(task: TaskDto, session: OutsideSessionDto): boolean {
  return parseModelRef(taskAuthor(task)?.model ?? "")?.agent === session.agent_id
}
