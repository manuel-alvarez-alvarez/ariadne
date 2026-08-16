/**
 * The screen around one agent session: it resolves the id in the URL, then
 * hands over to {@link SessionDetailView}, which is the same view the goal and
 * task panels embed.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"
import { Link, useNavigate, useParams } from "react-router-dom"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { paths } from "@/routes/paths"

import { sessionQueryOptions } from "./queries"
import { reason } from "./session-actions"
import { SessionDetailView } from "./session-detail-view"

export function SessionDetailPage() {
  const { sessionId } = useParams<{ sessionId: string }>()
  const navigate = useNavigate()
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

  return (
    <div className="space-y-4">
      <BackLink />
      {/* A resume hands back the session to attach to; the screen follows it. */}
      <SessionDetailView
        session={session.data}
        onResumed={(revived) => void navigate(paths.session(revived.id))}
      />
    </div>
  )
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
