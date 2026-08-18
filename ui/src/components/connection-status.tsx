/**
 * Global "is this window looking at a live daemon?" indicator, lived in by the
 * shell's footer.
 *
 * Two independent links are folded into one readout: the REST probe
 * (`/v1/health` + `/v1/version`) and the domain-event stream. REST up but
 * stream down means the screens still load yet stop updating themselves, which
 * is worth saying out loud.
 *
 * It renders as a real button because the daemon-logs drawer hangs off the
 * click — `onOpenLogs`, wired by the shell to the drawer it mounts.
 */

import { ChevronUpIcon } from "lucide-react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useConnection } from "@/hooks/use-connection"
import { formatDuration } from "@/lib/time"
import { cn } from "@/lib/utils"

export function ConnectionStatus({
  className,
  onOpenLogs,
}: {
  className?: string
  /** What the click opens: the shell's daemon-logs drawer. */
  onOpenLogs?: () => void
}) {
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
        aria-label="Daemon status — open logs"
        render={<button type="button" onClick={onOpenLogs} />}
        className={cn(
          "flex min-w-0 items-center gap-2 rounded-md px-2 py-0.5 text-xs text-muted-foreground outline-none transition-colors",
          "hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50",
          className,
        )}
      >
        <span
          className={cn("size-2 shrink-0 rounded-full", tone, live && "animate-pulse")}
          aria-hidden
        />
        <span className="truncate">{label}</span>
        <ChevronUpIcon className="size-3 shrink-0 opacity-60" aria-hidden />
      </TooltipTrigger>
      <TooltipContent side="top" align="start">
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
