/**
 * Every agent session the daemon knows about — the screen equivalent of
 * `ariadne session ls --all`.
 *
 * The list stays live on its own: `session_created` and `session_updated`
 * invalidate `sessions.lists()` in the event dispatcher, so a session starting,
 * going idle or being killed shows up here without a refresh.
 *
 * Filters live in the URL so coming back from a session keeps the view you
 * left, and so a filtered list can be reloaded as-is.
 */

import { useQuery } from "@tanstack/react-query"
import { Link, useSearchParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { paths } from "@/routes/paths"

import {
  byId,
  goalsQueryOptions,
  profilesQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
  tasksQueryOptions,
} from "./queries"
import { reason } from "./session-actions"
import {
  AGENT_KIND_LABELS,
  formatAge,
  formatTimestamp,
  ROLE_LABELS,
  SESSION_STATUSES,
  SessionStatusBadge,
} from "./session-display"
import { useNow } from "./use-now"

/** Select value standing for "no filter"; an empty value is not selectable. */
const ANY = "any"

/** A selection as a filter: the "any" sentinel and no selection both clear it. */
function filterValue(value: string | null): string | undefined {
  return value === null || value === ANY ? undefined : value
}

export function SessionsListPage() {
  const now = useNow()
  const [params, setParams] = useSearchParams()

  const goal = params.get("goal") ?? undefined
  const task = params.get("task") ?? undefined
  // Straight from the URL, so it is only a status if the daemon has one by
  // that name — anything else would come back as a 400.
  const statusParam = params.get("status")
  const status = SESSION_STATUSES.find((value) => value === statusParam)
  const filters: SessionListFilters = { goal, task, status }

  const sessions = useQuery(sessionsQueryOptions(filters))
  const goals = useQuery(goalsQueryOptions())
  const tasks = useQuery(tasksQueryOptions(goal))
  const profiles = useQuery(profilesQueryOptions())
  const profilesById = byId(profiles.data)

  function setFilter(next: Partial<Record<"goal" | "task" | "status", string | undefined>>) {
    const updated = new URLSearchParams(params)
    for (const [key, value] of Object.entries(next)) {
      if (value === undefined) updated.delete(key)
      else updated.set(key, value)
    }
    setParams(updated, { replace: true })
  }

  return (
    <div className="space-y-4">
      <header className="space-y-1">
        <h1 className="font-heading text-xl font-semibold tracking-tight">Sessions</h1>
        <p className="text-sm text-muted-foreground">
          Every agent the daemon has spawned, live and finished. Open one to watch its terminal.
        </p>
      </header>

      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={goal ?? ANY}
          onValueChange={(value) =>
            // A task filter from another goal would only ever match nothing.
            setFilter({ goal: filterValue(value), task: undefined })
          }
        >
          <SelectTrigger size="sm" aria-label="Filter by goal">
            <SelectValue>
              {(value: string) =>
                value === ANY
                  ? "All goals"
                  : (goals.data?.find((item) => item.id === value)?.title ?? value)
              }
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>All goals</SelectItem>
            {(goals.data ?? []).map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={task ?? ANY}
          onValueChange={(value) => setFilter({ task: filterValue(value) })}
        >
          <SelectTrigger size="sm" aria-label="Filter by task">
            <SelectValue>
              {(value: string) =>
                value === ANY
                  ? "All tasks"
                  : (tasks.data?.find((item) => item.id === value)?.title ?? value)
              }
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>All tasks</SelectItem>
            {(tasks.data ?? []).map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.title}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={status ?? ANY}
          onValueChange={(value) => setFilter({ status: filterValue(value) })}
        >
          <SelectTrigger size="sm" aria-label="Filter by status">
            <SelectValue>{(value: string) => (value === ANY ? "Any status" : value)}</SelectValue>
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ANY}>Any status</SelectItem>
            {SESSION_STATUSES.map((value) => (
              <SelectItem key={value} value={value}>
                {value}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        {sessions.data ? (
          <span className="ml-auto text-xs text-muted-foreground tabular-nums">
            {sessions.data.length} session{sessions.data.length === 1 ? "" : "s"}
          </span>
        ) : null}
      </div>

      {sessions.isError ? (
        <Alert variant="destructive">
          <AlertTitle>Could not load sessions</AlertTitle>
          <AlertDescription>{reason(sessions.error)}</AlertDescription>
        </Alert>
      ) : null}

      <div className="rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Session</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>Agent</TableHead>
              <TableHead>Profile</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Round</TableHead>
              <TableHead className="text-right">Last activity</TableHead>
              <TableHead className="text-right">Ended</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.isPending ? <LoadingRows /> : null}
            {sessions.data?.length === 0 ? (
              <TableRow>
                <TableCell colSpan={8} className="py-8 text-center text-sm text-muted-foreground">
                  No sessions match these filters.
                </TableCell>
              </TableRow>
            ) : null}
            {sessions.data?.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                profileName={profilesById.get(session.profile_id)?.name}
                now={now}
              />
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}

function SessionRow({
  session,
  profileName,
  now,
}: {
  session: SessionDto
  profileName: string | undefined
  now: number
}) {
  return (
    <TableRow className="relative">
      <TableCell className="font-mono text-xs">
        {/* Stretched over the whole row, so the row is clickable without a
            `<tr onClick>` that the keyboard could not reach. */}
        <Link
          to={paths.session(session.id)}
          className="after:absolute after:inset-0 hover:underline"
        >
          {session.id}
        </Link>
      </TableCell>
      <TableCell>{ROLE_LABELS[session.role]}</TableCell>
      <TableCell className="text-muted-foreground">
        {AGENT_KIND_LABELS[session.agent_kind]}
      </TableCell>
      <TableCell className="text-muted-foreground">
        {profileName ?? <span className="font-mono text-xs">{session.profile_id}</span>}
      </TableCell>
      <TableCell>
        <SessionStatusBadge status={session.status} />
      </TableCell>
      <TableCell className="text-right tabular-nums text-muted-foreground">
        {session.review_round ?? "—"}
      </TableCell>
      <TableCell
        className="text-right tabular-nums text-muted-foreground"
        title={formatTimestamp(session.last_activity_at)}
      >
        {formatAge(session.last_activity_at, now)}
      </TableCell>
      <TableCell
        className="text-right tabular-nums text-muted-foreground"
        title={formatTimestamp(session.ended_at)}
      >
        {formatAge(session.ended_at, now)}
      </TableCell>
    </TableRow>
  )
}

function LoadingRows() {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row}>
          <TableCell colSpan={8}>
            <Skeleton className="h-5 w-full" />
          </TableCell>
        </TableRow>
      ))}
    </>
  )
}
