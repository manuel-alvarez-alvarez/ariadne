/**
 * The "Set up worker" dialog of the Hosts screen: the SSH worker setup form,
 * its "Start or retry setup" and "Load setup progress" buttons, and the step
 * progress and errors of the settings daemon's setup operation.
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
          <DialogTitle>Set up worker</DialogTitle>
          <DialogDescription>
            Install Ariadne on a machine over SSH and add it as a host.
          </DialogDescription>
        </DialogHeader>
        <WorkerSetup />
      </DialogContent>
    </Dialog>
  )
}
