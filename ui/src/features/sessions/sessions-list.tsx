/**
 * The sessions table on its own, told what to show and what is selected.
 *
 * Rows select instead of linking anywhere, so the same table serves the
 * session tabs of both the goal and the task panel and the sessions screen — a
 * panel picks a session without leaving the screen it is floating over, and the
 * screen turns the pick into a link of its own.
 *
 * Five columns in a panel, because the table has to fit a 48rem one as well as
 * a full screen without scrolling sideways: the agent kind rides along with the
 * profile, the review round with the role, and the end of the session with its
 * last activity — all three on hover, where they were worth a column each only
 * for the sessions that have them. The sixth, the context, is the screen's
 * alone.
 *
 * The filters come from the caller (`{goal}`, `{task}`, whatever the panel
 * has); this component only reads them — but it reads them for more than the
 * request. What the list is *scoped* to is what decides the sixth column and
 * what an empty list is called: inside a goal's or a task's panel the subject
 * is the panel's own heading, and repeating it on every row (or blaming an
 * empty tab on filters that tab does not have) says nothing. See
 * {@link listScope}.
 *
 * The list stays live on its own: `session_created` and `session_updated`
 * invalidate `sessions.lists()` in the event dispatcher, so a session starting,
 * going idle or being killed shows up here without a refresh.
 */

import { useQuery } from "@tanstack/react-query"
import { Link, useSearchParams } from "react-router-dom"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { CopyableIdMenu } from "@/components/copyable-id"
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
// Both reached at their `queries` module rather than through the feature's
// barrel: `@/features/tasks` re-exports the task panel, whose sessions tab is
// this very component, and the round trip is an import cycle.
import { goalsQueryOptions } from "@/features/goals/queries"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { sessionCopyEntries } from "@/lib/copy-entries"
import { shortId } from "@/lib/ids"
import { AGENT_KIND_LABELS, ROLE_LABELS } from "@/lib/labels"
import { formatAbsolute, formatAge } from "@/lib/time"
import { paths, taskPanelTo } from "@/routes/paths"

import {
  byId,
  profilesQueryOptions,
  type SessionListFilters,
  sessionsQueryOptions,
} from "./queries"
import { SessionStatusBadge } from "./session-display"
import { useNow } from "./use-now"

/**
 * What the list is already inside of.
 *
 * A panel tab is scoped to its task or its goal. A list scoped to nothing is
 * the only place where two rows can be the same role, the same profile and the
 * same status and still be about different work — which is what the context
 * column is for.
 */
function listScope(filters: SessionListFilters): "task" | "goal" | "unscoped" {
  if (filters.task) return "task"
  if (filters.goal) return "goal"
  return "unscoped"
}

