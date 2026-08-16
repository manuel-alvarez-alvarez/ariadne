/**
 * `EventSource` wrapper for `GET /v1/sessions/{id}/logs/stream`.
 *
 * This is the one place a screen opens a connection of its own. The app-wide
 * rule — one `EventSource`, in `src/events`, feeding the query cache — is about
 * the *domain* event stream, which is a single shared subscription. Terminal
 * output is per session, is not cacheable state (it is a byte stream written
 * into xterm as it arrives), and only exists while somebody is looking at that
 * session, so it is opened and closed with the view.
 *
 * The protocol is a `resize` event carrying the pane's grid (`{"cols": 80,
 * "rows": 24}`) — the size the snapshot is wrapped at and every later repaint
 * is addressed in, repeated whenever the pane is resized under us — then one
 * `snapshot` event carrying the scrollback, then a `delta` event per burst of
 * new output — both `{"chunk": "..."}`, raw terminal bytes JSON-encoded so
 * escape sequences survive SSE's line framing — and a final `end` event when
 * the session is over, after which the daemon closes the connection.
 *
 * A session that is already over has no pane left to measure, so its stream
 * opens straight at the snapshot and `onResize` never fires.
 *
 * There is no replay and no `Last-Event-ID`: every connection starts from a
 * fresh snapshot. So a reconnect is not a resumption — the consumer has to
 * clear whatever it had and write the new snapshot, which is why `onSnapshot`
 * is a separate handler from `onDelta`.
 *
 * As with the domain stream, the browser's own reconnect is not used: it
 * cannot be told apart from a clean end, and it would silently splice a new
 * snapshot onto the old output.
 */

import { normalizeBaseUrl } from "@/api"

export type SessionLogStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; output is flowing. */
  | "live"
  /** Dropped mid-session; a retry is scheduled. */
  | "reconnecting"
  /** The daemon said the session is over. Nothing more is coming. */
  | "ended"

const INITIAL_BACKOFF_MS = 500
const MAX_BACKOFF_MS = 5_000

export const RESIZE_EVENT = "resize"
export const SNAPSHOT_EVENT = "snapshot"
export const DELTA_EVENT = "delta"
export const END_EVENT = "end"

/** The pane's grid, in cells. */
export interface PaneSize {
  cols: number
  rows: number
}

export interface SessionLogStreamHandlers {
  /**
   * The grid the output that follows was drawn for. Fires before the
   * snapshot, and again whenever the pane is resized under the stream.
   */
  onResize: (size: PaneSize) => void
  /** The connection's opening scrollback: replaces everything shown so far. */
  onSnapshot: (chunk: string) => void
  /** Output written since the last message: append it. */
  onDelta: (chunk: string) => void
  /** The session is over; the stream is closed and will not reconnect. */
  onEnd: () => void
  onStatus: (status: SessionLogStatus, error?: string | null) => void
}

/** URL of one session's log stream on the given daemon. */
export function sessionLogStreamUrl(baseUrl: string, sessionId: string): string {
  return new URL(
    `/v1/sessions/${encodeURIComponent(sessionId)}/logs/stream`,
    `${normalizeBaseUrl(baseUrl)}/`,
  ).toString()
}

export class SessionLogStream {
  #url: string
  #handlers: SessionLogStreamHandlers
  #source: EventSource | null = null
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #backoff = INITIAL_BACKOFF_MS
  #stopped = true
  /** Set by the `end` event: the session is over, so no retry is warranted. */
  #ended = false

  constructor(url: string, handlers: SessionLogStreamHandlers) {
    this.#url = url
    this.#handlers = handlers
  }

  /** Connect, and keep reconnecting until `stop()` or an `end` event. */
  start(): void {
    if (!this.#stopped) return
    this.#stopped = false
    this.#ended = false
    this.#connect(true)
  }

  /** Disconnect and cancel any pending retry. Idempotent. */
  stop(): void {
    this.#stopped = true
    this.#clearRetry()
    this.#closeSource()
  }

  /**
   * Start over from a fresh snapshot: what the "reconnect" control does once a
   * stream has ended, and the only way to re-read a finished session's log.
   */
  restart(): void {
    this.stop()
    this.start()
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

    source.addEventListener(RESIZE_EVENT, (message) => {
      const size = parseSize(message.data)
      if (size !== undefined) this.#handlers.onResize(size)
    })

    source.addEventListener(SNAPSHOT_EVENT, (message) => {
      const chunk = parseChunk(message.data)
      if (chunk !== undefined) this.#handlers.onSnapshot(chunk)
    })

    source.addEventListener(DELTA_EVENT, (message) => {
      const chunk = parseChunk(message.data)
      if (chunk !== undefined) this.#handlers.onDelta(chunk)
    })

    source.addEventListener(END_EVENT, () => {
      // The daemon closes the connection right after this. Close first, so its
      // hang-up does not look like a drop worth reconnecting to.
      this.#ended = true
      this.stop()
      this.#handlers.onStatus("ended", null)
      this.#handlers.onEnd()
    })
  }

  #scheduleRetry(error: string): void {
    if (this.#stopped || this.#ended || this.#retryTimer !== null) return
    this.#closeSource()
    this.#handlers.onStatus("reconnecting", error)
    // Jitter so several open session windows do not all hit a restarting
    // daemon on the same tick.
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

/** `{"chunk": "..."}` → the chunk, or `undefined` if the payload was junk. */
function parseChunk(raw: string): string | undefined {
  const parsed = parsePayload(raw)
  if (parsed === undefined) return undefined
  const chunk = (parsed as { chunk?: unknown }).chunk
  return typeof chunk === "string" ? chunk : undefined
}

/**
 * `{"cols": 80, "rows": 24}` → the grid. A size that is not two positive
 * numbers is dropped rather than resized to: rendering at the size we already
 * have is wrong, but rendering at zero columns is worse.
 */
function parseSize(raw: string): PaneSize | undefined {
  const parsed = parsePayload(raw)
  if (parsed === undefined) return undefined
  const { cols, rows } = parsed as { cols?: unknown; rows?: unknown }
  if (typeof cols !== "number" || typeof rows !== "number") return undefined
  if (!Number.isFinite(cols) || !Number.isFinite(rows) || cols < 1 || rows < 1) return undefined
  return { cols, rows }
}

function parsePayload(raw: string): object | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    console.error("[session-logs] dropping unparseable payload", cause, raw)
    return undefined
  }
  if (typeof parsed !== "object" || parsed === null) return undefined
  return parsed
}
