/**
 * The one way a screen introduces itself: no heading of its own any more —
 * the shell's window header already carries the screen's name — just
 * `description` and `actions` handed up to that header through
 * `PageHeaderContext` (`app-shell.tsx`). `actions` portals into the slot the
 * header renders at its end; `description` becomes the tooltip of the
 * header's title.
 *
 * `title` stays a required prop even though nothing here reads it: every
 * feature page still passes it, matching the shell's own `handle.title` for
 * the route, and changing that contract was out of scope for the task that
 * moved the heading into the header.
 *
 * Outside the shell — every feature test mounts its screen on its own, with
 * no `AppShell` around it — there is no slot to portal into, so `actions`
 * render inline instead, where a call site's existing query for them still
 * finds them.
 */

import { type ReactNode, useContext, useLayoutEffect } from "react"
import { createPortal } from "react-dom"

import { PageHeaderContext } from "@/components/app-shell"

export function PageHeader({
  description,
  actions,
}: {
  /** The screen's name, the same one the shell's header shows. */
  title: string
  /** One line on what the screen is for, shown as the header title's tooltip. */
  description?: ReactNode
  /** Filters, primary buttons — whatever the screen leads with. */
  actions?: ReactNode
}) {
  const slot = useContext(PageHeaderContext)

  useLayoutEffect(() => {
    if (!slot) return
    slot.setDescription(description ?? null)
    return () => slot.setDescription(null)
  }, [slot, description])

  if (!slot) {
    return actions ? (
      <div className="flex flex-wrap items-center justify-end gap-2">{actions}</div>
    ) : null
  }
  return slot.actionsSlot && actions ? createPortal(actions, slot.actionsSlot) : null
}
