/**
 * "Delete this?", for the three rows the daemon may refuse to let go of.
 *
 * A profile something still points at — a goal's planner, a task's engineer, a
 * session that ran as it — a repository a goal was created for, and a goal that
 * has been put back to work since the panel rendered all come back as a `409`
 * naming what holds them. That is a normal outcome rather than an error to
 * toast away: it is shown in the dialog, the dialog stays open, and the
 * confirming click stops being offered, because nothing about clicking it again
 * would change the answer.
 *
 * Anything else the daemon says is a failure like any other, and goes through
 * `ConfirmDialog`'s own error slot.
 */

import { type ReactNode, useEffect, useState } from "react"

import { ApiError } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

export function DeleteDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  errorTitle,
  pending,
  inUseTitle,
  inUseDescription,
  className,
  onDelete,
  onDeleted,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: ReactNode
  /** The dangerous verb: "Delete profile", "Remove repository". */
  confirmLabel: string
  errorTitle: string
  pending: boolean
  /** Heading of the 409 alert: "This profile is still in use". */
  inUseTitle: string
  /** What "in use" means here; the daemon's own words follow it. */
  inUseDescription: ReactNode
  /** Sizing for the dialog box, where the question needs more than the default. */
  className?: string
  /** Does the delete, and says what to toast about it. */
  onDelete: () => Promise<void>
  /** What the screen does once the row is gone and the dialog has closed. */
  onDeleted?: () => void
}) {
  const [inUse, setInUse] = useState<string | null>(null)
  const [failure, setFailure] = useState<unknown>(null)

  // Re-opening on another row must not show the previous verdict.
  useEffect(() => {
    if (open) {
      setInUse(null)
      setFailure(null)
    }
  }, [open])

  async function confirm() {
    try {
      await onDelete()
      onOpenChange(false)
      onDeleted?.()
    } catch (error) {
      if (ApiError.is(error) && error.status === 409) {
        setInUse(error.message)
        return
      }
      setFailure(error)
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      className={className}
      title={title}
      description={description}
      confirmLabel={confirmLabel}
      destructive
      pending={pending}
      // Nothing the dialog can do about a reference the daemon still sees.
      confirmDisabled={inUse !== null}
      error={failure}
      errorTitle={errorTitle}
      onConfirm={() => void confirm()}
    >
      {inUse ? (
        <Alert variant="destructive">
          <AlertTitle>{inUseTitle}</AlertTitle>
          <AlertDescription>
            {inUseDescription} The daemon said: <span className="text-foreground">{inUse}</span>
          </AlertDescription>
        </Alert>
      ) : null}
    </ConfirmDialog>
  )
}
