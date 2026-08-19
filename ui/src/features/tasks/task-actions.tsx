/**
 * The things the *user* actor may do to a task: edit it while it waits,
 * cancel it, and retry it once it has failed.
 *
 * All are named with the thing they act on. "Cancel" alone, on a panel that
 * is itself dismissible, reads as a way out of the panel rather than the end
 * of the task — and these buttons sit in the same corner of the panel as the
 * goal's and the session's, which say what they act on for the same reason.
 *
 * Which of them is offered comes from `status.ts`, which mirrors the daemon's
 * transition table — the daemon is still the authority, and if it disagrees
 * (the task moved between the render and the click) its error envelope is
 * shown as-is rather than reworded.
 *
 * Only cancelling asks first. A confirm dialog here means "this cannot be
 * undone", and retry is the opposite of that: the task goes back to `ready`
 * with its branch and worktree kept, exactly like resuming a session, which
 * also fires on the click. So retry says what it keeps in its tooltip and
 * toasts its refusal, having no dialog to put one in — see
 * `features/sessions/session-actions.tsx` for the same pair.
 */

import { BanIcon, PencilIcon, RotateCcwIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type { TaskDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { isSettling } from "@/lib/confirm-flow"
import { describeError } from "@/lib/errors"
import { useCancelTask, useRetryTask } from "./queries"
import { canCancel, canEdit, canRetry, statusLabel } from "./status"
import { EditTaskDialog } from "./task-form-dialog"

export function TaskActions({ task }: { task: TaskDto }) {
  const cancel = useCancelTask(task.id)
  const retry = useRetryTask(task.id)
  const [open, setOpen] = useState<"edit" | "cancel" | null>(null)

  const showEdit = canEdit(task.status)
  const showCancel = canCancel(task.status)
  const showRetry = canRetry(task.status)
  // Cancelling is optimistic, so by the time the request is in flight the task
  // is already `cancelled` and neither button applies any more. Returning null
  // here would take the open dialog — its spinner, and the refusal it may be
  // about to show — down with them. The edit form is kept the same way: if the
  // task starts while it is open, the save must still get to show the 409.
  const settling = isSettling({
    open: open === "cancel",
    pending: cancel.isPending,
    error: cancel.error,
  })
  if (!showEdit && !showCancel && !showRetry && open !== "edit" && !settling) return null

  function close() {
    setOpen(null)
    cancel.reset()
  }

  return (
    <div className="flex items-center gap-2">
      {showEdit && (
        <Button variant="outline" size="sm" onClick={() => setOpen("edit")}>
          <PencilIcon />
          Edit task
        </Button>
      )}
      {showRetry && (
        <Button
          variant="outline"
          size="sm"
          pending={retry.isPending}
          // The reassuring half of what the confirm used to say; the rest of
          // it — "back to ready, fresh engineer session" — is the toast.
          title="The task goes back to ready and the daemon schedules a fresh engineer session for it. Its branch and worktree are kept."
          onClick={() => {
            retry.mutate(undefined, {
              onSuccess: (updated) => {
                toast.success("Task retried", {
                  description: `Now ${statusLabel(updated.status).toLowerCase()}.`,
                })
              },
              onError: (error) =>
                toast.error("Could not retry", { description: describeError(error) }),
            })
          }}
        >
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

      {/* The form resets itself from the task each time it opens, so it never
          shows a previous attempt; `close()` has nothing to clear for it. */}
      <EditTaskDialog
        task={task}
        open={open === "edit"}
        onOpenChange={(next) => {
          if (!next) setOpen(null)
        }}
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
