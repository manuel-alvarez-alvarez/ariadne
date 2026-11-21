/**
 * The header's host picker: which hosts the screens read.
 *
 * All, `local`, and each named host of the settings daemon's `GET /v1/hosts`
 * with its status, live through `host_updated`. The pick is `hostFilter` in
 * the settings store, so it persists; `useHostFilter` is how a screen reads
 * it. `local`'s own connection state is the connection banner's and the
 * status button's — a named host's state shows here, beside its name: its
 * reach, the app's stream to it while that is not connected, and its sync
 * while that is pending or failed.
 */

import { ServerIcon } from "lucide-react"

import { type HostView, LOCAL_HOST, useHosts } from "@/api"
import { HostDot } from "@/components/host-dot"
import { HostLabel } from "@/components/host-label"
import { syncLabel } from "@/components/host-sync"
import { ReconnectHostButton } from "@/components/reconnect-host-button"
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
import { type HostStream, useHostStreamStore } from "@/stores/stream"

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

/** What the app's stream to a reachable host says while it is not connected. */
function streamNote(host: HostView, stream: HostStream | undefined): string | null {
  if (host.url === null || !stream || stream.status === "open" || stream.status === "idle") {
    return null
  }
  return `stream ${stream.status}`
}

/** What a host's sync says while it is pending or failed. */
function syncNote(host: HostView): string | null {
  return host.sync.state === "in_sync" ? null : `sync ${syncLabel(host)}`
}

/** Whether a named host needs the user: not reachable, or its sync failed. */
function needsAttention(host: HostView): boolean {
  return host.name !== LOCAL_HOST && (host.url === null || host.sync.state === "failed")
}

export function HostPicker() {
  const { hosts } = useHosts()
  const pick = useSettingsStore((state) => state.hostFilter)
  const setPick = useSettingsStore((state) => state.setHostFilter)
  // A pick the hosts no longer list reads as All, which is what it filters to.
  const shown = hosts.some((host) => host.name === pick) ? pick : ALL
  const shownHost = hosts.find((host) => host.name === shown)
  const named = useHasNamedHosts()
  const warn = shown === ALL && hosts.some(needsAttention)
  // With `local` alone there is nothing to pick.
  if (!named) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button variant="outline" size="sm" className="gap-2 font-normal" aria-label="Host" />
        }
      >
        {shown === ALL ? <ServerIcon /> : <HostDot host={shown} />}
        {shown === ALL ? "All hosts" : shown}
        {warn ? (
          <span
            role="img"
            aria-label="A host is not reachable or failed to sync"
            title="A host is not reachable or failed to sync"
            className="size-1.5 rounded-full bg-status-danger"
          />
        ) : null}
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
            <div key={host.name} className="flex items-center gap-1">
              <DropdownMenuRadioItem value={host.name} className="min-w-0 flex-1">
                <HostOption host={host} />
              </DropdownMenuRadioItem>
              <ReconnectHostButton name={host.name} />
            </div>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** One host: its name, and for a named host its status, why it is down, its stream and its sync. */
function HostOption({ host }: { host: HostView }) {
  const status = hostStatus(host)
  const stream = useHostStreamStore((state) => state.streams[host.name])
  const notes =
    host.name === LOCAL_HOST
      ? []
      : [host.unreachable, streamNote(host, stream), syncNote(host)].filter((note) => note !== null)
  return (
    <span className="flex min-w-0 flex-1 flex-col">
      <span className="flex items-center gap-2">
        <HostLabel host={host.name} className="truncate" />
        {host.name === LOCAL_HOST ? null : (
          <span className="ml-auto flex items-center gap-1 text-xs text-muted-foreground">
            <span aria-hidden className={cn("size-1.5 rounded-full", status.dot)} />
            {status.label}
          </span>
        )}
      </span>
      {notes.map((note) => (
        <span key={note} className="truncate text-xs text-muted-foreground">
          {note}
        </span>
      ))}
    </span>
  )
}
