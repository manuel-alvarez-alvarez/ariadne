/**
 * The agents that worked this task — the `session ls --task` equivalent, in
 * the task's own panel: the sessions table filtered to this task, and the
 * session that was picked from it, in full.
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

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"
import { usePanelSessionTo } from "@/routes/paths"
import { describeError, shortId } from "./format"

export function TaskSessions({
  taskId,
  onSelect,
}: {
  taskId: string
  /** Selects a session, which opens it over the whole panel. */
  onSelect: (sessionId: string) => void
}) {
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
  /** Selects another session, or goes back to the task with `undefined`. */
  onSelect: (sessionId: string | undefined) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))

  return (
    <>
      <SheetHeader>
        <Button
          variant="ghost"
          size="sm"
          className="-ml-2 w-fit"
          onClick={() => onSelect(undefined)}
        >
          <ArrowLeftIcon />
          Back to {taskTitle ?? `task ${shortId(taskId)}`}
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
        <Alert variant="destructive">
          <AlertTitle>Could not load session {shortId(sessionId)}</AlertTitle>
          <AlertDescription>{describeError(session.error)}</AlertDescription>
        </Alert>
      ) : (
        // No `terminalClassName`: the session has the panel to itself now, so
        // the terminal keeps the height it has on a page of its own.
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
 */
export function SessionLink({ sessionId }: { sessionId: string }) {
  const to = usePanelSessionTo(sessionId)
  return (
    <Link
      to={to}
      title={sessionId}
      className="font-mono text-muted-foreground underline-offset-3 hover:text-foreground hover:underline"
    >
      session {shortId(sessionId)}
    </Link>
  )
}
