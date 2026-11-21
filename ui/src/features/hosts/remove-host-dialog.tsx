/**
 * "Remove this host?": `DELETE /v1/hosts/{name}`. The app stops reading the
 * host; the daemon on it keeps running. A refusal is a toast.
 */

import { toast } from "sonner"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { describeError } from "@/lib/format"
import { useDeleteHost } from "./queries"

export function RemoveHostDialog({
  open,
  onOpenChange,
  name,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  name: string | null
}) {
  const deleteHost = useDeleteHost()
  return (
    <ConfirmDialog
      open={open}
      onClose={() => onOpenChange(false)}
      title={`Remove ${name}?`}
      description="Ariadne stops reading this host. The daemon on it keeps running."
      confirmLabel="Remove host"
      destructive
      pending={deleteHost.isPending}
      onConfirm={() => {
        if (!name) return
        deleteHost.mutate(name, {
          onSuccess: () => {
            toast.success("Host removed", { description: name })
            onOpenChange(false)
          },
          onError: (error) =>
            toast.error(`Could not remove ${name}`, { description: describeError(error) }),
        })
      }}
    />
  )
}
