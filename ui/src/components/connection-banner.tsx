/**
 * The app-level "you are not looking at a live daemon" banner, under the
 * header.
 *
 * The sidebar's {@link import("./connection-status").ConnectionStatus} dot is
 * the ambient indicator and stays; this is the one that interrupts, because a
 * daemon that went away turns every screen into a wall of error alerts that all
 * say the same thing and none of which say what to do about it. So it says it
 * once, at the top, with the settings dialog one click away — the daemon URL is
 * the only thing the UI has configured, and a wrong one looks exactly like a
 * dead daemon.
 *
 * Two states, because they are genuinely different: the REST probe losing the
 * daemon means nothing loads at all, while the event stream alone dropping
 * means the screens still load but stop updating themselves.
 */

import { PlugZapIcon, RefreshCwIcon, SettingsIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { useConnection } from "@/hooks/use-connection"
import { cn } from "@/lib/utils"

export function ConnectionBanner({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { status, streamStatus, baseUrl, refetch } = useConnection()

  // `connecting` is the first probe of a cold start and says nothing yet;
  // `idle`/`connecting` on the stream are its own first attempt.
  const unreachable = status === "disconnected"
  const streamDown = streamStatus === "reconnecting"
  if (!unreachable && !streamDown) return null

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        "flex h-9 shrink-0 items-center gap-2 border-b px-3 text-sm",
        unreachable
          ? "border-destructive/30 bg-destructive/10 text-destructive"
          : "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
      )}
    >
      <PlugZapIcon className="size-4 shrink-0" aria-hidden />
      <span className="truncate font-medium">
        {unreachable ? "Daemon unreachable — check settings" : "Live updates lost — reconnecting"}
      </span>
      <span className="hidden truncate font-mono text-xs opacity-70 sm:inline">{baseUrl}</span>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button variant="ghost" size="sm" className="h-7 px-2" onClick={refetch}>
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
