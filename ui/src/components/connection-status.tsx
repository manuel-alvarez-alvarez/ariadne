/**
 * Global "is this window looking at a live daemon?" indicator.
 *
 * Two independent links are folded into one badge: the REST probe
 * (`/v1/health` + `/v1/version`) and the domain-event stream. REST up but
 * stream down means the screens still load yet stop updating themselves, which
 * is worth saying out loud.
 */

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useConnection } from "@/hooks/use-connection"
import { formatDuration } from "@/lib/time"
import { cn } from "@/lib/utils"

export function ConnectionStatus({ className }: { className?: string }) {
  const { status, streamStatus, baseUrl, version, uptimeSecs, error } = useConnection()

  const live = status === "connected" && streamStatus === "open"
  // Straight off the status ramp: green while both links are up, the warn step
  // for a half-connected or still-connecting daemon, the danger step when it
  // is gone.
  const tone =
    status === "connected"
      ? live
        ? "bg-status-done"
        : "bg-status-warn"
      : status === "connecting"
        ? "bg-status-warn"
        : "bg-status-danger"

  const label =
    status === "connected"
      ? version
        ? `ariadned ${version}`
        : "connected"
      : status === "connecting"
        ? "connecting…"
        : "disconnected"

  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            className={cn(
              "flex items-center gap-2 rounded-md px-2 py-1 text-xs text-muted-foreground",
              className,
            )}
          />
        }
      >
        <span
          className={cn("size-2 shrink-0 rounded-full", tone, live && "animate-pulse")}
          aria-hidden
        />
        <span className="truncate">{label}</span>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="end">
        <div className="space-y-1">
          <p className="font-mono text-xs">{baseUrl}</p>
          <p>
            API: {status}
            {uptimeSecs !== null ? ` · up ${formatDuration(uptimeSecs)}` : ""}
          </p>
          <p>Events: {streamStatus}</p>
          {error ? <p className="text-destructive">{error.message}</p> : null}
        </div>
      </TooltipContent>
    </Tooltip>
  )
}
