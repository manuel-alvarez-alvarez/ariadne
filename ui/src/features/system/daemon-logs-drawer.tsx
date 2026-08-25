/**
 * Bottom drawer with a live tail of the daemon's own log, opened from the
 * footer's status button.
 *
 * The stream only exists while the drawer is open: the `EventSource` connects
 * on open and is torn down on close, so an idle app holds no log connection.
 * Every (re)connection starts from a fresh snapshot — replace, not append —
 * which `DaemonLogStream` already folds into the one `onLines` callback.
 *
 * Follows the newest line by default, and stops following the moment the user
 * scrolls up to read — scrolling back to the bottom resumes it. That is a
 * measurement, not state anybody owns, so it lives in a ref written by the
 * scroll handler rather than in React state.
 *
 * The disconnected banner mirrors {@link ConnectionBanner}'s tones: this
 * drawer is where somebody looks when the daemon misbehaves, so its own stream
 * dropping is said out loud rather than shown as a tail that quietly froze.
 */

import { PlugZapIcon } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { LogLineDto } from "@/api"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { cn } from "@/lib/format"
import { useBaseUrl } from "@/stores/settings"

import { type DaemonLogStatus, DaemonLogStream, daemonLogStreamUrl } from "./daemon-log-stream"

/**
 * Sub-pixel heights make `scrollTop` land a fraction short of the end; being
 * a pixel above the bottom still reads as "at the bottom".
 */
const FOLLOW_EPSILON = 1

const LEVEL_TINT: Record<string, string> = {
  ERROR: "text-status-danger-fg",
  WARN: "text-status-warn-fg",
  DEBUG: "text-muted-foreground",
  TRACE: "text-muted-foreground",
}

export function DaemonLogsDrawer({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const baseUrl = useBaseUrl()
  const [lines, setLines] = useState<LogLineDto[]>([])
  const [status, setStatus] = useState<DaemonLogStatus>("connecting")

  const scroller = useRef<HTMLDivElement | null>(null)
  /** Whether the view is pinned to the newest line. */
  const follow = useRef(true)

  useEffect(() => {
    if (!open) return
    // A fresh open reads from the bottom, whatever the last one was doing.
    follow.current = true
    setStatus("connecting")
    const stream = new DaemonLogStream(daemonLogStreamUrl(baseUrl), {
      onLines: setLines,
      onStatus: setStatus,
    })
    stream.start()
    return () => stream.stop()
  }, [open, baseUrl])

  // Keep the newest line in view — unless the user scrolled up to read.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `lines` is the trigger, not an input
  useEffect(() => {
    const el = scroller.current
    if (el && follow.current) el.scrollTop = el.scrollHeight
  }, [lines])

  const onScroll = () => {
    const el = scroller.current
    if (!el) return
    follow.current = el.scrollHeight - el.clientHeight - el.scrollTop <= FOLLOW_EPSILON
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="bottom" className="h-[50svh] gap-0 overflow-hidden p-0 after:hidden">
        {/* One wrapper, so the scroller escapes the popup's `*:shrink-0`. */}
        <div className="flex h-full min-h-0 flex-col">
          <SheetHeader className="border-b px-4 py-3">
            <SheetTitle>Daemon logs</SheetTitle>
            <SheetDescription className="sr-only">
              Live tail of the daemon&apos;s own log.
            </SheetDescription>
          </SheetHeader>

          {status === "reconnecting" ? (
            <div
              role="status"
              aria-live="polite"
              className="flex shrink-0 items-center gap-2 border-b bg-status-danger-soft px-4 py-1.5 text-xs text-status-danger-fg"
            >
              <PlugZapIcon className="size-3.5 shrink-0" aria-hidden />
              <span className="truncate font-medium">Log stream lost — reconnecting</span>
              <span className="hidden truncate font-mono opacity-70 sm:inline">{baseUrl}</span>
            </div>
          ) : null}

          <div
            ref={scroller}
            onScroll={onScroll}
            className="min-h-0 flex-1 overflow-y-auto bg-muted/20 px-4 py-2 font-mono text-xs leading-5"
          >
            {lines.length === 0 ? (
              <p className="py-6 text-center font-sans text-muted-foreground">
                {status === "live" ? "Nothing logged yet." : "Waiting for the daemon…"}
              </p>
            ) : (
              lines.map((line, index) => (
                // The buffer is append-only until the cap trims it from the
                // front, so an index key only ever relabels whole rows.
                // biome-ignore lint/suspicious/noArrayIndexKey: lines carry no id
                <div key={index} className="flex gap-2 whitespace-pre-wrap">
                  <span className="shrink-0 text-muted-foreground">{line.ts}</span>
                  <span className={cn("w-12 shrink-0", LEVEL_TINT[line.level])}>{line.level}</span>
                  <span className="shrink-0 text-muted-foreground">{line.target}</span>
                  <span className="min-w-0 break-all">{line.message}</span>
                </div>
              ))
            )}
          </div>
        </div>
      </SheetContent>
    </Sheet>
  )
}
