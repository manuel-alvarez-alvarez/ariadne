/**
 * The sessions table on its own, told what to show and what is selected.
 *
 * Rows select instead of linking anywhere, so the same table serves the
 * session tabs of both the goal panel and the task panel — a panel picks a
 * session without leaving the panel it is floating over.
 *
 * The session *is* one cell — the seat it ran as, with its id after it on the
 * same line, small and quiet enough to read as the aside it is — and what it
 * spent rides in the hint behind its last activity, beside the two stamps
 * that were already there. This is the panel's own table: the merged listing
 * over every session, of both kinds, is `sessions-page.tsx`'s own table, built
 * for the paging and the columns that screen alone needs.
 *
 * The filters come from the caller (`{goal, seat: "orchestrator"}`, `{task}`)
 * and always scope the list to one goal or one task, which is what decides
 * what an empty list is called: the subject is the panel's own heading, and
 * repeating it on every row says nothing.
 *
 * The list stays live on its own: `session_created` and `session_updated`
 * invalidate `sessions.lists()` in the event dispatcher, so a session starting,
 * going idle or being killed shows up here without a refresh. It is ordered by
 * what moved last (see {@link byLastActivity}), which is what keeps the row
 * that just changed at the top rather than wherever the daemon listed it.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { useSearchParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { ScrollableTable } from "@/components/scroll-edge"
import { TokenHalves } from "@/components/token-figure"
import { Skeleton } from "@/components/ui/skeleton"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { When, WhenDetail } from "@/components/when"
import { ModelPin } from "@/features/models/model-pin"
import { sessionCopyEntries } from "@/lib/clipboard"
import { cn, shortId } from "@/lib/format"

import { type SessionListFilters, sessionsQueryOptions } from "./queries"
import { SessionAttentionBadge, SessionStatusBadge, seatLabel } from "./session-display"

/**
 * The rows in the order the table shows them: whatever moved last, first.
 *
 * The daemon lists sessions oldest-first, which puts the agent that is working
 * right now at the bottom of a long screen — and this table is read for what is
 * happening, not for what a goal's history was. `created_at` stands in for a
 * session that has never reported activity, so the order is total and a session
 * with nothing to say still sits where it belongs.
 */
function byLastActivity(sessions: SessionDto[]): SessionDto[] {
  const movedAt = (session: SessionDto) => session.last_activity_at ?? session.created_at ?? ""
  return [...sessions].sort((a, b) => movedAt(b).localeCompare(movedAt(a)))
}

/**
 * What an empty list is called where it is empty; never claims more than the
 * list is actually showing.
 *
 * A list narrowed to one seat is that seat's list — the goal panel's tab is
 * the orchestrator's sessions alone — so an empty one says the seat is
 * missing rather than that the goal has no sessions, which is a thing it can
 * say while four of them are running.
 */
function emptyTitle(filters: SessionListFilters): string {
  if (filters.seat) return `No ${seatLabel(filters.seat).toLowerCase()} session yet`
  if (filters.task) return "No sessions yet for this task"
  if (filters.goal) return "No sessions yet for this goal"
  return "No sessions yet"
}

export function SessionsList({
  filters,
  selectedId,
  onSelect,
}: {
  filters: SessionListFilters
  /**
   * Id of the row to mark as selected. Defaults to `?session=` — the one param
   * every caller drives a session selection with — so a list that stays on
   * screen under an open panel marks the row that opened it without being
   * told which one that was.
   */
  selectedId?: string
  /** Called with the whole session, so callers do not have to look it up again. */
  onSelect: (session: SessionDto) => void
}) {
  const [search] = useSearchParams()
  const selected = selectedId ?? search.get("session") ?? undefined
  const sessions = useQuery(sessionsQueryOptions(filters))
  const rows = useMemo(() => byLastActivity(sessions.data ?? []), [sessions.data])

  return (
    <div className="space-y-4">
      {sessions.isError ? (
        <ErrorState
          title="Could not load sessions"
          error={sessions.error}
          onRetry={() => void sessions.refetch()}
        />
      ) : null}

      <ScrollableTable className="rounded-lg border">
        <TableHeader>
          <TableRow>
            <TableHead>Session</TableHead>
            <TableHead>Model</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="text-right">Last activity</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sessions.isPending ? <LoadingRows /> : null}
          {sessions.data?.length === 0 ? (
            <TableRow>
              <TableCell colSpan={4} className="p-0">
                {/* Inside the table's own frame, so the empty state drops its box. */}
                <EmptyState emphasis="quiet" title={emptyTitle(filters)} className="border-0" />
              </TableCell>
            </TableRow>
          ) : null}
          {rows.map((session) => (
            <SessionRow
              key={session.id}
              session={session}
              selected={session.id === selected}
              onSelect={() => onSelect(session)}
            />
          ))}
        </TableBody>
      </ScrollableTable>
    </div>
  )
}

