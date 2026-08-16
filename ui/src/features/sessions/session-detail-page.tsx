/**
 * One agent session: what it is, what it is printing, and what it reported.
 *
 * The metadata comes from the query cache, which the event dispatcher keeps
 * current — a session going idle or being killed elsewhere updates this screen
 * without a refetch. The terminal is the exception: it is a byte stream, not
 * cacheable state, and owns its own connection (see `log-stream.ts`).
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link, useParams } from "react-router-dom"

import type { SessionDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { paths, useTaskPanelTo } from "@/routes/paths"

import {
  goalQueryOptions,
  profilesQueryOptions,
  sessionQueryOptions,
  taskQueryOptions,
} from "./queries"
import { reason, SessionActions } from "./session-actions"
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

export function SessionDetailPage() {
  const { sessionId } = useParams<{ sessionId: string }>()
  const session = useQuery({ ...sessionQueryOptions(sessionId ?? ""), enabled: Boolean(sessionId) })

  if (!sessionId) return <NotFound />

  if (session.isPending) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-72" />
        <Skeleton className="h-32 w-full" />
        <Skeleton className="h-96 w-full" />
      </div>
    )
  }

  if (session.isError) {
    return (
      <div className="space-y-4">
        <BackLink />
        <Alert variant="destructive">
          <AlertTitle>Could not load this session</AlertTitle>
          <AlertDescription>{reason(session.error)}</AlertDescription>
        </Alert>
      </div>
    )
  }

  return <SessionDetail session={session.data} />
}

function SessionDetail({ session }: { session: SessionDto }) {
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
      <BackLink />

      <header className="flex flex-wrap items-center gap-3">
        <h1 className="font-heading text-xl font-semibold tracking-tight">
          {ROLE_LABELS[session.role]} session
        </h1>
        <SessionStatusBadge status={session.status} />
        <code className="font-mono text-xs text-muted-foreground">{session.id}</code>
        <div className="ml-auto">
          <SessionActions session={session} />
        </div>
      </header>

      <Card>
        <CardHeader>
          <CardTitle>Details</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-x-6 gap-y-3 sm:grid-cols-2 lg:grid-cols-3">
            <Detail label="Goal">
              <Link to={paths.goal(session.goal_id)} className="hover:underline">
                {goal.data?.title ?? <Mono>{session.goal_id}</Mono>}
              </Link>
            </Detail>
            <Detail label="Task">
              {session.task_id ? (
                <Link to={taskTo} className="hover:underline">
                  {task.data?.title ?? <Mono>{session.task_id}</Mono>}
                </Link>
              ) : (
                <span className="text-muted-foreground">— (planner session)</span>
              )}
            </Detail>
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
          <SessionTerminal sessionId={session.id} />
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

function BackLink() {
  return (
    <Button variant="ghost" size="sm" render={<Link to={paths.sessions()} />}>
      <ArrowLeftIcon />
      Sessions
    </Button>
  )
}

function NotFound() {
  return (
    <Alert variant="destructive">
      <AlertTitle>No session id in the URL</AlertTitle>
      <AlertDescription>
        Open a session from the <Link to={paths.sessions()}>sessions list</Link>.
      </AlertDescription>
    </Alert>
  )
}
