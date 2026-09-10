/**
 * `EventSource` wrapper for `GET /v1/sessions/{id}/console/stream` — the
 * counterpart of `log-stream.ts` for a session with no pane at all.
 *
 * The protocol is a `snapshot` event carrying every event the session has
 * recorded so far (`AgentEventDto[]`, oldest first), then an `event` per later
 * one (a bare `AgentEventDto`). There is no grid and no resize: an `acp`
 * session's console is a feed of events, not a byte stream drawn into a grid.
 *
 * There is no replay and no `Last-Event-ID`, so — exactly as the log stream —
 * every connection starts from a fresh snapshot: a reconnect replaces what is
 * shown rather than resuming it. A client that falls too far behind gets a
 * final `resync` event and the daemon closes the connection right after it;
 * that read as an ordinary drop and is left to the normal retry, since the
 * fresh snapshot the retry opens with is already the correct recovery.
 */

import type { AgentEventDto } from "@/api"
import { normalizeBaseUrl } from "@/api"
import { parsePayload, ReconnectingEventStream } from "@/events/reconnecting-stream"

export type ConsoleStreamStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; events are flowing. */
  | "live"
  /** Dropped mid-session; a retry is scheduled. */
  | "reconnecting"

const MAX_BACKOFF_MS = 5_000

const SNAPSHOT_EVENT = "snapshot"
const EVENT_EVENT = "event"

export interface ConsoleStreamHandlers {
  /** Everything recorded so far, oldest first: replaces whatever is shown. */
  onSnapshot: (events: AgentEventDto[]) => void
  /** One event recorded since the last message: append it. */
  onEvent: (event: AgentEventDto) => void
  onStatus: (status: ConsoleStreamStatus, error?: string | null) => void
}

/** URL of one session's console stream on the given daemon. */
export function consoleStreamUrl(baseUrl: string, sessionId: string): string {
  return new URL(
    `/v1/sessions/${encodeURIComponent(sessionId)}/console/stream`,
    `${normalizeBaseUrl(baseUrl)}/`,
  ).toString()
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
    source.addEventListener(SNAPSHOT_EVENT, (message) => {
      const events = parseEvents(message.data)
      if (events !== undefined) this.#handlers.onSnapshot(events)
    })

    source.addEventListener(EVENT_EVENT, (message) => {
      const event = parsePayload(message.data, "console-stream")
      if (event !== undefined) this.#handlers.onEvent(event as AgentEventDto)
    })
  }
}

/** A `snapshot` payload: a bare JSON array of `AgentEventDto`. */
function parseEvents(raw: string): AgentEventDto[] | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    console.error("[console-stream] dropping unparseable snapshot", cause, raw)
    return undefined
  }
  return Array.isArray(parsed) ? (parsed as AgentEventDto[]) : undefined
}
