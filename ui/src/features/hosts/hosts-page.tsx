/**
 * The Hosts screen: every host of the settings daemon's `GET /v1/hosts`, with
 * the facts `ariadne host ls` prints — name, kind, status, endpoint and
 * error — live through `host_updated`.
 *
 * Configured hosts and provisioned workers share this list. "Add host", and
 * each row's Edit and Remove, change a host through `POST`, `PATCH` and
 * `DELETE /v1/hosts`; the table follows `host_updated` and `host_deleted`. The
 * "Set up worker" action opens the settings daemon's durable SSH setup operation. The
 * screen reads `local` only, so the header's host picker does not change it.
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
import { HostFormDialog } from "./host-form-dialog"
import { RemoveHostDialog } from "./remove-host-dialog"
import { WorkerSetupDialog } from "./worker-setup-dialog"

export function HostsPage() {
  const { hosts, error } = useHosts()
  const [updateOpen, setUpdateOpen] = useState(false)
  const [setupOpen, setSetupOpen] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<HostView | null>(null)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [removing, setRemoving] = useState<string | null>(null)
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Hosts"
        description="The daemons this app reads. Add, edit or remove one here, or set up a worker over SSH. local is the settings daemon itself."
        actions={
          <>
            <Button
              variant="outline"
              onClick={() => {
                setEditing(null)
                setFormOpen(true)
              }}
            >
              <PlusIcon />
              Add host
            </Button>
            <Button variant="outline" onClick={() => setUpdateOpen(true)}>
              Update worker
            </Button>
            <Button onClick={() => setSetupOpen(true)}>
              <PlusIcon />
              Set up worker
            </Button>
          </>
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
        </TableBody>
      </Table>
      <WorkerSetupDialog update open={updateOpen} onOpenChange={setUpdateOpen} />
      <WorkerSetupDialog open={setupOpen} onOpenChange={setSetupOpen} />
      <HostFormDialog open={formOpen} onOpenChange={setFormOpen} host={editing} />
      <RemoveHostDialog open={removeOpen} onOpenChange={setRemoveOpen} name={removing} />
    </div>
  )
}

/** One host: what the app reaches of it, and the daemon its stream names. */
function HostRow({
  host,
  onEdit,
  onRemove,
}: {
  host: HostView
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
