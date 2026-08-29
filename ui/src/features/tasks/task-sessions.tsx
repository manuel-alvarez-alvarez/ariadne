/**
 * The agents that worked this task — the `session ls --task` equivalent, in
 * the task's own panel: the sessions table filtered to this task, and the
 * session that was picked from it, in full.
 *
 * Just the table: what the task has spent is the figure in its facts, and the
 * split by who spent it is the hint behind that figure (see
 * {@link import("@/components/token-figure").TokenFigure}). Every row here
 * carries its own session's figure besides.
 *
 * Picking one is drilling into it: {@link TaskSessionView} takes over the whole
 * panel (see `task-panel.tsx`), header and tabs included, with a link back to
 * the task. Both pieces are the sessions feature's own components, so a session
 * shown here is the same one the rest of the app shows, terminal and actions
 * included.
 *
 * The selection lives in the URL (`?session=`), like the tab itself: a link
 * can point at one agent's terminal inside a task, and the panel closing takes
 * it away again (see `detail-panels.tsx`).
 *
 * The terminal only exists while a session is the selected one — going back to
 * the list unmounts it, which drops the log stream. That is the default and it
 * is kept: the stream replays the whole pane on connect, so coming back costs a
 * reconnect and shows the same thing, where keeping it mounted would hold a
 * stream open for a session nobody is looking at.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"
import { Link } from "react-router-dom"

import { ErrorState } from "@/components/error-state"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"
import { shortId } from "@/lib/format"
import { usePanelSessionTo } from "@/routes/paths"

export function TaskSessions({
  taskId,
  onSelect,
}: {
  taskId: string
  /** Selects a session, which opens it over the whole panel. */
  onSelect: (sessionId: string) => void
}) {
  // The selected row marks itself: `SessionsList` reads the same `?session=`
  // this panel drives, so nothing has to be threaded through the panel.
  return <SessionsList filters={{ task: taskId }} onSelect={(session) => onSelect(session.id)} />
}

/**
 * The selected session as the panel's whole body, fetched by id rather than
 * taken from the row that was clicked: the id can also come straight from a
 * link, and then there is no row — and there may be no such session at all,
 * which is an empty state and not a broken panel.
 */
export function TaskSessionView({
  taskId,
  taskTitle,
  sessionId,
  onSelect,
}: {
  taskId: string
  /** The task's own name, when it is already loaded, for the way back. */
  taskTitle?: string
  sessionId: string
  /** Selects another session, or goes back to the task with `null`. */
  onSelect: (sessionId: string | null) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))
  // `GET /v1/sessions/{id}` is not scoped to a task, so a link can hand this
  // panel a session of some *other* task (or a goal's planner session, which
  // is nobody's task). It is not one of this task's, and the panel would
  // present it as if it were — with kill and resume on it.
  const foreign = session.data !== undefined && session.data.task_id !== taskId

  return (
    <>
      <SheetHeader>
        {/* `max-w-full` and the truncating label are what keep a long task
            title out from under the sheet's close button: a button is
            `whitespace-nowrap` and `w-fit`, so without them it grows straight
            through the header's own right padding (the goal panel's way back
            is the same one). */}
        <Button
          variant="ghost"
          size="sm"
          className="-ml-2 w-fit max-w-full"
          onClick={() => onSelect(null)}
        >
          <ArrowLeftIcon />
          <span className="truncate">Back to {taskTitle ?? `task ${shortId(taskId)}`}</span>
        </Button>
        {/* The panel is a dialog and needs a name of its own; the view below
            carries the visible heading. */}
        <SheetTitle className="sr-only">Session {shortId(sessionId)}</SheetTitle>
      </SheetHeader>

      {session.isPending ? (
        <div className="space-y-3">
          <Skeleton className="h-8 w-64" />
          <Skeleton className="h-32 w-full" />
          <Skeleton className="h-72 w-full" />
        </div>
      ) : session.error ? (
        <ErrorState
          title={`Could not load session ${shortId(sessionId)}`}
          error={session.error}
          onRetry={() => void session.refetch()}
        />
      ) : foreign ? (
        <Alert variant="destructive">
          <AlertTitle>Not a session of this task</AlertTitle>
          <AlertDescription>
            Session {shortId(sessionId)} belongs to another task, so it is not shown here.
          </AlertDescription>
        </Alert>
      ) : (
        <SessionDetailView
          session={session.data}
          context="task"
          // A resume hands back the session to attach to; the panel follows it.
          onResumed={(revived) => onSelect(revived.id)}
        />
      )}
    </>
  )
}

/**
 * A session id mentioned elsewhere in the panel — who posted a message, who
 * left a review — as a way into the view above.
 *
 * It replaces, like the rest of the navigation inside a panel: where the user
 * is within the panel is not a step of its own, and closing the panel has to
 * close it rather than step back out of the session.
 */
export function SessionLink({ sessionId }: { sessionId: string }) {
  const to = usePanelSessionTo(sessionId)
  return (
    // The short id is what fits in a sentence; the whole one is the hint, and
    // a hint in this app is a tooltip rather than a mouse-only `title=`.
    <Tooltip>
      <TooltipTrigger
        render={
          <Link
            to={to}
            replace
            className="font-mono text-muted-foreground underline-offset-3 hover:text-foreground hover:underline"
          />
        }
      >
        session {shortId(sessionId)}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{sessionId}</TooltipContent>
    </Tooltip>
  )
}
