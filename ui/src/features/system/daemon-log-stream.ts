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
 * As with the other streams, the browser's own reconnect is not used: it
 * cannot be told apart from a clean close, and the retry here has to survive
 * the daemon being down — which is exactly when somebody opens this drawer.
 */

import { type LogLineDto, normalizeBaseUrl } from "@/api"

export type DaemonLogStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; lines are flowing. */
  | "live"
  /** Dropped; a retry is scheduled. */
  | "reconnecting"

const INITIAL_BACKOFF_MS = 500
const MAX_BACKOFF_MS = 5_000

export const SNAPSHOT_EVENT = "snapshot"
export const DELTA_EVENT = "delta"

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

export class DaemonLogStream {
  #url: string
  #handlers: DaemonLogStreamHandlers
  #source: EventSource | null = null
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #backoff = INITIAL_BACKOFF_MS
  #stopped = true
  #lines: LogLineDto[] = []

  constructor(url: string, handlers: DaemonLogStreamHandlers) {
    this.#url = url
    this.#handlers = handlers
  }

  /** Connect, and keep reconnecting until `stop()`. */
  start(): void {
    if (!this.#stopped) return
    this.#stopped = false
    this.#connect(true)
  }

  /** Disconnect and cancel any pending retry. Idempotent. */
  stop(): void {
    this.#stopped = true
    this.#clearRetry()
    this.#closeSource()
  }

  #connect(first: boolean): void {
    if (this.#stopped) return
    this.#closeSource()
    this.#handlers.onStatus(first ? "connecting" : "reconnecting")

    let source: EventSource
    try {
      source = new EventSource(this.#url)
    } catch (cause) {
      this.#scheduleRetry(cause instanceof Error ? cause.message : String(cause))
      return
    }
    this.#source = source

    source.onopen = () => {
      this.#backoff = INITIAL_BACKOFF_MS
      this.#handlers.onStatus("live", null)
    }

    // `EventSource` reports every failure the same opaque way, and cannot tell
    // a dropped connection from a daemon that went away — both are retried.
    source.onerror = () => {
      this.#scheduleRetry("log stream disconnected")
    }

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

  #scheduleRetry(error: string): void {
    if (this.#stopped || this.#retryTimer !== null) return
    this.#closeSource()
    this.#handlers.onStatus("reconnecting", error)
    // Jitter so several windows do not all hit a restarting daemon on the
    // same tick.
    const delay = this.#backoff * (0.5 + Math.random() / 2)
    this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS)
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      this.#connect(false)
    }, delay)
  }

  #clearRetry(): void {
    if (this.#retryTimer === null) return
    clearTimeout(this.#retryTimer)
    this.#retryTimer = null
  }

  #closeSource(): void {
    if (this.#source === null) return
    this.#source.onopen = null
    this.#source.onerror = null
    this.#source.close()
    this.#source = null
  }
}

/** `{"lines": [...]}` → the well-formed lines, or `undefined` on junk. */
function parseSnapshot(raw: string): LogLineDto[] | undefined {
  const parsed = parsePayload(raw)
  if (parsed === undefined) return undefined
  const lines = (parsed as { lines?: unknown }).lines
  if (!Array.isArray(lines)) return undefined
  return lines.filter(isLogLine)
}

/** A `delta` payload → the line, or `undefined` if the payload was junk. */
function parseLine(raw: string): LogLineDto | undefined {
  const parsed = parsePayload(raw)
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

function parsePayload(raw: string): object | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    console.error("[daemon-logs] dropping unparseable payload", cause, raw)
    return undefined
  }
  if (typeof parsed !== "object" || parsed === null) return undefined
  return parsed
}
