/**
 * The two things a user can do to a goal.
 *
 * Each is confirmed first — cancelling tears the goal's sessions and worktrees
 * down, deleting drops the goal and every trace of it — and each surfaces the
 * daemon's 409 in the dialog rather than closing on a failure ("this goal is
 * running again" is the one a user will actually hit).
 *
 * They divide the lifecycle between them, the way `ariadne goal` does:
 * cancelling belongs to a goal that has not stopped, deleting to one that has.
 * They are therefore exact opposites — every goal offers one of them and never
 * both — which is why this row always has something to show and both dialogs
 * are always mounted. A trigger that goes away mid-flow (cancelling is
 * optimistic, so the goal is terminal by the time the request leaves) leaves
 * its dialog behind to finish, spinner, refusal and all.
 */

import { BanIcon, Trash2Icon } from "lucide-react"
import { useEffect, useState } from "react"
import { toast } from "sonner"

import type { GoalDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { DeleteDialog } from "@/components/delete-dialog"
import { Button } from "@/components/ui/button"
import { useCancelGoal, useDeleteGoal } from "./queries"
import { isTerminalGoalStatus } from "./status"

export function GoalActions({
  goal,
  onDeleted,
}: {
  goal: GoalDto
  /**
   * The goal has stopped existing, so whatever is showing it has to stop too.
   * The panel hands its own close in, so deleting leaves the board exactly the
   * way Escape would (see `routes/panel-history.ts`).
   */
  onDeleted?: () => void
}) {
  const [cancelOpen, setCancelOpen] = useState(false)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const cancel = useCancelGoal(goal.id)

  const terminal = isTerminalGoalStatus(goal.status)
  const canCancel = !terminal
  // A running goal still owns tmux sessions and git worktrees that only
  // cancelling tears down, so the daemon refuses to delete one — and the
  // button is never offered rather than offered and refused.
  const canDelete = terminal

  return (
    <div className="flex items-center gap-2">
      {canCancel ? (
        // Only opens the confirm; the solid red is on the click inside it.
        <Button variant="destructive-ghost" size="sm" onClick={() => setCancelOpen(true)}>
          {/* The same icon the task's cancel wears: one verb, one glyph. */}
          <BanIcon />
          Cancel goal
        </Button>
      ) : null}
      {canDelete ? (
        <Button variant="destructive-ghost" size="sm" onClick={() => setDeleteOpen(true)}>
          <Trash2Icon />
          Delete goal
        </Button>
      ) : null}

      <CancelGoalDialog
        goal={goal}
        open={cancelOpen}
        onOpenChange={setCancelOpen}
        cancel={cancel}
      />
      <DeleteGoalDialog
        goal={goal}
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        onDeleted={onDeleted}
      />
    </div>
  )
}

function CancelGoalDialog({
  goal,
  open,
  onOpenChange,
  cancel,
}: {
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Owned by `GoalActions`: the optimistic flip unmounts this dialog's trigger. */
  cancel: ReturnType<typeof useCancelGoal>
}) {
  useEffect(() => {
    if (open) cancel.reset()
  }, [open, cancel.reset])

  async function confirm() {
    try {
      await cancel.mutateAsync()
      toast.success("Goal cancelled", { description: goal.title })
      onOpenChange(false)
    } catch {
      // Shown in the dialog.
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      className="sm:max-w-lg"
      title="Cancel this goal?"
      description={
        <>
          <span className="font-medium text-foreground">{goal.title}</span> stops here: its running
          sessions are torn down and its unfinished tasks will not be picked up again. This cannot
          be undone.
        </>
      }
      confirmLabel="Cancel goal"
      // "Cancel" is the dangerous verb here, so the safe way out is spelled out.
      dismissLabel="Keep goal"
      destructive
      pending={cancel.isPending}
      error={cancel.error}
      errorTitle="Could not cancel the goal"
      onConfirm={() => void confirm()}
    />
  )
}

/**
 * Deleting is the one action with nothing after it: the goal and its tasks go,
 * and the daemon keeps no copy. So the question names what goes rather than
 * only asking whether to go ahead — the same thing `ariadne goal rm` asks
 * before it deletes.
 *
 * The 409 the daemon answers a goal that is running again with is handled by
 * `DeleteDialog`, along with the rest of the shape this shares with the
 * profile's and the repository's.
 */
function DeleteGoalDialog({
  goal,
  open,
  onOpenChange,
  onDeleted,
}: {
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onDeleted?: () => void
}) {
  const deleteGoal = useDeleteGoal(goal.id)

  return (
    <DeleteDialog
      open={open}
      onOpenChange={onOpenChange}
      className="sm:max-w-lg"
      title="Delete this goal?"
      description={
        <>
          <span className="font-medium text-foreground">{goal.title}</span> and everything under it
          — its tasks and every session that ran them — are removed for good. This cannot be undone
          and none of it can be recovered afterwards.
        </>
      }
      confirmLabel="Delete goal"
      errorTitle="Could not delete the goal"
      pending={deleteGoal.isPending}
      inUseTitle="This goal is running again"
      inUseDescription="Only a goal that has stopped can be deleted — cancel it first, which is what tears its sessions and worktrees down."
      onDelete={async () => {
        await deleteGoal.mutateAsync()
        toast.success("Goal deleted", { description: goal.title })
      }}
      onDeleted={onDeleted}
    />
  )
}
