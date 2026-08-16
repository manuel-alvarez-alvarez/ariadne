/**
 * The sessions table on its own, told what to show and what is selected.
 *
 * Rows select instead of linking anywhere, so the same table serves the
 * sessions screen and the session tabs inside the goal and task panels — a
 * panel picks a session without leaving the screen it is floating over.
 *
 * The filters come from the caller (`{goal}`, `{task}`, whatever the screen
 * has); this component only reads them. The list stays live on its own:
 * `session_created` and `session_updated` invalidate `sessions.lists()` in the
 * event dispatcher, so a session starting, going idle or being killed shows up
 * here without a refresh.
 */

import { useQuery } from "@tanstack/react-query"

import type { SessionDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

import {
  byId,
  profilesQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
} from "./queries"
import { reason } from "./session-actions"
import {
  AGENT_KIND_LABELS,
  formatAge,
  formatTimestamp,
  ROLE_LABELS,
  SessionStatusBadge,
} from "./session-display"
import { useNow } from "./use-now"

export function SessionsList({
  filters,
  selectedId,
  onSelect,
}: {
  filters: SessionListFilters
  /** Id of the row to mark as selected, if any. */
  selectedId?: string
  /** Called with the whole session, so callers do not have to look it up again. */
  onSelect: (session: SessionDto) => void
}) {
  const now = useNow()
  const sessions = useQuery(sessionsQueryOptions(filters))
  const profiles = useQuery(profilesQueryOptions())
  const profilesById = byId(profiles.data)

  return (
    <div className="space-y-4">
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
                selected={session.id === selectedId}
                onSelect={() => onSelect(session)}
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
  selected,
  onSelect,
}: {
  session: SessionDto
  profileName: string | undefined
  now: number
  selected: boolean
  onSelect: () => void
}) {
  return (
    <TableRow className="relative" data-state={selected ? "selected" : undefined}>
      <TableCell className="font-mono text-xs">
        {/* Stretched over the whole row, so the row is clickable without a
            `<tr onClick>` that the keyboard could not reach. */}
        <button
          type="button"
          onClick={onSelect}
          className="text-left after:absolute after:inset-0 hover:underline"
        >
          {session.id}
        </button>
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
