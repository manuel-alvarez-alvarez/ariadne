/**
 * The sessions of one goal, inside its panel: the goal's own agent — the
 * planner, once per resume or restart — as a list, and the one that was picked
 * from it, in full. The sessions of the goal's tasks are not listed here; each
 * task panel has its own sessions tab for those.
 *
 * Just the list: what the goal has spent is the figure in its facts, and the
 * split by the role that spent it is the hint behind that figure (see
 * {@link import("@/components/token-figure").TokenFigure}). Every row here
 * carries its own session's figure besides.
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

import { ErrorState } from "@/components/error-state"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"
import { shortId } from "@/lib/format"

export function GoalSessions({
  goalId,
  onSelect,
}: {
  goalId: string
  /** Selects a session, which opens it over the whole panel. */
  onSelect: (sessionId: string) => void
}) {
  // The selected row marks itself: `SessionsList` reads the same `?session=`
  // this panel drives, so nothing has to be threaded through the panel.
  return (
    <SessionsList
      filters={{ goal: goalId, role: "planner" }}
      onSelect={(session) => onSelect(session.id)}
    />
  )
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
        {/* `max-w-full` and the truncating label are what keep a long goal
            title out from under the sheet's close button: a button is
            `whitespace-nowrap` and `w-fit`, so without them it grows straight
            through the header's own right padding. */}
        <Button
          variant="ghost"
          size="sm"
          className="-ml-2 w-fit max-w-full"
          onClick={() => onSelect(null)}
        >
          <ArrowLeftIcon />
          <span className="truncate">Back to {goalTitle ?? `goal ${shortId(goalId)}`}</span>
        </Button>
        {/* The panel is a dialog and needs a name of its own; the view below
            carries the visible heading. */}
        <SheetTitle className="sr-only">Session {shortId(sessionId)}</SheetTitle>
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
        <ErrorState
          title={`Could not load session ${shortId(sessionId)}`}
          error={session.error}
          onRetry={() => void session.refetch()}
        />
      ) : foreign ? (
        <Alert variant="destructive">
          <AlertTitle>Not a session of this goal</AlertTitle>
          <AlertDescription>
            Session {shortId(sessionId)} belongs to another goal, so it is not shown here.
          </AlertDescription>
        </Alert>
      ) : (
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
