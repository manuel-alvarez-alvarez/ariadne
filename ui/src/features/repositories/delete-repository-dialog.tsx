import { toast } from "sonner"

import type { RepositoryDto } from "@/api"
import { DeleteDialog } from "@/components/delete-dialog"

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

  return (
    <DeleteDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Remove “${repository?.path}”?`}
      description="Ariadne forgets the checkout; nothing on disk is touched, and goals already working in it keep their worktrees."
      confirmLabel="Remove repository"
      errorTitle="Could not remove the repository"
      pending={deleteRepository.isPending}
      inUseTitle="This repository is still in use"
      inUseDescription="A repository can only be removed once nothing references it — a goal it was created for, or a task branched off it."
      onDelete={async () => {
        if (!repository) return
        await deleteRepository.mutateAsync(repository.id)
        toast.success("Repository removed", { description: repository.path })
      }}
    />
  )
}
