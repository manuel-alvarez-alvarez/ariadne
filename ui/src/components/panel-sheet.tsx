/**
 * A side panel that does not close out from under what the user is doing in
 * it.
 *
 * Every sheet in the app dismisses on Escape and on a press outside it, which
 * is right for a panel that only shows something. These panels do more than
 * show: two of them hold a compose box, and all three can hold a live terminal
 * the user types into. Both turn a routine dismissal into a loss.
 *
 * **Escape belongs to a focused terminal.** It is `\x1b` on the way to the
 * agent's pane — an interrupt in Claude Code, a dismissed menu in Codex — so a
 * panel whose terminal has the keyboard cancels the dismissal and lets the
 * keystroke through. With the pane unfocused Escape closes the panel exactly as
 * it always did. `terminal-focus.ts` is the whole of that check, and the
 * expanded terminal dialog makes the same one.
 *
 * **A written message is asked about before it is dismissed.** The draft
 * survives either way — it is kept per thread in session storage — but a panel
 * that vanishes mid-sentence looks like the sentence went with it, so the
 * accidental ways out ask first. This is `FormDialog`'s guard, with the same
 * shape and the same reason: the confirmation is a dialog nested inside this
 * one's root, so Base UI's stacking, focus trap and Escape handling stay
 * straight and Escape over the question answers the question.
 *
 * The close button is not one of the accidental ways out: it is aimed at, it
 * is the panel's own, and there is nowhere else it could mean. It closes.
 */

import { type ReactNode, useState } from "react"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { readDraft, type ThreadKey } from "@/components/thread-drafts"
import { Sheet } from "@/components/ui/sheet"
import { isTerminalEscape } from "@/features/sessions/terminal-focus"

/** Dismissals that are a slip rather than a decision, and so are worth a question. */
const ACCIDENTAL = new Set(["escape-key", "outside-press", "close-watcher", "focus-out"])

export function PanelSheet({
  onClose,
  draftKey,
  children,
}: {
  onClose: () => void
  /** The thread whose unsent draft this panel is holding, if it holds one. */
  draftKey?: ThreadKey
  /** The sheet's content, and anything that stacks on it. */
  children: ReactNode
}) {
  const [asking, setAsking] = useState(false)

  return (
    <Sheet
      open
      onOpenChange={(open, details) => {
        if (open) return
        if (isTerminalEscape(details.reason)) {
          details.cancel()
          return
        }
        if (draftKey && ACCIDENTAL.has(details.reason) && readDraft(draftKey).trim()) {
          // Cancelled rather than forwarded: the panel — and the message
          // being written in it — stays exactly as it was, and the question
          // goes up over it.
          details.cancel()
          setAsking(true)
          return
        }
        onClose()
      }}
    >
      {children}
      <ConfirmDialog
        open={asking}
        onClose={() => setAsking(false)}
        title="Leave the message unsent?"
        description="The panel has a message that has not been sent. Closing it keeps the draft — it is here again when the thread is reopened."
        confirmLabel="Close the panel"
        dismissLabel="Keep writing"
        onConfirm={() => {
          setAsking(false)
          onClose()
        }}
      />
    </Sheet>
  )
}
