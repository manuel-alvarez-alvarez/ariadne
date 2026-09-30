/**
 * "Remove this row?" — a plain confirm rather than `DeleteDialog`: nothing
 * else references a learned row the way a goal references a repository, so
 * there is no "still in use" refusal to make room for, and a refusal of any
 * other kind is toasted with the daemon's own message instead of held open in
 * the dialog.
 *
 * Named by the repository's folder and the request it matches, not only the
 * tool name: two rows both called "Bash" are common, and the tool name alone
 * does not say which one is about to go.
 */

import { toast } from "sonner"

import type { LearnedPermissionDto, RepositoryDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { describeError, folderName } from "@/lib/format"
import { useDeleteLearnedPermission } from "./queries"
import { requestSummary } from "./request-summary"

export function DeleteLearnedPermissionDialog({
  open,
  onOpenChange,
  learned,
  repositories,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  learned: LearnedPermissionDto | null
  /** Every registered repository, to name the one this row belongs to. */
  repositories: RepositoryDto[]
}) {
  const deleteLearned = useDeleteLearnedPermission()

  const repositoryPath =
    learned &&
    (repositories.find((repository) => repository.id === learned.repository_id)?.path ??
      learned.repository_id)
  const request = learned ? requestSummary(learned.tool_call) : null

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      title={
        learned && repositoryPath
          ? `Remove the row for “${learned.tool_name}” in ${folderName(repositoryPath)}?`
          : "Remove this row?"
      }
      description={
        request
          ? `Matches “${request}”. The next matching request is asked for again.`
          : "The next matching request is asked for again."
      }
      confirmLabel="Remove row"
      destructive
      pending={deleteLearned.isPending}
      onConfirm={() => {
        if (!learned) return
        deleteLearned.mutate(learned.id, {
          onSuccess: () => {
            toast.success("Row removed", { description: learned.tool_name })
            onOpenChange(false)
          },
          onError: (error) =>
            toast.error("Could not remove the row", { description: describeError(error) }),
        })
      }}
    />
  )
}
