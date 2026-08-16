/**
 * The sessions of one goal, inside its panel: every agent the goal has run —
 * the planner's own session and the ones of each of its tasks — as a list, and
 * the one that was picked from it, in full.
 *
 * Picking one is drilling into it: {@link GoalSessionView} takes over the whole
 * panel (see `goal-panel.tsx`), goal header and tabs included, with a link back
 * to the goal.
 *
 * The selection lives in the URL (`?session=`, next to the panel's `?tab=`),
 * so a link can point straight at a session inside a goal, and closing the
 * panel takes it away with it (see `src/components/detail-panels.tsx`).
 *
 * The list is {@link SessionsList} and the detail is {@link SessionDetailView},
 * the same two the task panel's sessions are made of — both stay live off the
 * query cache the event dispatcher patches.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { reason } from "@/features/sessions/session-actions"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"

export function GoalSessions({
  goalId,
  onSelect,
}: {
  goalId: string
  /** Selects a session, which opens it over the whole panel. */
  onSelect: (sessionId: string) => void
}) {
  return <SessionsList filters={{ goal: goalId }} onSelect={(session) => onSelect(session.id)} />
}

/** The selected session as the panel's whole body, with the way back to the goal. */
export function GoalSessionView({
  goalId,
  goalTitle,
  sessionId,
  onSelect,
}: {
  goalId: string
  /** The goal's own name, when it is already loaded, for the way back. */
  goalTitle?: string
  sessionId: string
  /** Selects another session, or goes back to the goal with `null`. */
  onSelect: (sessionId: string | null) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))
  // `GET /v1/sessions/{id}` is not scoped to a goal, so a link can hand this
  // panel a session of some *other* goal. It is not one of this goal's, and the
  // panel would present it as if it were — with kill and resume on it.
  const foreign = session.data !== undefined && session.data.goal_id !== goalId

  return (
    <>
      <SheetHeader>
        <Button variant="ghost" size="sm" className="-ml-2 w-fit" onClick={() => onSelect(null)}>
          <ArrowLeftIcon />
          Back to {goalTitle ?? "the goal"}
        </Button>
        {/* The panel is a dialog and needs a name of its own; the view below
            carries the visible heading. */}
        <SheetTitle className="sr-only">Session {sessionId}</SheetTitle>
      </SheetHeader>

      {session.isPending ? (
        <div className="space-y-4">
          <Skeleton className="h-8 w-64" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-64 w-full" />
        </div>
      ) : session.isError ? (
        // A link can point at a session that is gone altogether; the panel says
        // so and keeps the way back.
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
        // No `terminalClassName`: the session has the panel to itself now, so
        // the terminal keeps the height it has on a page of its own.
        <SessionDetailView
          session={session.data}
          context="goal"
          // A resume hands back the session to attach to; the panel follows it.
          onResumed={(revived) => onSelect(revived.id)}
        />
      )}
    </>
  )
}
