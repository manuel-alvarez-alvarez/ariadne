/**
 * The Hosts screen: every host of the settings daemon's `GET /v1/hosts`, with
 * the facts `ariadne host ls` prints — name, kind, status, endpoint and
 * error — live through `host_updated`.
 *
 * Configured hosts and installed machines share this list. "Add machine"
 * opens one step-by-step dialog that installs Ariadne through SSH or connects
 * to a daemon that already runs (`add-machine-dialog.tsx`). Each row's Edit
 * and Remove change a host through `PATCH` and `DELETE /v1/hosts`; the table
 * follows `host_updated` and `host_deleted`. A host with a saved setup has an
 * "Update" action (`update-machine-dialog.tsx`). A saved setup that is not
 * complete has its own row, with "Continue setup" and, once it stopped,
 * "Discard setup". `GET /v1/host-setups` has no event, so the table reads it
 * every second while an operation runs. The screen reads `local` only, so
 * the header's host picker does not change it.
 */

import { PencilIcon, PlusIcon, Trash2Icon } from "lucide-react"
import { useState } from "react"

import { type HostView, LOCAL_HOST, useHosts } from "@/api"
import { HostLabel } from "@/components/host-label"
import { hostStatus } from "@/components/host-picker"
import { PageHeader } from "@/components/page-header"
import { ReconnectHostButton } from "@/components/reconnect-host-button"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { useHostDaemon } from "@/hooks/use-host-daemon"
import { describeError, formatDuration } from "@/lib/format"
import { AddMachineDialog } from "./add-machine-dialog"
import { DiscardSetupDialog } from "./discard-setup-dialog"
import { HostFormDialog } from "./host-form-dialog"
import { type HostSetup, useHostSetups } from "./queries"
import { RemoveHostDialog } from "./remove-host-dialog"
import { UpdateMachineDialog } from "./update-machine-dialog"

export function HostsPage() {
  const { hosts, error } = useHosts()
  const setups = useHostSetups().data ?? []
  const [updating, setUpdating] = useState<HostSetup | null>(null)
  const [continuing, setContinuing] = useState<string | null>(null)
  const [discarding, setDiscarding] = useState<string | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<HostView | null>(null)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [removing, setRemoving] = useState<string | null>(null)
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Hosts"
        description="The machines this app reads. Add a machine, or edit or remove one here. local is the daemon of this machine."
        actions={
          <Button
            onClick={() => {
              setContinuing(null)
              setAddOpen(true)
            }}
          >
            <PlusIcon />
            Add machine
          </Button>
        }
      />
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          Could not load the hosts: {describeError(error)}
        </p>
      ) : null}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Kind</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Endpoint</TableHead>
            <TableHead>Error</TableHead>
            <TableHead>Daemon</TableHead>
            <TableHead>
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {hosts.map((host) => (
            <HostRow
              key={host.name}
              host={host}
              onUpdate={(() => {
                const saved = setups.find((one) => one.name === host.name)
                return saved && (saved.state === "complete" || saved.operation === "update")
                  ? () => setUpdating(saved)
                  : null
              })()}
              onEdit={() => {
                setEditing(host)
                setFormOpen(true)
              }}
              onRemove={() => {
                setRemoving(host.name)
                setRemoveOpen(true)
              }}
            />
          ))}
          {setups
            .filter((one) => one.state !== "complete")
            .map((one) => (
              <SetupRow
                key={`setup:${one.name}`}
                setup={one}
                onContinue={() => {
                  if (one.operation === "update") {
                    setUpdating(one)
                    return
                  }
                  setContinuing(one.name)
                  setAddOpen(true)
                }}
                onDiscard={() => setDiscarding(one.name)}
              />
            ))}
        </TableBody>
      </Table>
      <UpdateMachineDialog setup={updating} onOpenChange={(open) => !open && setUpdating(null)} />
      <AddMachineDialog open={addOpen} onOpenChange={setAddOpen} continueSetup={continuing} />
      <DiscardSetupDialog name={discarding} onClose={() => setDiscarding(null)} />
      <HostFormDialog open={formOpen} onOpenChange={setFormOpen} host={editing} />
      <RemoveHostDialog open={removeOpen} onOpenChange={setRemoveOpen} name={removing} />
    </div>
  )
}

/** One host: what the app reaches of it, and the daemon its stream names. */
function HostRow({
  host,
  onUpdate,
  onEdit,
  onRemove,
}: {
  host: HostView
  /** Null where the host has no saved setup to update. */
  onUpdate: (() => void) | null
  onEdit: () => void
  onRemove: () => void
}) {
  const daemon = useHostDaemon(host.name)
  return (
    <TableRow>
      <TableCell>
        <HostLabel host={host.name} />
      </TableCell>
      <TableCell>{host.kind}</TableCell>
      <TableCell>{hostStatus(host).label}</TableCell>
      <TableCell className="font-mono break-all whitespace-normal">
        {host.endpoint ?? "-"}
      </TableCell>
      <TableCell className="break-words whitespace-normal">
        {host.unreachable ?? host.error ?? "-"}
      </TableCell>
      <TableCell>
        {host.url !== null && daemon
          ? `${daemon.version}${daemon.uptimeSecs === null ? "" : ` · up ${formatDuration(daemon.uptimeSecs)}`}`
          : "-"}
      </TableCell>
      <TableCell>
        {onUpdate ? (
          <Button type="button" variant="ghost" size="xs" onClick={onUpdate}>
            Update
          </Button>
        ) : null}
        <ReconnectHostButton name={host.name} />
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={`Edit ${host.name}`}
          onClick={onEdit}
        >
          <PencilIcon />
        </Button>
        {host.name === LOCAL_HOST ? null : (
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Remove ${host.name}`}
            onClick={onRemove}
          >
            <Trash2Icon />
          </Button>
        )}
      </TableCell>
    </TableRow>
  )
}

/** The state of a saved operation, as the row says it. */
function stateLabel(setup: HostSetup): string {
  const noun = setup.operation === "update" ? "Update" : "Setup"
  return `${noun} ${setup.state}`
}

/**
 * A saved setup or update that is not complete. An update continues in the
 * update dialog, and only a setup is discarded: the daemon keeps update records.
 */
function SetupRow({
  setup,
  onContinue,
  onDiscard,
}: {
  setup: HostSetup
  onContinue: () => void
  onDiscard: () => void
}) {
  const stopped = setup.state === "incomplete" || setup.state === "failed"
  return (
    <TableRow>
      <TableCell>{setup.name}</TableCell>
      <TableCell>ssh</TableCell>
      <TableCell>{stateLabel(setup)}</TableCell>
      <TableCell className="font-mono break-all whitespace-normal">{setup.ssh}</TableCell>
      <TableCell className="break-words whitespace-normal">
        {setup.steps.find((step) => step.error && !step.skipped)?.error ?? "-"}
      </TableCell>
      <TableCell>-</TableCell>
      <TableCell>
        <Button type="button" variant="ghost" size="xs" onClick={onContinue}>
          {setup.operation === "update" ? "Continue update" : "Continue setup"}
        </Button>
        {stopped && setup.operation === "setup" ? (
          <Button type="button" variant="ghost" size="xs" onClick={onDiscard}>
            Discard setup
          </Button>
        ) : null}
      </TableCell>
    </TableRow>
  )
}