function SessionRow({
  session,
  selected,
  onSelect,
}: {
  session: SessionDto
  selected: boolean
  onSelect: () => void
}) {
  return (
    <TableRow
      className="cursor-pointer"
      data-state={selected ? "selected" : undefined}
      // Anywhere on the row picks the session, down to the controls that carry
      // an action of their own: the copy trigger keeps its menu and the seat
      // button below selects by itself. The id *text* is not one of them — it
      // is read here, and clicking it is still a click on the row.
      //
      // The `contains` is what keeps the copy menu whole: React events travel
      // the component tree rather than the DOM, so a click on an entry of a
      // menu portalled out of the table arrives here all the same — and it is
      // a copy, not a pick.
      onClick={(event) => {
        const target = event.target as Element
        if (event.currentTarget.contains(target) && !target.closest("button, a")) onSelect()
      }}
    >
      {/* The seat and the id on the same line, small and quiet enough that
          the seat is still what the cell reads as. */}
      <TableCell>
        <span className="flex items-center gap-2">
          <SessionRole session={session} onSelect={onSelect} />
          <CopyableIdMenu
            value={session.id}
            display={shortId}
            label="session id"
            entries={sessionCopyEntries(session.id)}
            className="text-xs"
          />
        </span>
      </TableCell>
      {/* What the agent runs on, and nothing else: an agent has no name to
          carry it any more, so this column is the model itself — the agent and
          the model of it as one id, with the effort after an `@` where one is
          pinned. The seat is already on the row, so it is not repeated here.
          It is the session's own snapshot: what it was launched on. */}
      <TableCell className="max-w-36 text-xs text-muted-foreground lg:max-w-56">
        <ModelPin model={session.model} effort={session.effort} mode="row" />
      </TableCell>
      {/* The reason rides in the status cell rather than taking a fifth
          column: it is empty for almost every row, and where it is not it is
          the one thing on the row worth reading — a session can be `running`
          and still be waiting on a permission prompt. */}
      <TableCell>
        <div className="flex flex-wrap items-center gap-1.5">
          <SessionStatusBadge status={session.status} />
          {session.attention_reason ? (
            <SessionAttentionBadge attention={session.attention_reason} />
          ) : null}
        </div>
      </TableCell>
      {/* The compact age is the column's text — the heading says what it is
          the age of, and "N minutes ago" down a column is a column of repeated
          words. Everything else about the session's clock is the hint behind
          it, which is where the columns this table has no room for live. */}
      <TableCell className="text-right tabular-nums text-muted-foreground">
        <When
          at={session.last_activity_at}
          format="age"
          label="last activity"
          detail={
            <>
              <WhenDetail label="started" at={session.created_at} />
              <WhenDetail label="ended" at={session.ended_at} />
              <span className="flex items-center gap-2 text-background/70">
                tokens
                <TokenHalves usage={session.usage} />
              </span>
            </>
          }
        />
      </TableCell>
    </TableRow>
  )
}

/**
 * The seat, as the thing that opens the session.
 *
 * The row above takes the pointer clicks; this button is the same action for
 * the keyboard, which cannot reach a `<tr onClick>` — and it is why the row
 * lets buttons through rather than picking twice. Nothing is stretched over the
 * row: `position` on a `<tr>` is undefined per spec, and an overlay that
 * resolves against the table container instead swallows every other row's
 * clicks.
 */
function SessionRole({ session, onSelect }: { session: SessionDto; onSelect: () => void }) {
  const label = seatLabel(session.seat)
  return (
    <button
      type="button"
      onClick={onSelect}
      // The visible word is the seat; what Enter does is open the session,
      // and "Author" on its own says none of that. The id is what the
      // panel it opens is drilled into — see `useFocusReturn`.
      aria-label={`Open ${label} session`}
      data-focus-return={session.id}
      className={cn(
        "rounded-xs text-left outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50",
      )}
    >
      {label}
    </button>
  )
}

function LoadingRows() {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row}>
          <TableCell colSpan={4}>
            <Skeleton className="h-5 w-full" />
          </TableCell>
        </TableRow>
      ))}
    </>
  )
}
