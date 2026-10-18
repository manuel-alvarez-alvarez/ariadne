/**
 * Global "is this window looking at a live daemon?" indicator, lived in by the
 * sidebar's own foot — its last child, below the navigation.
 *
 * There is one link and one readout: the domain-event stream, which is both how
 * the screens stay live and how the daemon says who it is. Green means the
 * stream is open and the daemon is beating; red means it is not, and then
 * nothing on screen is being kept up to date either.
 *
 * It renders as a real button because the daemon-logs drawer hangs off the
 * click — `onOpenLogs`, wired by the shell to the drawer it mounts.
 *
 * `collapsed` is the rail's own version: the dot alone, named by a tooltip
 * instead of by the label and chevron there is no room for.
 */

import { ChevronUpIcon } from "lucide-react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useConnection } from "@/hooks/use-connection"
import { cn, formatDuration } from "@/lib/format"
export function ConnectionStatus({
  className,
  onOpenLogs,
  collapsed = false,
}: {
  className?: string
  /** What the click opens: the shell's daemon-logs drawer. */
  onOpenLogs?: () => void
  /** The rail: the dot alone, with a tooltip naming the status. */
  collapsed?: boolean
}) {
  const { status, baseUrl, version, uptimeSecs, error } = useConnection()

  const live = status === "connected"
  // Straight off the status ramp: green while the stream is up, the warn step
  // while it is still being opened, the danger step once it is gone.
  const tone = live
    ? "bg-status-done"
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

  const dot = (
    <span
      className={cn("size-2 shrink-0 rounded-full", tone, live && "animate-pulse")}
      aria-hidden
    />
  )

  if (collapsed) {
    return (
      <Tooltip>
        <TooltipTrigger
          aria-label={`Daemon status: ${label}`}
          render={<button type="button" onClick={onOpenLogs} />}
          className={cn(
            "flex items-center justify-center rounded-md p-1.5 text-muted-foreground outline-none transition-colors",
            "hover:bg-muted hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50",
            className,
          )}
        >
          {dot}
        </TooltipTrigger>
        <TooltipContent side="right">{label}</TooltipContent>
      </Tooltip>
    )
  }

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
        {dot}
        <span className="truncate">{label}</span>
        <ChevronUpIcon className="size-3 shrink-0 opacity-60" aria-hidden />
      </TooltipTrigger>
      <TooltipContent side="top" align="start">
        <div className="space-y-1">
          <p className="font-mono text-xs">{baseUrl}</p>
          <p>
            Daemon: {status}
            {uptimeSecs !== null ? ` · up ${formatDuration(uptimeSecs)}` : ""}
          </p>
          {error ? <p className="text-destructive">{error}</p> : null}
        </div>
      </TooltipContent>
    </Tooltip>
  )
}
