/**
 * A dialog that will not throw away what was typed into it.
 *
 * Every dialog in the app dismisses on an outside press and on Escape, which
 * is right for the ones that only show something and wrong for the ones that
 * hold a form: a goal brief or a profile's four prompts are the longest text
 * the app ever asks for, and one misplaced click a few paragraphs in is not an
 * instruction to delete them.
 *
 * So a form dialog says whether it is dirty, and this stands in for
 * `<Dialog>`: pristine it dismisses exactly as before, dirty every way out —
 * outside press, Escape, the close button, the form's own Cancel — asks
 * first.
 *
 * What it guards is dismissal, which is the dialog root's `onOpenChange` and
 * nothing else. A submit is not a dismissal and needs no way around the guard:
 * a form closes itself on success by calling the `onOpenChange` it was handed,
 * which is the caller's own setter rather than the guarded handler this passes
 * down, so a saved form closes on the spot with nothing asked about a draft
 * that is no longer one. `form-dialog.test.tsx` pins that, as does the
 * repository dialog's own test through a real mutation.
 *
 * The confirmation is a dialog of its own nested inside this one's root, which
 * is what keeps Base UI's stacking, focus trap and Escape handling straight:
 * Escape over the confirmation answers it rather than the form behind it.
 */

import { type ReactNode, useEffect, useState } from "react"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { Dialog } from "@/components/ui/dialog"

export function FormDialog({
  open,
  onOpenChange,
  dirty,
  discardTitle = "Discard changes?",
  discardDescription = "This form has unsaved changes. Closing it now drops them.",
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Whether the form holds anything a dismissal would destroy. */
  dirty: boolean
  discardTitle?: string
  discardDescription?: ReactNode
  children: ReactNode
}) {
  const [asking, setAsking] = useState(false)

  // The form closing some other way — a successful submit, the screen behind it
  // navigating — takes the question with it, so the next open does not come up
  // with a discard prompt over a form nobody has touched yet.
  useEffect(() => {
    if (!open) setAsking(false)
  }, [open])

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // Opening, and closing a pristine form, are what they have always been.
        if (next || !dirty) {
          setAsking(false)
          onOpenChange(next)
          return
        }
        // Dirty: the dialog is controlled, so not forwarding this is what keeps
        // it on screen with everything in it intact.
        setAsking(true)
      }}
    >
      {children}
      <ConfirmDialog
        open={asking}
        onClose={() => setAsking(false)}
        title={discardTitle}
        description={discardDescription}
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => {
          setAsking(false)
          onOpenChange(false)
        }}
      />
    </Dialog>
  )
}
