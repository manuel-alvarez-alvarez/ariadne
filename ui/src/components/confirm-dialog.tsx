/**
 * "Are you sure?", for every action that cannot be taken back.
 *
 * The daemon is the authority on whether the action is allowed, so a refusal
 * is shown in the dialog and the dialog stays open — a 409 ("cannot finalize a
 * plan with no tasks", "the profile is still in use") is the answer the user
 * needs to read, not something to toast away.
 *
 * The dismiss button says "Cancel" unless the dangerous verb is itself
 * "Cancel …", where the caller passes the opposite ("Keep goal") so the same
 * word is never both the safe and the destructive choice in one dialog.
 *
 * The confirming click is where `variant="destructive"` earns its solid red:
 * whatever the trigger that opened this dialog looked like, the button that
 * cannot be taken back is never the quieter one on screen.
 */

import type { ReactNode } from "react"

import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export function ConfirmDialog({
  open,
  onClose,
  title,
  description,
  children,
  confirmLabel,
  dismissLabel = "Cancel",
  destructive = false,
  pending = false,
  confirmDisabled = false,
  error,
  errorTitle = "The daemon refused",
  className,
  onConfirm,
}: {
  open: boolean
  onClose: () => void
  title: string
  /** What the action does, in the dialog's own description slot. */
  description: ReactNode
  /** Anything the dialog needs below the description — a field, an explanation. */
  children?: ReactNode
  confirmLabel: string
  dismissLabel?: string
  destructive?: boolean
  pending?: boolean
  /** Keeps the action out of reach when the dialog already knows it will fail. */
  confirmDisabled?: boolean
  /** The failed mutation's error, if it failed; shown in the dialog. */
  error?: unknown
  errorTitle?: string
  className?: string
  onConfirm: () => void
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose()
      }}
    >
      <DialogContent className={className}>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        {children}
        {error != null && <ErrorState title={errorTitle} error={error} />}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            {dismissLabel}
          </DialogClose>
          <Button
            type="button"
            variant={destructive ? "destructive" : "default"}
            pending={pending}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/**
 * When a confirm flow has to stay on screen, even though its own action just
 * took the reason for it away.
 *
 * An optimistic mutation puts the row into its new state on the click — and
 * that is exactly the state whose actions a screen stops offering. Left alone,
 * "Cancel goal" therefore unmounts its own confirm dialog the moment it is
 * confirmed: the spinner disappears mid-request, and a refusal rolls the cache
 * back with no dialog left to read the reason in.
 *
 * So a screen that hides its actions on a terminal status asks {@link isSettling}
 * first. A flow is still settling while its dialog is open, while its request is
 * in flight, and while it holds an error nobody has read yet — the last of these
 * because a rollback restores the *previous* status, and the window where that
 * has landed but the dialog has not re-rendered is exactly where the error would
 * otherwise be thrown away.
 */
export interface ConfirmFlow {
  /** Its dialog is on screen. */
  open: boolean
  /** Its mutation is in flight. */
  pending: boolean
  /** Its mutation's last error, until something resets it. */
  error?: unknown
}

/** True while any of these flows still needs to be rendered. */
export function isSettling(...flows: ConfirmFlow[]): boolean {
  return flows.some((flow) => flow.open || flow.pending || flow.error != null)
}
