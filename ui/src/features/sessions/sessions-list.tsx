/**
 * The sessions table on its own, told what to show and what is selected.
 *
 * Rows select instead of linking anywhere, so the same table serves the
 * session tabs of both the goal and the task panel and the sessions screen — a
 * panel picks a session without leaving the screen it is floating over, and the
 * screen turns the pick into a link of its own.
 *
 * Four columns in a panel and seven on the screen, out of the same rows: a
 * 48rem panel does not hold what a window holds, and the panel's table was
 * cutting its last heading to "Las…" at every width. So in a panel the session
 * *is* one cell — the role it ran as, with its id after it on the same line,
 * small and quiet enough to read as the aside it is — and what it spent rides
 * in the hint behind its last activity, beside the two stamps that were
 * already there. What the agent runs on rides along with the profile and the
 * review round with the role, in either variant, and the end of the session
 * with its last activity: all three were worth a column only for the sessions
 * that have them.
 *
 * The screen keeps a column each where a window is wide enough, and folds two
 * of them away below `lg`: the id, which says nothing about the work a row is
 * about, and the figure, which the hint carries anyway — what is left is what
 * the row is read for, and it fits. The tokens cell is the whole figure, both
 * counts and the cached share of the input: the share is a property of the
 * input rather than a column of its own, and it is the number that says
 * whether a figure that looks expensive was. Only the exact counts are behind
 * its hint. The context column is the screen's alone.
 *
 * Wider than its frame either way, the table says so: the fade at its edge is
 * the only sign there is, since macOS draws no scrollbar until something moves
 * (see {@link ScrollableTable}).
 *
 * The filters come from the caller (`{goal}`, `{task}`, whatever the panel
 * has); this component only reads them — but it reads them for more than the
 * request. What the list is *scoped* to is what decides the context column and
 * what an empty list is called: inside a goal's or a task's panel the subject
 * is the panel's own heading, and repeating it on every row (or blaming an
 * empty tab on filters that tab does not have) says nothing. The sessions
 * screen narrows itself by the same two filters from outside, which is what
 * `inside` is for. See {@link listScope}.
 *
 * The list stays live on its own: `session_created` and `session_updated`
 * invalidate `sessions.lists()` in the event dispatcher, so a session starting,
 * going idle or being killed shows up here without a refresh. It is ordered by
 * what moved last (see {@link byLastActivity}), which is what keeps the row
 * that just changed at the top rather than wherever the daemon listed it.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { ScrollableTable } from "@/components/scroll-edge"
import { TokenFigure, TokenHalves } from "@/components/token-figure"
import { Skeleton } from "@/components/ui/skeleton"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When, WhenDetail } from "@/components/when"
// Both reached at their `queries` module rather than through the feature's
// barrel: `@/features/tasks` re-exports the task panel, whose sessions tab is
// this very component, and the round trip is an import cycle.
import { goalsQueryOptions } from "@/features/goals/queries"
import { formatModelRef } from "@/features/profiles/model-ref"
import { ProfileSummary } from "@/features/profiles/profile-summary"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { sessionCopyEntries } from "@/lib/clipboard"
import { cn, ROLE_LABELS, shortId } from "@/lib/format"

import { GOAL_PARAM, TASK_PARAM, withFilter } from "./filters"
import { byId, type SessionListFilters, sessionsQueryOptions } from "./queries"
import { SessionAttentionBadge, SessionStatusBadge } from "./session-display"

/**
 * What the list is already inside of.
 *
 * A panel tab is scoped to its task or its goal. A list scoped to nothing is
 * the only place where two rows can be the same role, the same profile and the
 * same status and still be about different work — which is what the context
 * column is for.
 */
function listScope(filters: SessionListFilters, inside: boolean): "task" | "goal" | "unscoped" {
  if (!inside) return "unscoped"
  if (filters.task) return "task"
  if (filters.goal) return "goal"
  return "unscoped"
}

/**
 * The two cells the screen gives up below `lg`, where seven columns do not fit
 * a window that narrow: the id, which identifies nothing a reader is looking
 * for, and the figure, which is the one cell that is also reachable from the
 * hint behind the row's last activity. What is left — what the session was run
 * for, its role, its profile, its status and when it last moved — fits.
 */
const FOLDS_AWAY = "hidden lg:table-cell"

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
  const movedAt = (session: SessionDto) => session.last_activity_at ?? session.created_at
  return [...sessions].sort((a, b) => movedAt(b).localeCompare(movedAt(a)))
}

/**
 * What an empty list is called where it is empty; never blames absent filters,
 * and never claims more than the list is actually showing.
 *
 * A scoped list narrowed to one role is that role's list — the goal panel's
 * tab is the planner's sessions alone — so an empty one says the role is
 * missing rather than that the goal has no sessions, which is a thing it can
 * say while four of them are running.
 */
