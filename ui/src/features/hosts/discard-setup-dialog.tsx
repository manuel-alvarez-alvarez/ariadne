/**
 * "Discard setup" of a saved setup that stopped: `DELETE /v1/hosts/{name}/setup`.
 * It deletes the saved record only. Files on the remote machine stay.
 */

import { ConfirmDialog } from "@/components/confirm-dialog"
import { useDiscardSetup } from "./queries"

export function DiscardSetupDialog({
  name,
  onClose,
}: {
  /** The setup to discard; null while the dialog is closed. */
  name: string | null
  onClose: () => void
}) {
  const discard = useDiscardSetup()
  return (
    <ConfirmDialog
      open={name !== null}
      onClose={() => {
        discard.reset()
        onClose()
      }}
      title={`Discard the setup of ${name ?? ""}?`}
      description="Ariadne forgets this setup, and a new setup starts from its first step. Files on the remote machine stay."
      confirmLabel="Discard setup"
      destructive
      pending={discard.isPending}
      error={discard.error}
      errorTitle="Could not discard the setup"
      onConfirm={() => name && discard.mutate(name, { onSuccess: onClose })}
    />
  )
}
