/**
 * The agents that worked this task — the `session ls --task` equivalent, in
 * the task's own panel: the sessions table filtered to this task, with the
 * selected session laid out under it.
 *
 * Both pieces are the sessions feature's own components, so this tab shows a
 * session in full, terminal and actions included.
 *
 * The selection lives in the URL (`?session=`), like the tab itself: a link
 * can point at one agent's terminal inside a task, and the panel closing takes
 * it away again (see `detail-panels.tsx`).
 *
 * The terminal only exists while this tab is the open one — Base UI unmounts
 * the panels behind it, which drops the log stream when the user goes to read
 * the diff. That is the default and it is kept: the stream replays the whole
 * pane on connect, so coming back costs a reconnect and shows the same thing,
 * where keeping it mounted would hold a stream open for a tab nobody is
 * looking at.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon } from "lucide-react"
import { Link } from "react-router-dom"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { sessionQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView } from "@/features/sessions/session-detail-view"
import { SessionsList } from "@/features/sessions/sessions-list"
import { usePanelSessionTo } from "@/routes/paths"
import { describeError, shortId } from "./format"

export function TaskSessions({
  taskId,
  selectedId,
  onSelect,
}: {
  taskId: string
  /** Session in the URL, if any. Not necessarily one of this task's. */
  selectedId?: string
  /** Selects a session, or clears the selection with `undefined`. */
  onSelect: (sessionId: string | undefined) => void
}) {
  return (
    <div className="space-y-4">
      <SessionsList
        filters={{ task: taskId }}
        selectedId={selectedId}
        onSelect={(session) => onSelect(session.id)}
      />
      {selectedId && <SelectedSession sessionId={selectedId} onSelect={onSelect} />}
    </div>
  )
}

/**
 * The selected session, fetched by id rather than taken from the row that was
 * clicked: the id can also come straight from a link, and then there is no row
 * — and there may be no such session at all, which is an empty state and not a
 * broken tab.
 */
function SelectedSession({
  sessionId,
  onSelect,
}: {
  sessionId: string
  onSelect: (sessionId: string | undefined) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))

  return (
    <section className="space-y-3 border-t pt-4">
      <Button variant="ghost" size="sm" onClick={() => onSelect(undefined)}>
        <ArrowLeftIcon />
        Back to the list
      </Button>
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
        <SessionDetailView
          session={session.data}
          context="task"
          // A resume hands back the session to attach to; the tab follows it.
          onResumed={(revived) => onSelect(revived.id)}
          terminalClassName="h-72"
        />
      )}
    </section>
  )
}

/**
 * A session id mentioned elsewhere in the panel — who posted a message, who
 * left a review — as a way into the tab above.
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
