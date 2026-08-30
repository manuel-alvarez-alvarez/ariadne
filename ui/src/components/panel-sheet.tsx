/**
 * A side panel that does not close out from under what the user is doing in
 * it.
 *
 * Every sheet in the app dismisses on Escape and on a press outside it, which
 * is right for a panel that only shows something. These panels do more: all
 * three can hold a live terminal the user types into, and Escape belongs to
 * it first.
 *
 * **Escape belongs to a focused terminal.** It is `\x1b` on the way to the
 * agent's pane — an interrupt in Claude Code, a dismissed menu in Codex — so a
 * panel whose terminal has the keyboard cancels the dismissal and lets the
 * keystroke through. With the pane unfocused Escape closes the panel exactly as
 * it always did. `terminal-focus.ts` is the whole of that check, and the
 * expanded terminal dialog makes the same one.
 *
 * The close button is not one of the accidental ways out: it is aimed at, it
 * is the panel's own, and there is nowhere else it could mean. It closes.
 */

import type { ReactNode } from "react"

import { Sheet } from "@/components/ui/sheet"
import { isTerminalEscape } from "@/features/sessions/terminal-focus"

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
      onOpenChange={(open, details) => {
        if (open) return
        if (isTerminalEscape(details.reason)) {
          details.cancel()
          return
        }
        onClose()
      }}
    >
      {children}
    </Sheet>
  )
}
