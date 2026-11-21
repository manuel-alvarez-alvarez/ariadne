/**
 * The Hosts screen: every host of the settings daemon's `GET /v1/hosts`, with
 * the facts `ariadne host ls` prints — name, kind, status, endpoint and
 * error — live through `host_updated`.
 *
 * Configured hosts and provisioned workers share this list. The "Set up
 * worker" action opens the settings daemon's durable SSH setup operation. The
 * screen reads `local` only, so the header's host picker does not change it.
 */

import { PlusIcon } from "lucide-react"
import { useState } from "react"

import { type HostView, useHosts } from "@/api"
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
import { WorkerSetupDialog } from "./worker-setup-dialog"

export function HostsPage() {
  const { hosts, error } = useHosts()
  const [setupOpen, setSetupOpen] = useState(false)
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Hosts"
        description="The daemons this app reads. The [hosts.<name>] tables of the config.toml of the settings daemon configure them, and setup adds worker connections. local is that daemon itself."
        actions={
          <Button onClick={() => setSetupOpen(true)}>
            <PlusIcon />
            Set up worker
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
            <HostRow key={host.name} host={host} />
          ))}
        </TableBody>
      </Table>
      <WorkerSetupDialog open={setupOpen} onOpenChange={setSetupOpen} />
    </div>
  )
}

/** One host: what the app reaches of it, and the daemon its stream names. */
function HostRow({ host }: { host: HostView }) {
  const daemon = useHostDaemon(host.name)
  return (
    <TableRow>
      <TableCell className="font-mono">{host.name}</TableCell>
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
      </TableCell>
    </TableRow>
  )
}
