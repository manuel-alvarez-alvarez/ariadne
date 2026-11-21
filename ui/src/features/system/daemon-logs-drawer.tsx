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
import { type Host, hostBaseUrl, LOCAL_HOST, type LogLineDto, useHosts } from "@/api"
import { HostDot } from "@/components/host-dot"
import { HostLabel } from "@/components/host-label"
import { hostStatus } from "@/components/host-picker"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { useHasNamedHosts, usePickedHost } from "@/hooks/use-host-filter"
import { cn } from "@/lib/format"

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

/** `"all"` leaves the filter off; every other value narrows the stream. */
const ALL = "all"

/** The three services the daemon's telemetry pipeline can carry a record
 * for (033): this daemon, the desktop app, and the CLI. */
const SERVICES = [
  { value: ALL, label: "All services" },
  { value: "ariadned", label: "ariadned" },
  { value: "ariadne-desktop", label: "ariadne-desktop" },
  { value: "ariadne-cli", label: "ariadne-cli" },
]

const LEVELS = [
  { value: ALL, label: "All levels" },
  { value: "trace", label: "Trace" },
  { value: "debug", label: "Debug" },
  { value: "info", label: "Info" },
  { value: "warn", label: "Warn" },
  { value: "error", label: "Error" },
]

export function DaemonLogsDrawer({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { hosts } = useHosts()
  // The picker's host, or `local` under All, until the user picks another here.
  const picked = usePickedHost()
  const [choice, setChoice] = useState<Host | null>(null)
  const named = useHasNamedHosts()
  // The picked host when the app can reach it, else `local`.
  const pickedReachable = hosts.find((row) => row.name === picked)?.url != null
  const fallback = pickedReachable && picked ? picked : LOCAL_HOST
  const host = choice ?? fallback
  // Keep the host the drawer opened on, so a later outage shows its reason
  // rather than switching the stream to `local`.
  useEffect(() => {
    if (open && choice === null && picked !== null) setChoice(fallback)
  }, [open, choice, picked, fallback])
  // A pick the hosts list has not answered for yet opens no stream, rather
  // than one on `local`.
  const waiting = choice === null && picked === null
  const baseUrl = waiting ? null : hostBaseUrl(host)
  const unreachable = hosts.find((row) => row.name === host)?.unreachable ?? null
  const [lines, setLines] = useState<LogLineDto[]>([])
  const [status, setStatus] = useState<DaemonLogStatus>("connecting")
  const [service, setService] = useState(ALL)
  const [level, setLevel] = useState(ALL)

  const scroller = useRef<HTMLDivElement | null>(null)
  /** Whether the view is pinned to the newest line. */
  const follow = useRef(true)

  useEffect(() => {
    if (!open) setChoice(null)
  }, [open])

  useEffect(() => {
    if (!open) return
    // A fresh open (or a changed filter or host) reads from the bottom,
    // whatever the last one was doing, and shows nothing of it.
    follow.current = true
    setStatus("connecting")
    setLines([])
    if (baseUrl === null) return
    const url = daemonLogStreamUrl(baseUrl, {
      service: service === ALL ? undefined : service,
      level: level === ALL ? undefined : level,
    })
    const stream = new DaemonLogStream(url, {
      onLines: setLines,
      onStatus: setStatus,
    })
    stream.start()
    return () => stream.stop()
  }, [open, baseUrl, service, level])

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
          <SheetHeader className="flex-row items-center justify-between border-b px-4 py-3">
            <div>
              <SheetTitle>Daemon logs</SheetTitle>
              <SheetDescription className="sr-only">
                Live tail of the daemon&apos;s own log.
              </SheetDescription>
            </div>
            <div className="flex items-center gap-2">
              {named ? (
                <Select
                  value={host}
                  onValueChange={(value) => setChoice(value ?? LOCAL_HOST)}
                  items={hosts.map((row) => ({
                    value: row.name,
                    label: (
                      <span className="flex items-center gap-2">
                        <HostDot host={row.name} />
                        {row.name}
                      </span>
                    ),
                  }))}
                >
                  <SelectTrigger size="sm" className="w-32" aria-label="Host">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent align="end">
                    {hosts.map((row) => (
                      <SelectItem key={row.name} value={row.name} disabled={row.url === null}>
                        <span className="flex items-center gap-2">
                          <HostDot host={row.name} />
                          {row.url === null
                            ? `${row.name} (${hostStatus(row).label}: ${row.unreachable ?? ""})`
                            : row.name}
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : null}
              <Select
                value={service}
                onValueChange={(value) => setService(value ?? ALL)}
                items={SERVICES}
              >
                <SelectTrigger size="sm" className="w-40" aria-label="Filter by service">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="end">
                  {SERVICES.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                value={level}
                onValueChange={(value) => setLevel(value ?? ALL)}
                items={LEVELS}
              >
                <SelectTrigger size="sm" className="w-32" aria-label="Filter by level">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="end">
                  {LEVELS.map((item) => (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </SheetHeader>

          {baseUrl === null && !waiting ? (
            <div
              role="status"
              className="shrink-0 border-b bg-status-danger-soft px-4 py-1.5 text-xs text-status-danger-fg"
            >
              Host <HostLabel host={host} /> is not reachable{unreachable ? `: ${unreachable}` : ""}
            </div>
          ) : null}

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
