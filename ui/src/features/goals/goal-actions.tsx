/**
 * The two things a user can do to a goal from outside the thread.
 *
 * Both are confirmed first — finalizing starts every ready task, cancelling
 * tears the goal's sessions and worktrees down — and both surface the daemon's
 * 409 in the dialog rather than closing on a failure ("cannot finalize a plan
 * with no tasks" is the one a user will actually hit).
 */

import { CheckCheckIcon, CircleSlashIcon } from "lucide-react"
import { useEffect, useState } from "react"
import { toast } from "sonner"

import type { GoalDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { useCancelGoal, useFinalizeGoalPlan } from "./queries"
import { isTerminalGoalStatus } from "./status"

export function GoalActions({ goal }: { goal: GoalDto }) {
  const [finalizeOpen, setFinalizeOpen] = useState(false)
  const [cancelOpen, setCancelOpen] = useState(false)

  const canFinalize = goal.status === "planning"
  const canCancel = !isTerminalGoalStatus(goal.status)

  if (!canFinalize && !canCancel) return null

  return (
    <div className="flex items-center gap-2">
      {canFinalize ? (
        <Button variant="outline" onClick={() => setFinalizeOpen(true)}>
          <CheckCheckIcon />
          Finalize plan
        </Button>
      ) : null}
      {canCancel ? (
        // Only opens the confirm; the solid red is on the click inside it.
        <Button variant="destructive-ghost" onClick={() => setCancelOpen(true)}>
          <CircleSlashIcon />
          Cancel goal
        </Button>
      ) : null}

      <FinalizePlanDialog goal={goal} open={finalizeOpen} onOpenChange={setFinalizeOpen} />
      <CancelGoalDialog goal={goal} open={cancelOpen} onOpenChange={setCancelOpen} />
    </div>
  )
}

function FinalizePlanDialog({
  goal,
  open,
  onOpenChange,
}: {
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [summary, setSummary] = useState("")
  const finalize = useFinalizeGoalPlan(goal.id)

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
}: {
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const cancel = useCancelGoal(goal.id)

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
