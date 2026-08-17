/**
 * The two things the *user* actor may do to a task: cancel it, and retry it
 * once it has failed.
 *
 * Both are named with the thing they act on. "Cancel" alone, on a panel that
 * is itself dismissible, reads as a way out of the panel rather than the end
 * of the task — and these buttons sit in the same corner of the panel as the
 * goal's and the session's, which say what they act on for the same reason.
 *
 * Which of them is offered comes from `status.ts`, which mirrors the daemon's
 * transition table — the daemon is still the authority, and if it disagrees
 * (the task moved between the render and the click) its error envelope is
 * shown as-is rather than reworded.
 */

import { BanIcon, RotateCcwIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type { TaskDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { isSettling } from "@/lib/confirm-flow"
import { useCancelTask, useRetryTask } from "./queries"
import { canCancel, canRetry, statusLabel } from "./status"

export function TaskActions({ task }: { task: TaskDto }) {
  const cancel = useCancelTask(task.id)
  const retry = useRetryTask(task.id)
  const [open, setOpen] = useState<"cancel" | "retry" | null>(null)

  const showCancel = canCancel(task.status)
  const showRetry = canRetry(task.status)
  // Cancelling is optimistic, so by the time the request is in flight the task
  // is already `cancelled` and neither button applies any more. Returning null
  // here would take the open dialog — its spinner, and the refusal it may be
  // about to show — down with them.
  const settling = isSettling(
    { open: open === "cancel", pending: cancel.isPending, error: cancel.error },
    { open: open === "retry", pending: retry.isPending, error: retry.error },
  )
  if (!showCancel && !showRetry && !settling) return null

  function close() {
    setOpen(null)
    cancel.reset()
    retry.reset()
  }

  return (
    <div className="flex items-center gap-2">
      {showRetry && (
        <Button variant="outline" size="sm" onClick={() => setOpen("retry")}>
          <RotateCcwIcon />
          Retry task
        </Button>
      )}
      {showCancel && (
        // Only opens the confirm; the solid red is on the click inside it.
        <Button variant="destructive-ghost" size="sm" onClick={() => setOpen("cancel")}>
          <BanIcon />
          Cancel task
        </Button>
      )}

      <ConfirmDialog
        open={open === "retry"}
        onClose={close}
        title="Retry this task?"
        description={
          <>
            The task goes back to <strong>ready</strong> and the daemon schedules a fresh engineer
            session for it. Its branch and worktree are kept.
          </>
        }
        confirmLabel="Retry"
        pending={retry.isPending}
        error={retry.error}
        onConfirm={() =>
          retry.mutate(undefined, {
            onSuccess: (updated) => {
              close()
              toast.success("Task retried", {
                description: `Now ${statusLabel(updated.status).toLowerCase()}.`,
              })
            },
          })
        }
      />

      <ConfirmDialog
        open={open === "cancel"}
        onClose={close}
        title="Cancel this task?"
        description={
          <>
            <strong>Cancelled</strong> is terminal: no agent will work on{" "}
            <span className="font-medium">{task.title}</span> again, and nothing moves it back.
          </>
        }
        confirmLabel="Cancel task"
        // "Cancel" is the dangerous verb here, so the safe way out is spelled out.
        dismissLabel="Keep task"
        destructive
        pending={cancel.isPending}
        error={cancel.error}
        onConfirm={() =>
          cancel.mutate(undefined, {
            onSuccess: () => {
              close()
              toast.success("Task cancelled")
            },
          })
        }
      />
    </div>
  )
}