function emptyTitle(filters: SessionListFilters, inside: boolean): string {
  const scope = listScope(filters, inside)
  if (filters.role && scope !== "unscoped") {
    return `No ${ROLE_LABELS[filters.role].toLowerCase()} session yet`
  }
  switch (scope) {
    case "task":
      return "No sessions yet for this task"
    case "goal":
      return "No sessions yet for this goal"
    default:
      return filters.status ||
        filters.role ||
        filters.live ||
        filters.attention ||
        filters.goal ||
        filters.task
        ? "No sessions match these filters"
        : "No sessions yet"
  }
}

export function SessionsList({
  filters,
  selectedId,
  inside = true,
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
  /**
   * Whether the table is *inside* the thing it is scoped to — a goal's or a
   * task's panel, whose heading already names it, which is the default and
   * what every panel is.
   *
   * The sessions screen narrows itself by the same filters but from a chip
   * above the table, so it says `false`: the goal is named once up there, which
   * leaves the Context column worth having (which task each row ran) and makes
   * an empty list the filters' doing rather than the subject's.
   */
  inside?: boolean
  /** Called with the whole session, so callers do not have to look it up again. */
  onSelect: (session: SessionDto) => void
}) {
  const [search] = useSearchParams()
  const selected = selectedId ?? search.get("session") ?? undefined
  const sessions = useQuery(sessionsQueryOptions(filters))
  const rows = useMemo(() => byLastActivity(sessions.data ?? []), [sessions.data])

  // Both are shared keys the rest of the app already holds (the goals board's
  // attention strip mounts them), so the context column usually costs no
  // request at all — and none whatsoever in a panel, which does not draw it.
  const showContext = listScope(filters, inside) === "unscoped"
  const goals = useQuery({ ...goalsQueryOptions(), enabled: showContext })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: showContext })
  const goalsById = byId(goals.data)
  const tasksById = byId(tasks.data)
  const columnCount = showContext ? 7 : 4

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
            {/* In a panel this heading covers the whole cell under it, role
                and id both; on the screen it is the id alone, which is what
                lets it go away below `lg`. */}
            <TableHead className={cn(showContext && FOLDS_AWAY)}>Session</TableHead>
            {showContext ? <TableHead>Context</TableHead> : null}
            {showContext ? <TableHead>Role</TableHead> : null}
            <TableHead>Profile</TableHead>
            <TableHead>Status</TableHead>
            {showContext ? (
              <TableHead className={cn("text-right", FOLDS_AWAY)}>Tokens</TableHead>
            ) : null}
            <TableHead className="text-right">Last activity</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sessions.isPending ? <LoadingRows columnCount={columnCount} /> : null}
          {sessions.data?.length === 0 ? (
            <TableRow>
              <TableCell colSpan={columnCount} className="p-0">
                {/* Inside the table's own frame, so the empty state drops its box. */}
                <EmptyState
                  emphasis="quiet"
                  title={emptyTitle(filters, inside)}
                  className="border-0"
                />
              </TableCell>
            </TableRow>
          ) : null}
          {rows.map((session) => (
            <SessionRow
              key={session.id}
              session={session}
              goal={goalsById.get(session.goal_id)}
              task={session.task_id ? tasksById.get(session.task_id) : undefined}
              showContext={showContext}
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
  goal,
  task,
  showContext,
  selected,
  onSelect,
}: {
  session: SessionDto
  /** The goal this session ran for, when the goals list has it. */
  goal: GoalDto | undefined
  /** The task it ran, when there is one and the task list has it. */
  task: TaskDto | undefined
  showContext: boolean
  selected: boolean
  onSelect: () => void
}) {
  const sessionId = (
    <CopyableIdMenu
      value={session.id}
      display={shortId}
      label="session id"
      entries={sessionCopyEntries(session.id)}
      className={cn("text-xs", !showContext && "text-muted-foreground")}
    />
  )

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
          copy menu is worth the one thing on the row that is not a pick.
          In a panel it follows the role on the same line, small and quiet
          enough that the role is still what the cell reads as; on the screen
          it has a cell of its own, and gives it up below `lg`. */}
      <TableCell className={cn(showContext && FOLDS_AWAY)}>
        {showContext ? (
          sessionId
        ) : (
          <span className="flex items-center gap-2">
            <SessionRole session={session} onSelect={onSelect} />
            {sessionId}
          </span>
        )}
      </TableCell>
      {showContext ? <ContextCell session={session} goal={goal} task={task} /> : null}
      {showContext ? (
        <TableCell>
          <SessionRole session={session} onSelect={onSelect} />
        </TableCell>
      ) : null}
      {/* What the agent runs on rides along with the name: which CLI and which
          model of it is what the column is read for, and it was a `title=`
          nobody hovers. A session keeps the two apart, so the id the badge
          takes is composed here. Narrower below `lg`, where every pixel this
          column does not take is one the status and the figure after it get. */}
      <TableCell className="max-w-36 text-xs lg:max-w-56">
        <ProfileSummary
          profileId={session.profile_id}
          model={formatModelRef(session.agent_kind, session.model)}
        />
      </TableCell>
      {/* The reason rides in the status cell rather than taking a seventh
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
      {/* What this agent has spent, compact and right-aligned so the column
          reads down: both counts and the share of the input the cache served,
          with the exact counts in the hint behind them. A panel never has room
          for a column of its own, and a narrow window stops having one — both
          read it off the hint below instead. */}
      {showContext ? (
        <TableCell className={cn("text-right text-xs text-muted-foreground", FOLDS_AWAY)}>
          <TokenFigure usage={session.usage} />
        </TableCell>
      ) : null}
      {/* The compact age is the column's text — the heading says what it is
          the age of, and "N minutes ago" down a column is a column of repeated
          words. Everything else about the session's clock is the hint behind
          it, which is where the columns this table has no room for live. The
          figure is one of them wherever it is folded away, and it rides here
          in every variant rather than appearing and disappearing with a
          breakpoint: it is the plain pair, not a figure of its own, since a
          tooltip inside a tooltip is not a thing. */}
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
 * The role, as the thing that opens the session.
 *
 * The row above takes the pointer clicks; this button is the same action for
 * the keyboard, which cannot reach a `<tr onClick>` — and it is why the row
 * lets buttons through rather than picking twice. Nothing is stretched over the
 * row: `position` on a `<tr>` is undefined per spec, and an overlay that
 * resolves against the table container instead swallows every other row's
 * clicks.
 *
 * The plain span around the pair is what keeps the round with the role: in a
 * panel the two sit inside the cell's flex row, where a round of its own would
 * be an item of its own and lose the space in front of it.
 */
function SessionRole({ session, onSelect }: { session: SessionDto; onSelect: () => void }) {
  return (
    <span>
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
        <Tooltip>
          <TooltipTrigger render={<span className="text-muted-foreground" />}>
            {" "}
            · R{session.review_round}
          </TooltipTrigger>
          <TooltipContent>Review round {session.review_round}</TooltipContent>
        </Tooltip>
      ) : null}
    </span>
  )
}

