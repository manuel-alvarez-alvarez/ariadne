/**
 * The settings dialog's Hosts section: every host of the settings daemon's
 * `GET /v1/hosts`, with the facts `ariadne host ls` prints — name, kind,
 * status, endpoint and error — live through `host_updated`.
 *
 * Configured hosts and provisioned workers share this list. Setup uses the
 * settings daemon's durable SSH setup operation.
 */

import { type HostView, useHosts } from "@/api"
import { hostStatus } from "@/components/host-picker"
import { ReconnectHostButton } from "@/components/reconnect-host-button"
import { FieldDescription, FieldLabel } from "@/components/ui/field"
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
import { WorkerSetup } from "./worker-setup"

export function HostsSettings() {
  const { hosts, error } = useHosts()
  return (
    <section aria-labelledby="settings-hosts" className="flex flex-col gap-2">
      <FieldLabel id="settings-hosts">Hosts</FieldLabel>
      <FieldDescription>
        The daemons this app reads, as the <code className="font-mono">[hosts.&lt;name&gt;]</code>{" "}
        tables of the <code className="font-mono">config.toml</code> of the daemon above configure
        them. Setup adds worker connections below. <code className="font-mono">local</code> is that
        daemon itself.
      </FieldDescription>
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          Could not load the hosts: {describeError(error)}
        </p>
      ) : null}
      <WorkerSetup />
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
    </section>
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
