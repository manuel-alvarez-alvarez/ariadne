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

import { ApiError, type GoalDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { isTerminalGoalStatus } from "./goal-status-badge"
import { useCancelGoal, useFinalizeGoalPlan } from "./queries"

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
        <Button variant="destructive" onClick={() => setCancelOpen(true)}>
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Finalize the plan?</DialogTitle>
          <DialogDescription>
            The goal moves from planning to active and its tasks start as soon as their dependencies
            allow. The planner cannot add to the plan afterwards.
          </DialogDescription>
        </DialogHeader>
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
        <MutationError title="Could not finalize the plan" error={finalize.error} />
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button onClick={confirm} disabled={finalize.isPending}>
            {finalize.isPending ? "Finalizing…" : "Finalize plan"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Cancel this goal?</DialogTitle>
          <DialogDescription>
            <span className="font-medium text-foreground">{goal.title}</span> stops here: its
            running sessions are torn down and its unfinished tasks will not be picked up again.
            This cannot be undone.
          </DialogDescription>
        </DialogHeader>
        <MutationError title="Could not cancel the goal" error={cancel.error} />
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Keep goal</DialogClose>
          <Button variant="destructive" onClick={confirm} disabled={cancel.isPending}>
            {cancel.isPending ? "Cancelling…" : "Cancel goal"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function MutationError({ title, error }: { title: string; error: unknown }) {
  if (!ApiError.is(error)) return null
  return (
    <Alert variant="destructive">
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>
        {error.message}
        <span className="ml-1 font-mono text-xs">({error.code})</span>
      </AlertDescription>
    </Alert>
  )
}
