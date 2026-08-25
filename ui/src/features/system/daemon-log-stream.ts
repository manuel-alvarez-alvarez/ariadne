/**
 * `EventSource` wrapper for `GET /v1/logs/stream` — the daemon's own log.
 *
 * Like the session log stream (`features/sessions/log-stream.ts`), this is a
 * per-view connection rather than part of the shared domain stream: the
 * daemon's log only matters while the logs drawer is open, so the drawer
 * connects on open and disconnects on close.
 *
 * The protocol is a `snapshot` event carrying the daemon's ring buffer (a
 * `LogSnapshotResponse`), then a `delta` event per new line (a `LogLineDto`).
 * There is no replay and no `Last-Event-ID`: every connection starts over from
 * a fresh snapshot, which is also how a follower the daemon dropped for
 * lagging resyncs. So `snapshot` always means "replace everything shown so
 * far" — which is why this class keeps the line buffer itself and hands the
 * consumer the whole thing, instead of asking it to tell replacement from
 * append.
 *
 * The buffer is capped at [`DAEMON_LOG_LINE_CAP`] lines, oldest dropped first,
 * so a drawer left open under a chatty daemon does not grow without bound.
 *
 * Reconnecting is `@/events/reconnecting-stream`'s. The retry has to survive
 * the daemon being down, which is exactly when somebody opens this drawer.
 */

import { type LogLineDto, normalizeBaseUrl } from "@/api"
import { parsePayload, ReconnectingEventStream } from "@/events/reconnecting-stream"

export type DaemonLogStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; lines are flowing. */
  | "live"
  /** Dropped; a retry is scheduled. */
  | "reconnecting"

const MAX_BACKOFF_MS = 5_000

const SNAPSHOT_EVENT = "snapshot"
const DELTA_EVENT = "delta"

/** Most lines kept client-side; older ones are dropped as new ones arrive. */
export const DAEMON_LOG_LINE_CAP = 2_000

export interface DaemonLogStreamHandlers {
  /**
   * The retained lines after every change, oldest first. Always a fresh
   * array, so the consumer can use it as immutable state.
   */
  onLines: (lines: LogLineDto[]) => void
  onStatus: (status: DaemonLogStatus, error?: string | null) => void
}

/** URL of the daemon log stream on the given daemon. */
export function daemonLogStreamUrl(baseUrl: string): string {
  return new URL("/v1/logs/stream", `${normalizeBaseUrl(baseUrl)}/`).toString()
}

export class DaemonLogStream extends ReconnectingEventStream<DaemonLogStatus> {
  #handlers: DaemonLogStreamHandlers
  #lines: LogLineDto[] = []

  constructor(url: string, handlers: DaemonLogStreamHandlers) {
    super(() => url, {
      states: { connecting: "connecting", live: "live", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      dropped: "log stream disconnected",
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
    source.addEventListener(SNAPSHOT_EVENT, (message) => {
      const lines = parseSnapshot(message.data)
      if (lines !== undefined) this.#replace(lines)
    })

    source.addEventListener(DELTA_EVENT, (message) => {
      const line = parseLine(message.data)
      if (line !== undefined) this.#append(line)
    })
  }

  #replace(lines: LogLineDto[]): void {
    this.#lines = lines.length > DAEMON_LOG_LINE_CAP ? lines.slice(-DAEMON_LOG_LINE_CAP) : lines
    this.#handlers.onLines([...this.#lines])
  }

  #append(line: LogLineDto): void {
    this.#lines.push(line)
    if (this.#lines.length > DAEMON_LOG_LINE_CAP) {
      this.#lines.splice(0, this.#lines.length - DAEMON_LOG_LINE_CAP)
    }
    this.#handlers.onLines([...this.#lines])
  }
}

/** `{"lines": [...]}` → the well-formed lines, or `undefined` on junk. */
function parseSnapshot(raw: string): LogLineDto[] | undefined {
  const parsed = parsePayload(raw, "daemon-logs")
  if (parsed === undefined) return undefined
  const lines = (parsed as { lines?: unknown }).lines
  if (!Array.isArray(lines)) return undefined
  return lines.filter(isLogLine)
}

/** A `delta` payload → the line, or `undefined` if the payload was junk. */
function parseLine(raw: string): LogLineDto | undefined {
  const parsed = parsePayload(raw, "daemon-logs")
  if (parsed === undefined) return undefined
  return isLogLine(parsed) ? parsed : undefined
}

function isLogLine(value: unknown): value is LogLineDto {
  if (typeof value !== "object" || value === null) return false
  const { ts, level, target, message } = value as Record<string, unknown>
  return (
    typeof ts === "string" &&
    typeof level === "string" &&
    typeof target === "string" &&
    typeof message === "string"
  )
}
