/**
 * A side panel that does not close out from under what the user is doing in
 * it.
 *
 * Every sheet in the app dismisses on Escape and on a press outside it, and
 * these panels do too. What they add is the one place a close is decided:
 * the goal, task and session panels all close through `onClose`, which puts
 * the URL back, rather than each one wiring the sheet's own open state.
 *
 * The close button is not one of the accidental ways out: it is aimed at, it
 * is the panel's own, and there is nowhere else it could mean. It closes.
 */

import type { ReactNode } from "react"

import { Sheet } from "@/components/ui/sheet"

export function PanelSheet({
  onClose,
  children,
}: {
  onClose: () => void
  /** The sheet's content, and anything that stacks on it. */
  children: ReactNode
}) {
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) onClose()
      }}
    >
      {children}
    </Sheet>
  )
}
