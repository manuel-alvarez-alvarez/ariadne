/**
 * One agent session laid out in full: what it is, what it is printing, and
 * what it reported. No page chrome, so it renders the same inside the goal
 * panel and the task panel.
 *
 * The metadata comes from the query cache, which the event dispatcher keeps
 * current — a session going idle or being killed elsewhere updates this view
 * without a refetch. The terminal is the exception: it is a byte stream, not
 * cacheable state, and owns its own connection (see `log-stream.ts`).
 */

import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"

import type { SessionDto } from "@/api"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { paths, useTaskPanelTo } from "@/routes/paths"

import { goalQueryOptions, profilesQueryOptions, taskQueryOptions } from "./queries"
import { SessionActions } from "./session-actions"
import { SessionActivity } from "./session-activity"
import {
  AGENT_KIND_LABELS,
  formatAge,
  formatTimestamp,
  ROLE_LABELS,
  SessionStatusBadge,
} from "./session-display"
import { SessionTerminal } from "./session-terminal"
import { useNow } from "./use-now"

export function SessionDetailView({
  session,
  context,
  onResumed,
  terminalClassName,
}: {
  session: SessionDto
  /**
   * What the view is embedded in. The link back to that goal or task is
   * dropped, since inside its own panel it would only point at itself.
   */
  context?: "goal" | "task"
  /** Where to go once a resume hands the session back; see {@link SessionActions}. */
  onResumed?: (session: SessionDto) => void
  /** Height of the terminal box, for embeddings with less room than a page. */
  terminalClassName?: string
}) {
  const now = useNow()
  const goal = useQuery(goalQueryOptions(session.goal_id))
  const task = useQuery({
    ...taskQueryOptions(session.task_id ?? ""),
    enabled: Boolean(session.task_id),
  })
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === session.profile_id)
  const taskTo = useTaskPanelTo(session.task_id ?? "")

  return (
    <div className="space-y-4">
      <header className="flex flex-wrap items-center gap-3">
        <h1 className="font-heading text-xl font-semibold tracking-tight">
          {ROLE_LABELS[session.role]} session
        </h1>
        <SessionStatusBadge status={session.status} />
        <code className="font-mono text-xs text-muted-foreground">{session.id}</code>
        <div className="ml-auto">
          <SessionActions session={session} onResumed={onResumed} />
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>Details</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-x-6 gap-y-3 sm:grid-cols-2 lg:grid-cols-3">
            {context === "goal" ? null : (
              <Detail label="Goal">
                <Link to={paths.goal(session.goal_id)} className="hover:underline">
                  {goal.data?.title ?? <Mono>{session.goal_id}</Mono>}
                </Link>
              </Detail>
            )}
            {context === "task" ? null : (
              <Detail label="Task">
                {session.task_id ? (
                  <Link to={taskTo} className="hover:underline">
                    {task.data?.title ?? <Mono>{session.task_id}</Mono>}
                  </Link>
                ) : (
                  <span className="text-muted-foreground">— (planner session)</span>
                )}
              </Detail>
            )}
            <Detail label="Profile">
              {profile?.name ?? <Mono>{session.profile_id}</Mono>}
              <span className="text-muted-foreground">
                {" "}
                · {AGENT_KIND_LABELS[session.agent_kind]}
              </span>
            </Detail>
            <Detail label="tmux session">
              <Mono>{session.tmux_session}</Mono>
            </Detail>
            <Detail label="Worktree">
              {session.worktree_path ? <Mono>{session.worktree_path}</Mono> : <Dash />}
            </Detail>
            <Detail label="Agent session id">
              {session.internal_session_id ? <Mono>{session.internal_session_id}</Mono> : <Dash />}
            </Detail>
            <Detail label="Review round">{session.review_round ?? <Dash />}</Detail>
            <Detail label="Started">
              <Ago at={session.created_at} now={now} />
            </Detail>
            <Detail label={session.ended_at ? "Ended" : "Last activity"}>
              <Ago at={session.ended_at ?? session.last_activity_at} now={now} />
            </Detail>
          </dl>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Terminal</CardTitle>
        </CardHeader>
        <CardContent>
          <SessionTerminal
            sessionId={session.id}
            status={session.status}
            screenClassName={terminalClassName}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Agent activity</CardTitle>
        </CardHeader>
        <CardContent>
          <SessionActivity sessionId={session.id} />
        </CardContent>
      </Card>
    </div>
  )
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0 space-y-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="truncate text-sm">{children}</dd>
    </div>
  )
}

/** Age of a timestamp, with the exact time on hover. */
function Ago({ at, now }: { at: string | null | undefined; now: number }) {
  if (!at) return <Dash />
  return (
    <time dateTime={at} title={formatTimestamp(at)}>
      {formatAge(at, now)} ago
    </time>
  )
}

function Mono({ children }: { children: ReactNode }) {
  return <code className="font-mono text-xs">{children}</code>
}

function Dash() {
  return <span className="text-muted-foreground">—</span>
}
