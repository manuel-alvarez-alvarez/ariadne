/**
 * Delete confirmation.
 *
 * The daemon refuses to delete a profile that anything still points at — a
 * goal's planner, a task's engineer or reviewer, or any session that ever ran
 * as it — and answers 409. That is a normal outcome rather than an error to
 * toast away, so it is shown in the dialog, which stays open.
 */

import { useEffect, useState } from "react"
import { toast } from "sonner"

import { ApiError, type ProfileDto } from "@/api"
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

import { useDeleteProfile } from "./queries"

export function DeleteProfileDialog({
  open,
  onOpenChange,
  profile,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  profile: ProfileDto | null
}) {
  const deleteProfile = useDeleteProfile()
  const [inUse, setInUse] = useState<string | null>(null)
  const [failure, setFailure] = useState<string | null>(null)

  // Re-opening on another profile must not show the previous verdict.
  useEffect(() => {
    if (open) {
      setInUse(null)
      setFailure(null)
    }
  }, [open])

  async function confirm() {
    if (!profile) return
    try {
      await deleteProfile.mutateAsync(profile.id)
      toast.success("Profile deleted", { description: profile.name })
      onOpenChange(false)
    } catch (error) {
      if (ApiError.is(error) && error.status === 409) {
        setInUse(error.message)
        return
      }
      setFailure(ApiError.is(error) ? error.message : String(error))
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete “{profile?.name}”?</DialogTitle>
          <DialogDescription>
            The profile is removed from the daemon. Sessions that already ran as it keep their
            transcripts.
          </DialogDescription>
        </DialogHeader>

        {inUse ? (
          <Alert variant="destructive">
            <AlertTitle>This profile is still in use</AlertTitle>
            <AlertDescription>
              A profile can only be deleted once nothing references it — a goal's planner, a task's
              engineer or reviewer, or a session that ran as it. The daemon said:{" "}
              <span className="text-foreground">{inUse}</span>
            </AlertDescription>
          </Alert>
        ) : null}

        {failure ? (
          <Alert variant="destructive">
            <AlertTitle>Could not delete the profile</AlertTitle>
            <AlertDescription>{failure}</AlertDescription>
          </Alert>
        ) : null}

        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>
            {inUse ? "Close" : "Cancel"}
          </DialogClose>
          <Button
            variant="destructive"
            onClick={confirm}
            disabled={deleteProfile.isPending || inUse !== null}
          >
            {deleteProfile.isPending ? "Deleting…" : "Delete profile"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
