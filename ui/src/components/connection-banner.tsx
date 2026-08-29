/**
 * The app-level "you are not looking at a live daemon" banner, under the
 * header.
 *
 * The footer's {@link import("./connection-status").ConnectionStatus} dot is
 * the ambient indicator and stays; this is the one that interrupts, because a
 * daemon that went away turns every screen into a wall of error alerts that all
 * say the same thing and none of which say what to do about it. So it says it
 * once, at the top, with the settings dialog one click away — the daemon URL is
 * the only thing the UI has configured, and a wrong one looks exactly like a
 * dead daemon.
 *
 * One state, because there is one link: the event stream is both how a screen
 * loads live and how the daemon is known to be there at all. Losing it means
 * nothing on screen is being kept up to date and nothing new will load.
 */

import { PlugZapIcon, RefreshCwIcon, SettingsIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { useConnection } from "@/hooks/use-connection"

export function ConnectionBanner({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { status, baseUrl, retry } = useConnection()

  // `connecting` is the first attempt of a cold start and says nothing yet: the
  // banner appears once an attempt has actually failed.
  if (status !== "disconnected") return null

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex h-9 shrink-0 items-center gap-2 border-b bg-status-danger-soft px-3 text-sm text-status-danger-fg"
    >
      <PlugZapIcon className="size-4 shrink-0" aria-hidden />
      <span className="truncate font-medium">Daemon unreachable — check settings</span>
      <span className="hidden truncate font-mono text-xs opacity-70 sm:inline">{baseUrl}</span>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button variant="ghost" size="sm" className="h-7 px-2" onClick={retry}>
          <RefreshCwIcon />
          Retry
        </Button>
        <Button variant="ghost" size="sm" className="h-7 px-2" onClick={onOpenSettings}>
          <SettingsIcon />
          Settings
        </Button>
      </div>
    </div>
  )
}
