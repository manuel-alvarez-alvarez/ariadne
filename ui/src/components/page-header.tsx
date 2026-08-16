/**
 * The one way a screen introduces itself: a full-bleed column, the screen's
 * name as an `h1`, one line saying what it is, and whatever the screen puts on
 * the right (its filters, its "new" button).
 *
 * There used to be two conventions — the board's `text-lg` full-bleed heading
 * and the profiles screen's `text-xl` inside `max-w-5xl` — and this is the
 * board's, because the board cannot adopt anything else: its swimlanes need
 * every pixel of the width. `goals-list-page.tsx` still writes the same markup
 * by hand (it is owned by another task); everything else uses this.
 */

import type { ReactNode } from "react"

export function PageHeader({
  title,
  description,
  actions,
}: {
  /** The screen's name, the same one the shell's header shows. */
  title: string
  /** One line on what the screen is for. */
  description?: ReactNode
  /** Filters, primary buttons — whatever the screen leads with. */
  actions?: ReactNode
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-2">
      <div>
        <h1 className="font-heading text-lg font-semibold">{title}</h1>
        {description ? <p className="text-sm text-muted-foreground">{description}</p> : null}
      </div>
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </div>
  )
}
