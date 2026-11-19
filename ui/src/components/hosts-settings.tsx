/**
 * The settings dialog's Hosts section: every host of the settings daemon's
 * `GET /v1/hosts`, with the facts `ariadne host ls` prints — name, kind,
 * status, endpoint and error — live through `host_updated`.
 *
 * Read-only: the hosts are the settings daemon's `[hosts.<name>]` tables in
 * its `config.toml`, and the section says so rather than editing them.
 */

import { useHosts } from "@/api"
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
import { describeError } from "@/lib/format"

export function HostsSettings() {
  const { hosts, error } = useHosts()
  return (
    <section aria-labelledby="settings-hosts" className="flex flex-col gap-2">
      <FieldLabel id="settings-hosts">Hosts</FieldLabel>
      <FieldDescription>
        The daemons this app reads, as the <code className="font-mono">[hosts.&lt;name&gt;]</code>{" "}
        tables of the <code className="font-mono">config.toml</code> of the daemon above configure
        them. <code className="font-mono">local</code> is that daemon itself.
      </FieldDescription>
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
            <TableHead>
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {hosts.map((host) => (
            <TableRow key={host.name}>
              <TableCell className="font-mono">{host.name}</TableCell>
              <TableCell>{host.kind}</TableCell>
              <TableCell>{host.status}</TableCell>
              <TableCell className="font-mono break-all whitespace-normal">
                {host.endpoint ?? "-"}
              </TableCell>
              <TableCell className="break-words whitespace-normal">{host.error ?? "-"}</TableCell>
              <TableCell>
                <ReconnectHostButton name={host.name} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </section>
  )
}