/**
 * What this session was run for: its task, or — for a planner session, which
 * has none — its goal.
 *
 * It is a link rather than plain text, and what it links to is *this list,
 * narrowed to that piece of work* — `?task=` or `?goal=`, the screen's own
 * scope params, which come back as a chip above the table
 * (`sessions-page.tsx`). The column is only ever drawn where nothing scopes the
 * list already, so following it is the natural next question: "and what else
 * has run for this?" The work itself is one step further on, through the panel
 * a row opens, which names its goal and its task as links of their own. The row
 * lets anchors through for exactly this (see the row's `onClick`).
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
    ? {
        what: "task",
        label: task?.title ?? shortId(session.task_id),
        to: { search: `?${withFilter(search, TASK_PARAM, session.task_id)}` },
      }
    : {
        what: "goal",
        label: goal?.title ?? shortId(session.goal_id),
        to: { search: `?${withFilter(search, GOAL_PARAM, session.goal_id)}` },
      }

  return (
    // `max-w-*` on the cell with a truncating block inside it is what keeps a
    // long title from stretching the table: the cell's own `whitespace-nowrap`
    // would otherwise make the column as wide as the longest goal name. It is
    // capped harder below `lg`, where the columns after it are the ones being
    // pushed off the edge.
    <TableCell className="max-w-36 lg:max-w-56">
      <Tooltip>
        {/* The link is the trigger: it already takes focus, so the pair is in
            reach of a keyboard without a stop of its own. */}
        <TooltipTrigger
          render={
            <Link
              to={subject.to}
              className="block truncate rounded-xs underline-offset-3 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
            />
          }
        >
          {subject.label}
        </TooltipTrigger>
        <TooltipContent className="flex-col items-start gap-0.5">
          <span>Goal: {goal?.title ?? session.goal_id}</span>
          <span>
            Task: {session.task_id ? (task?.title ?? session.task_id) : "— (planner session)"}
          </span>
          {/* What the link does, since the name alone cannot say it — and the
              name is what the link has to keep being called. */}
          <span className="text-background/70">Opens only this {subject.what}'s sessions</span>
        </TooltipContent>
      </Tooltip>
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
