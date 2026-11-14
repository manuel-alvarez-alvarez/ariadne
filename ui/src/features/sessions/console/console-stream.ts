/**
 * `EventSource` wrapper for `GET /v1/sessions/{id}/console/stream`: one
 * session's console, for as long as its pane is on screen.
 *
 * The daemon opens with a `snapshot` — the newest page of stored events,
 * oldest first, then the running turn's text so far — and sends an `event`
 * per later one, live chunks and tool progress included. There is no replay:
 * every connection starts over from a fresh snapshot, so a snapshot always
 * replaces what the pane shows. A client that falls behind gets a `resync`
 * and the daemon hangs up; that, like a drop, is retried on the backoff, and
 * the next connection's snapshot is the way back.
 */

import { type AgentEventDto, normalizeBaseUrl } from "@/api"
import { parsePayload, ReconnectingEventStream } from "@/events/reconnecting-stream"

export type ConsoleStreamStatus = "connecting" | "live" | "reconnecting"

const MAX_BACKOFF_MS = 5_000

interface ConsoleStreamHandlers {
  /** The stored transcript a connection opens on, oldest first. */
  onSnapshot: (events: AgentEventDto[]) => void
  onEvent: (event: AgentEventDto) => void
  onStatus: (status: ConsoleStreamStatus, error?: string | null) => void
}

/** URL of a session's console stream on the given daemon. */
export function consoleStreamUrl(baseUrl: string, sessionId: string): string {
  return `${normalizeBaseUrl(baseUrl)}/v1/sessions/${encodeURIComponent(sessionId)}/console/stream`
}

export class ConsoleStream extends ReconnectingEventStream<ConsoleStreamStatus> {
  #handlers: ConsoleStreamHandlers

  constructor(url: string, handlers: ConsoleStreamHandlers) {
    super(() => url, {
      states: { connecting: "connecting", live: "live", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      dropped: "console stream disconnected",
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
    source.addEventListener("snapshot", (message) => {
      let parsed: unknown
      try {
        parsed = JSON.parse(message.data)
      } catch (cause) {
        console.error("[console] dropping unparseable snapshot", cause)
        return
      }
      if (Array.isArray(parsed)) this.#handlers.onSnapshot(parsed.filter(isEvent))
    })

    source.addEventListener("event", (message) => {
      const event = parsePayload(message.data, "console")
      if (isEvent(event)) this.#handlers.onEvent(event)
    })

    source.addEventListener("resync", () => this.retry("resync"))
  }
}

function isEvent(value: unknown): value is AgentEventDto {
  if (typeof value !== "object" || value === null) return false
  const { id, kind } = value as Record<string, unknown>
  return typeof id === "string" && typeof kind === "string"
}
