/**
 * The "Update worker" dialog of the Hosts screen: the update form, its "Start
 * or retry update" and "Load update progress" buttons, and the step progress
 * and errors of the settings daemon's update operation. A new machine comes
 * from the "Add machine" dialog (`add-machine-dialog.tsx`).
 */

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { WorkerSetup } from "./worker-setup"

export function WorkerSetupDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Update worker</DialogTitle>
          <DialogDescription>Update worker programs after running tasks finish.</DialogDescription>
        </DialogHeader>
        <WorkerSetup />
      </DialogContent>
    </Dialog>
  )
}
