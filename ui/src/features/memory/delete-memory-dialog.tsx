/**
 * "Delete this memory?" A memory holds nothing else's reference the way a
 * profile or a repository can, so unlike {@link import("@/components/delete-dialog").DeleteDialog}
 * there is no 409 to read — a plain {@link ConfirmDialog} is the whole of it.
 */

import { useEffect } from "react"
import { toast } from "sonner"

import type { MemoryDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"

import { useDeleteMemory } from "./queries"

export function DeleteMemoryDialog({
  open,
  onOpenChange,
  repositoryId,
  memory,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  repositoryId: string
  memory: MemoryDto | null
}) {
  const deleteMemory = useDeleteMemory(repositoryId)

  // Re-opening on another row must not show the previous failure.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reset` is the trigger's cleanup, not an input
  useEffect(() => {
    if (open) deleteMemory.reset()
  }, [open])

  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      title="Delete this memory?"
      description="Nothing kept a copy: an agent that searches this repository's memory again will not find it."
      confirmLabel="Delete memory"
      destructive
      pending={deleteMemory.isPending}
      error={deleteMemory.error}
      errorTitle="Could not delete the memory"
      onConfirm={() => {
        if (!memory) return
        deleteMemory.mutate(memory.id, {
          onSuccess: () => {
            onOpenChange(false)
            toast.success("Memory deleted")
          },
        })
      }}
    />
  )
}
