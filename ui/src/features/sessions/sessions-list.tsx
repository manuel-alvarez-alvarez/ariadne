/**
 * The sessions table on its own, told what to show and what is selected.
 *
 * Rows select instead of linking anywhere, so the same table serves the
 * session tabs of both the goal and the task panel and the sessions screen — a
 * panel picks a session without leaving the screen it is floating over, and the
 * screen turns the pick into a link of its own.
 *
 * Five columns, because the table has to fit a 48rem panel as well as a full
 * screen without scrolling sideways: the agent kind rides along with the
 * profile, the review round with the role, and the end of the session with its
 * last activity — all three on hover, where they were worth a column each only
 * for the sessions that have them.
 *
 * The filters come from the caller (`{goal}`, `{task}`, whatever the panel
 * has); this component only reads them. The list stays live on its own:
 * `session_created` and `session_updated` invalidate `sessions.lists()` in the
 * event dispatcher, so a session starting, going idle or being killed shows up
 * here without a refresh.
 */

import { useQuery } from "@tanstack/react-query"
import { useSearchParams } from "react-router-dom"

import type { ProfileDto, SessionDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { shortId } from "@/lib/ids"
import { formatAbsolute, formatAge } from "@/lib/time"

import {
  byId,
  profilesQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
} from "./queries"
import { AGENT_KIND_LABELS, ROLE_LABELS, SessionStatusBadge } from "./session-display"
import { useNow } from "./use-now"

const COLUMN_COUNT = 5

export function SessionsList({
  filters,
  selectedId,
  onSelect,
}: {
  filters: SessionListFilters
  /**
   * Id of the row to mark as selected. Defaults to `?session=` — the one param
   * every caller drives a session selection with, panels included — so a list
   * that stays on screen under an open panel marks the row that opened it
   * without being told which one that was.
   */
  selectedId?: string
  /** Called with the whole session, so callers do not have to look it up again. */
  onSelect: (session: SessionDto) => void
}) {
  const [search] = useSearchParams()
  const selected = selectedId ?? search.get("session") ?? undefined
  const now = useNow()
  const sessions = useQuery(sessionsQueryOptions(filters))
  const profiles = useQuery(profilesQueryOptions())
  const profilesById = byId(profiles.data)

  return (
    <div className="space-y-4">
      {sessions.isError ? (
        <ErrorState
          title="Could not load sessions"
          error={sessions.error}
          onRetry={() => void sessions.refetch()}
        />
      ) : null}

      <div className="rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Session</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>Profile</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Last activity</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.isPending ? <LoadingRows /> : null}
            {sessions.data?.length === 0 ? (
              <TableRow>
                <TableCell colSpan={COLUMN_COUNT} className="p-0">
                  {/* Inside the table's own frame, so the empty state drops its box. */}
                  <EmptyState
                    emphasis="quiet"
                    title="No sessions match these filters."
                    className="border-0"
                  />
                </TableCell>
              </TableRow>
            ) : null}
            {sessions.data?.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                profile={profilesById.get(session.profile_id)}
                now={now}
                selected={session.id === selected}
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
  profile,
  now,
  selected,
  onSelect,
}: {
  session: SessionDto
  profile: ProfileDto | undefined
  now: number
  selected: boolean
  onSelect: () => void
}) {
  const agent = AGENT_KIND_LABELS[session.agent_kind]

  return (
    <TableRow className="relative" data-state={selected ? "selected" : undefined}>
      {/* Above the row-wide click target below, so the one thing on the row
          that has its own action keeps it: these ids are read here and typed
          into a terminal (`ariadne attach <session-id>`), and the row is the
          only place the whole list of them is on screen at once. */}
      <TableCell className="relative z-10">
        <CopyableId value={session.id} display={shortId} label="session id" className="text-xs" />
      </TableCell>
      <TableCell>
        {/* Stretched over the whole row, so the row is clickable without a
            `<tr onClick>` that the keyboard could not reach. */}
        <button
          type="button"
          onClick={onSelect}
          className="text-left after:absolute after:inset-0 hover:underline"
        >
          {ROLE_LABELS[session.role]}
        </button>
        {session.review_round != null ? (
          <span className="text-muted-foreground" title="Review round">
            {" "}
            · r{session.review_round}
          </span>
        ) : null}
      </TableCell>
      <TableCell className="text-muted-foreground" title={`${agent} · ${session.profile_id}`}>
        {profile?.name ?? <span className="font-mono text-xs">{shortId(session.profile_id)}</span>}
      </TableCell>
      <TableCell>
        <SessionStatusBadge status={session.status} />
      </TableCell>
      <TableCell
        className="text-right tabular-nums text-muted-foreground"
        title={[
          `Last activity: ${formatAbsolute(session.last_activity_at)}`,
          `Started: ${formatAbsolute(session.created_at)}`,
          `Ended: ${formatAbsolute(session.ended_at)}`,
        ].join("\n")}
      >
        {formatAge(session.last_activity_at, now)}
      </TableCell>
    </TableRow>
  )
}

function LoadingRows() {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row}>
          <TableCell colSpan={COLUMN_COUNT}>
            <Skeleton className="h-5 w-full" />
          </TableCell>
        </TableRow>
      ))}
    </>
  )
}
