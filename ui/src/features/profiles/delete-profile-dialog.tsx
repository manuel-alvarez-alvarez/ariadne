import { toast } from "sonner"

import type { ProfileDto } from "@/api"
import { DeleteDialog } from "@/components/delete-dialog"

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

  return (
    <DeleteDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Delete “${profile?.name}”?`}
      description="The profile is removed from the daemon. Sessions that already ran as it keep their transcripts."
      confirmLabel="Delete profile"
      errorTitle="Could not delete the profile"
      pending={deleteProfile.isPending}
      inUseTitle="This profile is still in use"
      inUseDescription="A profile can only be deleted once nothing references it — a goal's planner, a task's engineer or reviewer, or a session that ran as it."
      onDelete={async () => {
        if (!profile) return
        await deleteProfile.mutateAsync(profile.id)
        toast.success("Profile deleted", { description: profile.name })
      }}
    />
  )
}
