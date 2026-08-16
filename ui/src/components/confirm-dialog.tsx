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
  pendingLabel = "Working…",
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
  /** The confirm button while the request is in flight. */
  pendingLabel?: string
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
            disabled={pending || confirmDisabled}
            onClick={onConfirm}
          >
            {pending ? pendingLabel : confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
