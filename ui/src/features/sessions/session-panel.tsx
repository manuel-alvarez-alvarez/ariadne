/**
 * One session in a side panel of its own, over whatever screen it was opened
 * from — the sessions screen, the attention list — driven by a `?session=` that
 * belongs to no other panel (see `src/components/detail-panels.tsx`).
 *
 * It is the same {@link SessionDetailView} the goal and task panels drill into,
 * with no `context`: nothing is open behind it, so the goal and the task the
 * session belongs to are shown as links out of it rather than dropped. That is
 * also why there is no "foreign session" check here — the panel is scoped to
 * nothing, and every session is equally its own, the planner's included.
 *
 * The id can come straight from a link or a reload, so the session it names may
 * be gone altogether: that is an error inside the panel, not a broken screen.
 */

import { useQuery } from "@tanstack/react-query"
import { useLocation, useSearchParams } from "react-router-dom"

import { ErrorState } from "@/components/error-state"
import { PanelSheet } from "@/components/panel-sheet"
import { SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { shortId } from "@/lib/format"
import { sessionPanelFrom } from "@/routes/paths"

import { sessionQueryOptions } from "./queries"
import { SessionDetailView } from "./session-detail-view"

export function SessionPanel({ sessionId, onClose }: { sessionId: string; onClose: () => void }) {
  const [search, setSearch] = useSearchParams()
  // The panel floats over whichever screen opened it, and the sessions screen
  // owns two of the params a session panel would otherwise clear.
  const { pathname } = useLocation()
  const session = useQuery(sessionQueryOptions(sessionId))

  return (
    // The panel is a terminal with a header on it, so Escape is the pane's
    // whenever the pane has the keyboard; see `PanelSheet`.
    <PanelSheet onClose={onClose}>
      {/* As wide as the other panels: the terminal is the point of this one. */}
      <SheetContent className="sm:max-w-3xl" aria-describedby={undefined}>
        <SheetHeader>
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
        ) : session.isError ? (
          <ErrorState
            title={`Could not load session ${shortId(sessionId)}`}
            error={session.error}
            onRetry={() => void session.refetch()}
          />
        ) : (
          <SessionDetailView
            session={session.data}
            // A resume hands back the session to attach to; the panel follows
            // it. It replaces rather than pushes: the revived session is still
            // this one panel, and Back should close it, not walk the sessions
            // it has been pointed at.
            onResumed={(revived) =>
              setSearch(sessionPanelFrom(pathname, search, revived.id).search, { replace: true })
            }
          />
        )}
      </SheetContent>
    </PanelSheet>
  )
}
