/**
 * Delete confirmation.
 *
 * The daemon refuses to delete a repository any goal or task still references
 * and answers 409, naming what holds it. That is a normal outcome rather than
 * an error to toast away, so it is shown in the dialog, which stays open.
 */

import { useEffect, useState } from "react"
import { toast } from "sonner"

import { ApiError, type RepositoryDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

import { useDeleteRepository } from "./queries"

export function DeleteRepositoryDialog({
  open,
  onOpenChange,
  repository,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  repository: RepositoryDto | null
}) {
  const deleteRepository = useDeleteRepository()
  const [inUse, setInUse] = useState<string | null>(null)
  const [failure, setFailure] = useState<unknown>(null)

  // Re-opening on another repository must not show the previous verdict.
  useEffect(() => {
    if (open) {
      setInUse(null)
      setFailure(null)
    }
  }, [open])

  async function confirm() {
    if (!repository) return
    try {
      await deleteRepository.mutateAsync(repository.id)
      toast.success("Repository removed", { description: repository.path })
      onOpenChange(false)
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
      title={`Remove “${repository?.path}”?`}
      description="Ariadne forgets the checkout; nothing on disk is touched, and goals already working in it keep their worktrees."
      confirmLabel="Remove repository"
      destructive
      pending={deleteRepository.isPending}
      // Nothing the dialog can do about a reference the daemon still sees.
      confirmDisabled={inUse !== null}
      error={failure}
      errorTitle="Could not remove the repository"
      onConfirm={() => void confirm()}
    >
      {inUse ? (
        <Alert variant="destructive">
          <AlertTitle>This repository is still in use</AlertTitle>
          <AlertDescription>
            A repository can only be removed once nothing references it — a goal it was created for,
            or a task branched off it. The daemon said:{" "}
            <span className="text-foreground">{inUse}</span>
          </AlertDescription>
        </Alert>
      ) : null}
    </ConfirmDialog>
  )
}
