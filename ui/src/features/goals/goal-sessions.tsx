/**
 * The sessions of one goal, inside its panel: every agent the goal has run —
 * the planner's own session and the ones of each of its tasks — with the
 * selected one opened in place of the list.
 *
 * The selection lives in the URL (`?session=`, next to the panel's `?tab=`),
 * so a link can point straight at a session inside a goal, and closing the
 * panel takes it away with it (see `src/components/detail-panels.tsx`).
 *
 * The list is {@link SessionsList} and the detail is {@link SessionDetailView},
 * the same two the task panel's sessions tab is made of — both stay live off
 * the query cache the event dispatcher patches.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { reason } from "@/features/sessions/session-actions"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"

export function GoalSessions({
  goalId,
  sessionId,
  onSelect,
}: {
  goalId: string
  /** The session the URL points at, if any. */
  sessionId: string | null
  /** Selects a session, or goes back to the list with `null`. */
  onSelect: (sessionId: string | null) => void
}) {
  if (sessionId) {
    return <SelectedSession goalId={goalId} sessionId={sessionId} onSelect={onSelect} />
  }

  return <SessionsList filters={{ goal: goalId }} onSelect={(session) => onSelect(session.id)} />
}

function SelectedSession({
  goalId,
  sessionId,
  onSelect,
}: {
  goalId: string
  sessionId: string
  onSelect: (sessionId: string | null) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))
  // `GET /v1/sessions/{id}` is not scoped to a goal, so a link can hand this
  // tab a session of some *other* goal. It is not one of this goal's, and the
  // panel would present it as if it were — with kill and resume on it.
  const foreign = session.data !== undefined && session.data.goal_id !== goalId

  return (
    <div className="space-y-4">
      <Button variant="ghost" size="sm" onClick={() => onSelect(null)}>
        <ArrowLeftIcon />
        All sessions
      </Button>

      {session.isPending ? (
        <div className="space-y-4">
          <Skeleton className="h-8 w-64" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-64 w-full" />
        </div>
      ) : session.isError ? (
        // A link can point at a session that is gone altogether; the tab says
        // so and stays usable.
        <Alert variant="destructive">
          <AlertTitle>Could not load session {sessionId}</AlertTitle>
          <AlertDescription>{reason(session.error)}</AlertDescription>
        </Alert>
      ) : foreign ? (
        <Alert variant="destructive">
          <AlertTitle>Not a session of this goal</AlertTitle>
          <AlertDescription>
            Session {sessionId} belongs to another goal, so it is not shown here.
          </AlertDescription>
        </Alert>
      ) : (
        <SessionDetailView
          session={session.data}
          context="goal"
          // A resume hands back the session to attach to; the tab follows it.
          onResumed={(revived) => onSelect(revived.id)}
          terminalClassName="h-80"
        />
      )}
    </div>
  )
}
