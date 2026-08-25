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
 * is addressed in — then a `snapshot` event carrying the scrollback, then a
 * `delta` event per burst of new output — both `{"chunk": "..."}`, raw
 * terminal bytes JSON-encoded so escape sequences survive SSE's line framing
 * — and a final `end` event when the session is over, after which the daemon
 * closes the connection.
 *
 * A pane resized under the stream sends another `resize`, followed by a fresh
 * `snapshot`: the output in flight was drawn partly at each size and belongs
 * to neither, so the daemon starts the client over rather than splicing it.
 * `snapshot` therefore always means "replace everything shown so far",
 * whenever it arrives — which is what `onSnapshot` already had to mean for
 * reconnects. If the daemon cannot get a screen at the new size it closes the
 * connection instead of sending output the client cannot place; that arrives
 * here as a drop, and the reconnect below is the recovery.
 *
 * A session that ended before it was ever measured has no grid to report, and
 * `onResize` never fires; the consumer draws at whatever default it has.
 *
 * There is no replay and no `Last-Event-ID`: every connection starts from a
 * fresh snapshot. So a reconnect is not a resumption — the consumer has to
 * clear whatever it had and write the new snapshot, which is why `onSnapshot`
 * is a separate handler from `onDelta`.
 *
 * Reconnecting is `@/events/reconnecting-stream`'s, and the browser's own is
 * not used for the reason given there — with one addition of its own: `end`
 * means the session is over, so the daemon's hang-up right after it must not
 * look like a drop worth retrying.
 */

import { normalizeBaseUrl } from "@/api"
import { parsePayload, ReconnectingEventStream } from "@/events/reconnecting-stream"

export type SessionLogStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; output is flowing. */
  | "live"
  /** Dropped mid-session; a retry is scheduled. */
  | "reconnecting"
  /** The daemon said the session is over. Nothing more is coming. */
  | "ended"

const MAX_BACKOFF_MS = 5_000

const RESIZE_EVENT = "resize"
const SNAPSHOT_EVENT = "snapshot"
const DELTA_EVENT = "delta"
const END_EVENT = "end"

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
  /** Scrollback drawn at the last reported grid: replaces everything shown. */
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

export class SessionLogStream extends ReconnectingEventStream<SessionLogStatus> {
  #handlers: SessionLogStreamHandlers

  constructor(url: string, handlers: SessionLogStreamHandlers) {
    super(() => url, {
      states: { connecting: "connecting", live: "live", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      dropped: "log stream disconnected",
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
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
      this.halt()
      this.#handlers.onStatus("ended", null)
      this.#handlers.onEnd()
    })
  }
}

/** `{"chunk": "..."}` → the chunk, or `undefined` if the payload was junk. */
function parseChunk(raw: string): string | undefined {
  const parsed = parsePayload(raw, "session-logs")
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
  const parsed = parsePayload(raw, "session-logs")
  if (parsed === undefined) return undefined
  const { cols, rows } = parsed as { cols?: unknown; rows?: unknown }
  if (typeof cols !== "number" || typeof rows !== "number") return undefined
  if (!Number.isFinite(cols) || !Number.isFinite(rows) || cols < 1 || rows < 1) return undefined
  return { cols, rows }
}
