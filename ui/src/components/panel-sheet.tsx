/**
 * The one place a panel close is decided. The frame is a pane floating over
 * the screen, while the URL still owns its contents and the history entry a
 * close unwinds. Its close button, its scrim and Escape all close it here.
 * Escape belongs to this pane only when focus is inside it; a dialog opened
 * from it owns its own Escape, even though its portal bubbles through here.
 */

import { type ReactNode, type RefObject, useEffect, useRef } from "react"

import { DockedPane } from "@/components/ui/docked-pane"

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
    opener.current ??= document.activeElement instanceof HTMLElement ? document.activeElement : null
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
      const target = opener.current
      queueMicrotask(() => {
        if (target?.isConnected) target.focus()
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
        // The inline console sends Escape as input and also lets the panel
        // close. Its Expand modal stops propagation itself (spec rule 20).
        if (event.defaultPrevented && !(event.target as Element).closest(".xterm")) return
        event.stopPropagation()
        onClose()
      }}
    >
      {children}
    </DockedPane>
  )
}
