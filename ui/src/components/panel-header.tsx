/**
 * The header every detail panel opens on: the goal, task and session panels,
 * and the session drill-downs inside the goal's and the task's own — five
 * places that used to each lay the same three rows out by hand, and drift
 * apart the way the goal's and the task's already had (a status badge on the
 * title row in one and the meta row in the other, a title that wrapped in one
 * and had no tooltip in the other).
 *
 * The rows, top to bottom, every one but the title optional:
 *
 * - a breadcrumb back to the entity this panel is drilled into — a goal, for
 *   a task's or a session's own panel; a task, for one of its sessions;
 * - the title, which always truncates to one line with the full text as its
 *   `title` attribute, and the panel's own actions at the row's end;
 * - the dense meta row: the entity's status first, then its id, then when it
 *   was made and when it last moved.
 */

import { ChevronRightIcon } from "lucide-react"
import type { ReactNode } from "react"

import { PaneHeader, PaneTitle } from "@/components/ui/docked-pane"

export function PanelHeader({
  breadcrumb,
  title,
  actions,
  status,
  id,
  stamps,
}: {
  /** The entity this panel is drilled into; omitted where the panel is top-level. */
  breadcrumb?: {
    /** What the ancestor is called. */
    label: string
    /** Opens the ancestor in this panel's place. */
    onClick: () => void
  }
  title: string
  /** What can be done to the entity, at the title row's end. */
  actions?: ReactNode
  /** The entity's own status badge, or badges, leading the meta row. */
  status?: ReactNode
  /** The entity's id, after its status. */
  id?: ReactNode
  /** When it was made and when it last moved, after the id. */
  stamps?: ReactNode
}) {
  return (
    <PaneHeader>
      {breadcrumb ? (
        <nav
          aria-label="Breadcrumb"
          // Clears the sheet's own close button, which floats over this row.
          className="flex shrink-0 min-w-0 items-center gap-1.5 pr-8 text-xs text-muted-foreground"
        >
          <button
            type="button"
            onClick={breadcrumb.onClick}
            // The ring every other control in the app wears: this one is the
            // first thing focused when a deep link opens the panel, and it
            // was showing the browser's own outline there.
            className="min-w-0 truncate rounded-xs underline-offset-3 outline-none hover:text-foreground hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
          >
            {breadcrumb.label}
          </button>
          <ChevronRightIcon className="size-3 shrink-0" aria-hidden />
          <span className="min-w-0 truncate font-medium text-foreground" aria-current="page">
            {title}
          </span>
        </nav>
      ) : null}
      <div className="flex items-start gap-3">
        <PaneTitle className="min-w-0 flex-1 truncate" title={title}>
          {title}
        </PaneTitle>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </div>
      {status || id || stamps ? (
        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
          {status}
          {id}
          {stamps}
        </div>
      ) : null}
    </PaneHeader>
  )
}
