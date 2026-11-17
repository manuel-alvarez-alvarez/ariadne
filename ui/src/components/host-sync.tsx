/**
 * The sync line of every named host, under a screen `local` owns.
 *
 * The skills, workflows, permissions and repositories screens read and write
 * `local` only; the daemon pushes what they hold to each host (037). One line
 * per host says how far that push got: in sync, pending, or failed with the
 * error. The Learned tab also says how many rows a host skipped.
 */

import { type HostView, LOCAL_HOST, useHosts } from "@/api"
import { cn } from "@/lib/format"

const STATE_LABEL: Record<HostView["sync"]["state"], string> = {
  in_sync: "in sync",
  pending: "pending",
  failed: "failed",
}

const STATE_TONE: Record<HostView["sync"]["state"], string> = {
  in_sync: "bg-status-done",
  pending: "bg-status-warn",
  failed: "bg-status-danger",
}

export function HostSync({ skippedPermissions = false }: { skippedPermissions?: boolean }) {
  const { hosts } = useHosts()
  const named = hosts.filter((host) => host.name !== LOCAL_HOST)
  if (named.length === 0) return null
  return (
    <section className="flex flex-col gap-2 text-xs text-muted-foreground">
      <p data-testid="local-note">
        This screen shows and writes the settings of <span className="font-mono">local</span>,
        whichever host the picker has selected. Ariadne pushes them to the other hosts.
      </p>
      <h2 className="font-medium text-foreground">Sync to other hosts</h2>
      <ul aria-label="Sync" className="flex flex-col gap-1">
        {named.map((host) => (
          <li key={host.name} className="flex items-center gap-2" data-testid={`sync-${host.name}`}>
            <span
              aria-hidden
              className={cn("size-1.5 rounded-full", STATE_TONE[host.sync.state])}
            />
            <span className="font-mono">{host.name}</span>
            <span>
              {STATE_LABEL[host.sync.state]}
              {host.sync.state === "failed" && host.sync.error ? `: ${host.sync.error}` : ""}
            </span>
            {skippedPermissions && host.sync.skipped_permissions > 0 ? (
              <span>
                · {host.sync.skipped_permissions} learned{" "}
                {host.sync.skipped_permissions === 1 ? "permission" : "permissions"} skipped, no
                checkout of their repository
              </span>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  )
}