/** What an empty list is called where it is empty; never blames absent filters. */
function emptyTitle(filters: SessionListFilters): string {
  switch (listScope(filters)) {
    case "task":
      return "No sessions yet for this task"
    case "goal":
      return "No sessions yet for this goal"
    default:
      return filters.status || filters.role ? "No sessions match these filters" : "No sessions yet"
  }
}

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

  // Both are shared keys the rest of the app already holds (the goals board's
  // attention strip mounts them), so the sixth column usually costs no request
  // at all — and none whatsoever in a panel, which does not draw it.
  const showContext = listScope(filters) === "unscoped"
  const goals = useQuery({ ...goalsQueryOptions(), enabled: showContext })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: showContext })
  const goalsById = byId(goals.data)
  const tasksById = byId(tasks.data)
  const columnCount = showContext ? 6 : 5

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
              {showContext ? <TableHead>Context</TableHead> : null}
              <TableHead>Role</TableHead>
              <TableHead>Profile</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="text-right">Last activity</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {sessions.isPending ? <LoadingRows columnCount={columnCount} /> : null}
            {sessions.data?.length === 0 ? (
              <TableRow>
                <TableCell colSpan={columnCount} className="p-0">
                  {/* Inside the table's own frame, so the empty state drops its box. */}
                  <EmptyState emphasis="quiet" title={emptyTitle(filters)} className="border-0" />
                </TableCell>
              </TableRow>
            ) : null}
            {sessions.data?.map((session) => (
              <SessionRow
                key={session.id}
                session={session}
                profile={profilesById.get(session.profile_id)}
                goal={goalsById.get(session.goal_id)}
                task={session.task_id ? tasksById.get(session.task_id) : undefined}
                showContext={showContext}
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
  goal,
  task,
  showContext,
  now,
  selected,
  onSelect,
}: {
  session: SessionDto
  profile: ProfileDto | undefined
  /** The goal this session ran for, when the goals list has it. */
  goal: GoalDto | undefined
  /** The task it ran, when there is one and the task list has it. */
  task: TaskDto | undefined
  showContext: boolean
  now: number
  selected: boolean
  onSelect: () => void
}) {
  const agent = AGENT_KIND_LABELS[session.agent_kind]

  return (
    <TableRow
      className="cursor-pointer"
      data-state={selected ? "selected" : undefined}
      // Anywhere on the row picks the session, down to the controls that carry
      // an action of their own: the copy trigger keeps its menu, the role
      // button below selects by itself, and the context link goes to the work
      // the session was run for. The id *text* is not one of them — it is read
      // here, and clicking it is still a click on the row.
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
      {/* These ids are read here on their way into a terminal, and the row is
          the only place the whole list of them is on screen at once, so the
          copy menu is worth the one thing on the row that is not a pick. */}
      <TableCell>
        <CopyableIdMenu
          value={session.id}
          display={shortId}
          label="session id"
          entries={sessionCopyEntries(session.id)}
          className="text-xs"
        />
      </TableCell>
      {showContext ? <ContextCell session={session} goal={goal} task={task} /> : null}
      <TableCell>
        {/* The row above takes the pointer clicks; this button is the same
            action for the keyboard, which cannot reach a `<tr onClick>` — and
            it is why the row lets buttons through rather than picking twice.
            Nothing is stretched over the row: `position` on a `<tr>` is
            undefined per spec, and an overlay that resolves against the table
            container instead swallows every other row's clicks. */}
        <button
          type="button"
          onClick={onSelect}
          // The visible word is the role; what Enter does is open the session,
          // and "Engineer" on its own says none of that. The id is what the
          // panel it opens is drilled into — see `useFocusReturn`.
          aria-label={`Open ${ROLE_LABELS[session.role]} session`}
          data-focus-return={session.id}
          className="rounded-xs text-left outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          {ROLE_LABELS[session.role]}
        </button>
        {session.review_round != null ? (
          <span className="text-muted-foreground" title="Review round">
            {" "}
            · R{session.review_round}
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

/**
 * What this session was run for: its task, or — for a planner session, which
 * has none — its goal.
 *
 * It is a link rather than plain text because the row itself opens the
 * *session*, and the question this column answers ("which piece of work is
 * this?") is usually followed by wanting that piece of work. Both targets are
 * the panel scheme the rest of the app uses; the row lets anchors through for
 * exactly this (see the row's `onClick`).
 */
function ContextCell({
  session,
  goal,
  task,
}: {
  session: SessionDto
  goal: GoalDto | undefined
  task: TaskDto | undefined
}) {
  const [search] = useSearchParams()
  // The row's Role column already says which of the two this is, so both read
  // the same; the tooltip carries the pair in full.
  const subject = session.task_id
    ? { label: task?.title ?? shortId(session.task_id), to: taskPanelTo(search, session.task_id) }
    : { label: goal?.title ?? shortId(session.goal_id), to: paths.goal(session.goal_id) }

  return (
    // `max-w-*` on the cell with a truncating block inside it is what keeps a
    // long title from stretching the table: the cell's own `whitespace-nowrap`
    // would otherwise make the column as wide as the longest goal name.
    <TableCell
      className="max-w-56"
      title={[
        `Goal: ${goal?.title ?? session.goal_id}`,
        `Task: ${session.task_id ? (task?.title ?? session.task_id) : "— (planner session)"}`,
      ].join("\n")}
    >
      <Link
        to={subject.to}
        className="block truncate rounded-xs underline-offset-3 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        {subject.label}
      </Link>
    </TableCell>
  )
}

function LoadingRows({ columnCount }: { columnCount: number }) {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row}>
          <TableCell colSpan={columnCount}>
            <Skeleton className="h-5 w-full" />
          </TableCell>
        </TableRow>
      ))}
    </>
  )
}
