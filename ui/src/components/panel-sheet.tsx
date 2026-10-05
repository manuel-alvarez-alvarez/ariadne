/**
 * The one place a panel close is decided. The frame is a pane floating over
 * the screen, while the URL still owns its contents and the history entry a
 * close unwinds. Its close button, its scrim and Escape all close it here.
 * Escape belongs to this pane only when focus is inside it; a dialog opened
 * from it owns its own Escape, even though its portal bubbles through here.
 */

import { type ReactNode, type RefObject, useEffect, useRef } from "react"

import { DockedPane } from "@/components/ui/docked-pane"

// A replacement mounts before the old sheet's queued focus return runs.
// Keep its original opener available until the next sheet claims it.
let pendingReturn: { target: HTMLElement | null } | null = null

export function PanelSheet({
  onClose,
  children,
  panelRef,
}: {
  onClose: () => void
  children: ReactNode
  panelRef?: RefObject<HTMLDivElement | null>
}) {
  const ownRef = useRef<HTMLDivElement>(null)
  const panel = panelRef ?? ownRef
  const opener = useRef<HTMLElement | null>(null)

  useEffect(() => {
    const active = document.activeElement
    opener.current ??=
      pendingReturn?.target ??
      (active instanceof HTMLElement && active !== document.body ? active : null)
    pendingReturn = null
    // The close button leads the tab order, but a panel opens on its own
    // first control: the close is one Shift+Tab or Escape away.
    const first = panel.current?.querySelector<HTMLElement>(
      'button:not(:disabled):not([data-slot="pane-close"]), a[href], [role="tab"]',
    )
    ;(first ?? panel.current)?.focus()
  }, [panel])

  useEffect(
    () => () => {
      // The opener is on the screen behind the pane's scrim, which goes away
      // on this same commit. Wait until React has removed this panel
      // before focusing it.
      const returning = { target: opener.current }
      pendingReturn = returning
      queueMicrotask(() => {
        // A replacement claims the opener and keeps focus until its own close.
        if (pendingReturn !== returning) return
        pendingReturn = null
        if (!document.querySelector('[data-slot="docked-pane"]') && returning.target?.isConnected) {
          returning.target.focus()
        }
      })
    },
    [],
  )

  return (
    <DockedPane
      ref={panel}
      onClose={onClose}
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !event.currentTarget.contains(event.target as Node)) return
        // A focused console, or a dialog opened from this pane, already
        // answered this Escape (spec rule 21) — a close here would be a
        // second meaning for one keystroke.
        if (event.defaultPrevented) return
        event.stopPropagation()
        onClose()
      }}
    >
      {children}
    </DockedPane>
  )
}
