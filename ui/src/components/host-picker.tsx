/**
 * The header's host picker: which hosts the screens read.
 *
 * All, `local`, and each named host of the settings daemon's `GET /v1/hosts`
 * with its status, live through `host_updated`. The pick is `hostFilter` in
 * the settings store, so it persists; `useHostFilter` is how a screen reads
 * it. `local`'s own connection state is the connection banner's and the
 * status button's — a named host's state shows here, beside its name.
 */

import { ServerIcon } from "lucide-react"

import { type HostView, LOCAL_HOST, useHosts } from "@/api"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useHasNamedHosts } from "@/hooks/use-host-filter"
import { cn } from "@/lib/format"
import { useSettingsStore } from "@/stores/settings"

/** The radio value of All; a host name is never empty. */
const ALL = ""

/** What a host's status reads as in the picker, and its dot. */
const STATUS_META: Record<HostView["status"], { label: string; dot: string }> = {
  ready: { label: "ready", dot: "bg-status-done" },
  connecting: { label: "connecting", dot: "bg-status-warn" },
  down: { label: "down", dot: "bg-status-danger" },
}

/**
 * What a named host reads as: a host the app cannot reach is "not reachable"
 * whatever its own status says, so a socket-path host never reads as ready.
 */
export function hostStatus(host: HostView): { label: string; dot: string } {
  if (host.url === null) return { label: "not reachable", dot: "bg-status-danger" }
  return STATUS_META[host.status]
}

export function HostPicker() {
  const { hosts } = useHosts()
  const pick = useSettingsStore((state) => state.hostFilter)
  const setPick = useSettingsStore((state) => state.setHostFilter)
  // A pick the hosts no longer list reads as All, which is what it filters to.
  const shown = hosts.some((host) => host.name === pick) ? pick : ALL
  const shownHost = hosts.find((host) => host.name === shown)
  const named = useHasNamedHosts()
  // With `local` alone there is nothing to pick.
  if (!named) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button variant="outline" size="sm" className="gap-2 font-normal" aria-label="Host" />
        }
      >
        <ServerIcon />
        {shown === ALL ? "All hosts" : shown}
        {shownHost && shownHost.name !== LOCAL_HOST ? (
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            <span aria-hidden className={cn("size-1.5 rounded-full", hostStatus(shownHost).dot)} />
            {hostStatus(shownHost).label}
          </span>
        ) : null}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        <DropdownMenuRadioGroup value={shown} onValueChange={(value) => setPick(String(value))}>
          <DropdownMenuRadioItem value={ALL}>All hosts</DropdownMenuRadioItem>
          <DropdownMenuSeparator />
          {hosts.map((host) => (
            <DropdownMenuRadioItem key={host.name} value={host.name}>
              <HostOption host={host} />
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** One host: its name, and for a named host its status and why it is down. */
function HostOption({ host }: { host: HostView }) {
  const status = hostStatus(host)
  return (
    <span className="flex min-w-0 flex-1 flex-col">
      <span className="flex items-center gap-2">
        <span className="truncate font-mono">{host.name}</span>
        {host.name === LOCAL_HOST ? null : (
          <span className="ml-auto flex items-center gap-1 text-xs text-muted-foreground">
            <span aria-hidden className={cn("size-1.5 rounded-full", status.dot)} />
            {status.label}
          </span>
        )}
      </span>
      {host.unreachable && host.name !== LOCAL_HOST ? (
        <span className="truncate text-xs text-muted-foreground">{host.unreachable}</span>
      ) : null}
    </span>
  )
}
