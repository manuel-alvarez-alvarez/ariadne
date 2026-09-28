/**
 * "Remove this approval?" — a plain confirm rather than `DeleteDialog`: nothing
 * else references a learned approval the way a goal references a repository,
 * so there is no "still in use" refusal to make room for, and a refusal of any
 * other kind is toasted with the daemon's own message instead of held open in
 * the dialog.
 */

import { toast } from "sonner"

import type { LearnedPermissionDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { describeError } from "@/lib/format"

import { useDeleteLearnedPermission } from "./queries"

export function DeleteLearnedPermissionDialog({
  open,
  onOpenChange,
  learned,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  learned: LearnedPermissionDto | null
}) {
  const deleteLearned = useDeleteLearnedPermission()

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      title={learned ? `Remove the approval for “${learned.tool_name}”?` : "Remove this approval?"}
      description="The next matching request is asked for again."
      confirmLabel="Remove approval"
      destructive
      pending={deleteLearned.isPending}
      onConfirm={() => {
        if (!learned) return
        deleteLearned.mutate(learned.id, {
          onSuccess: () => {
            toast.success("Approval removed", { description: learned.tool_name })
            onOpenChange(false)
          },
          onError: (error) =>
            toast.error("Could not remove the approval", { description: describeError(error) }),
        })
      }}
    />
  )
}
