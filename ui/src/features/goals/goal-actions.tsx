/**
 * The two things a user can do to a goal from outside the thread.
 *
 * Both are confirmed first — finalizing starts every ready task, cancelling
 * tears the goal's sessions and worktrees down — and both surface the daemon's
 * 409 in the dialog rather than closing on a failure ("cannot finalize a plan
 * with no tasks" is the one a user will actually hit).
 *
 * Both mutations are held here rather than inside their dialogs. Cancelling is
 * optimistic, so the goal is terminal by the time the request leaves — which
 * takes these buttons off screen with it. The dialog waiting for the daemon
 * has to outlive the trigger that opened it, and `isSettling` is what keeps it
 * (and the mutation observer behind it) mounted until there is nothing left to
 * show.
 */

import { CheckCheckIcon, CircleSlashIcon } from "lucide-react"
import { useEffect, useState } from "react"
import { toast } from "sonner"

import type { GoalDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { isSettling } from "@/lib/confirm-flow"
import { useCancelGoal, useFinalizeGoalPlan } from "./queries"
import { isTerminalGoalStatus } from "./status"

export function GoalActions({ goal }: { goal: GoalDto }) {
  const [finalizeOpen, setFinalizeOpen] = useState(false)
  const [cancelOpen, setCancelOpen] = useState(false)
  const finalize = useFinalizeGoalPlan(goal.id)
  const cancel = useCancelGoal(goal.id)

  const canFinalize = goal.status === "planning"
  const canCancel = !isTerminalGoalStatus(goal.status)
  const settling = isSettling(
    { open: finalizeOpen, pending: finalize.isPending, error: finalize.error },
    { open: cancelOpen, pending: cancel.isPending, error: cancel.error },
  )

  if (!canFinalize && !canCancel && !settling) return null

  return (
    <div className="flex items-center gap-2">
      {canFinalize ? (
        <Button variant="outline" size="sm" onClick={() => setFinalizeOpen(true)}>
          <CheckCheckIcon />
          Finalize plan
        </Button>
      ) : null}
      {canCancel ? (
        // Only opens the confirm; the solid red is on the click inside it.
        <Button variant="destructive-ghost" size="sm" onClick={() => setCancelOpen(true)}>
          <CircleSlashIcon />
          Cancel goal
        </Button>
      ) : null}

      <FinalizePlanDialog open={finalizeOpen} onOpenChange={setFinalizeOpen} finalize={finalize} />
      <CancelGoalDialog
        goal={goal}
        open={cancelOpen}
        onOpenChange={setCancelOpen}
        cancel={cancel}
      />
    </div>
  )
}

function FinalizePlanDialog({
  open,
  onOpenChange,
  finalize,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Owned by `GoalActions`, so it survives the goal leaving `planning`. */
  finalize: ReturnType<typeof useFinalizeGoalPlan>
}) {
  const [summary, setSummary] = useState("")

  useEffect(() => {
    if (open) {
      setSummary("")
      finalize.reset()
    }
  }, [open, finalize.reset])

  async function confirm() {
    try {
      await finalize.mutateAsync(summary.trim())
      toast.success("Plan finalized", { description: "The goal is now active." })
      onOpenChange(false)
    } catch {
      // Shown in the dialog; the 409 is the interesting case.
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      className="sm:max-w-lg"
      title="Finalize the plan?"
      description="The goal moves from planning to active and its tasks start as soon as their dependencies allow. The planner cannot add to the plan afterwards."
      confirmLabel="Finalize plan"
      pending={finalize.isPending}
      error={finalize.error}
      errorTitle="Could not finalize the plan"
      onConfirm={() => void confirm()}
    >
      <Field>
        <FieldLabel htmlFor="finalize-summary">Summary</FieldLabel>
        <Textarea
          id="finalize-summary"
          rows={3}
          value={summary}
          onChange={(event) => setSummary(event.target.value)}
          placeholder="Optional — recorded in the goal thread."
        />
        <FieldDescription>Posted to the thread as “Plan finalized: …”.</FieldDescription>
      </Field>
    </ConfirmDialog>
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
