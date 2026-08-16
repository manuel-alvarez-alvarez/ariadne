/**
 * `EventSource` wrapper for `GET /v1/events/stream`.
 *
 * The daemon sends each domain event with its kind as the SSE `event:` name and
 * the bare DTO as `data:`, plus one control event, `resync`, sent just before
 * the daemon hangs up on a client that fell behind.
 *
 * The browser's own reconnect is deliberately not used: this wrapper closes the
 * source on error and reconnects with capped exponential backoff, so the UI can
 * report an honest status and run a full refetch whenever a gap opened.
 */

import type { DomainEvent, DomainEventKind, ResyncDto } from "@/api"
import type { StreamStatus } from "@/stores/stream"

/**
 * Every event kind the daemon can send. Declared as a total record over
 * `DomainEventKind` so a new kind in the generated types fails to compile here
 * until it is listed (and handled in `dispatch.ts`).
 */
const DOMAIN_EVENT_KINDS_PRESENT: Record<DomainEventKind, true> = {
  goal_created: true,
  goal_updated: true,
  task_created: true,
  task_updated: true,
  message_created: true,
  review_created: true,
  session_created: true,
  session_updated: true,
  agent_event: true,
  profile_created: true,
  profile_updated: true,
  profile_deleted: true,
}

export const DOMAIN_EVENT_KINDS = Object.keys(DOMAIN_EVENT_KINDS_PRESENT) as DomainEventKind[]

/** SSE `event:` name of the control event closing a lagged connection. */
export const RESYNC_EVENT = "resync"

const INITIAL_BACKOFF_MS = 500
const MAX_BACKOFF_MS = 10_000

export interface DomainEventStreamHandlers {
  /** A domain event arrived, already parsed into its tagged-union shape. */
  onEvent: (event: DomainEvent) => void
  /** The daemon dropped events for this connection and is closing it. */
  onResync: (payload: ResyncDto) => void
  /** The stream opened. `reconnected` is false only for the very first open. */
  onOpen: (info: { reconnected: boolean }) => void
  /** Status changes worth showing in the UI. */
  onStatus: (status: StreamStatus, error?: string | null) => void
}

export class DomainEventStream {
  #url: () => string
  #handlers: DomainEventStreamHandlers
  #source: EventSource | null = null
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #backoff = INITIAL_BACKOFF_MS
  #everOpened = false
  #stopped = true

  constructor(url: () => string, handlers: DomainEventStreamHandlers) {
    this.#url = url
    this.#handlers = handlers
  }

  /** Connect, and keep reconnecting until `stop()`. Idempotent. */
  start(): void {
    if (!this.#stopped) return
    this.#stopped = false
    this.#connect()
  }

  /** Disconnect and cancel any pending retry. Idempotent. */
  stop(): void {
    this.#stopped = true
    this.#clearRetry()
    this.#closeSource()
  }

  #connect(): void {
    if (this.#stopped) return
    this.#closeSource()
    this.#handlers.onStatus(this.#everOpened ? "reconnecting" : "connecting")

    let source: EventSource
    try {
      source = new EventSource(this.#url())
    } catch (cause) {
      this.#scheduleRetry(cause instanceof Error ? cause.message : String(cause))
      return
    }
    this.#source = source

    source.onopen = () => {
      this.#backoff = INITIAL_BACKOFF_MS
      const reconnected = this.#everOpened
      this.#everOpened = true
      this.#handlers.onOpen({ reconnected })
    }

    // `EventSource` reports every failure as an opaque error event, so there is
    // nothing more specific to surface than "the connection dropped".
    source.onerror = () => {
      this.#scheduleRetry("event stream disconnected")
    }

    for (const kind of DOMAIN_EVENT_KINDS) {
      source.addEventListener(kind, (message) => {
        const data = parseJson(message.data, kind)
        if (data === undefined) return
        // The wire splits the union: the kind is the SSE event name and the
        // payload is the bare DTO. Put them back together.
        this.#handlers.onEvent({ event: kind, data } as DomainEvent)
      })
    }

    source.addEventListener(RESYNC_EVENT, (message) => {
      const data = parseJson(message.data, RESYNC_EVENT)
      if (data === undefined) return
      this.#handlers.onResync(data as ResyncDto)
    })
  }

  #scheduleRetry(error: string): void {
    if (this.#stopped || this.#retryTimer !== null) return
    this.#closeSource()
    this.#handlers.onStatus("reconnecting", error)
    // Jitter so a daemon restart does not get a thundering herd of windows.
    const delay = this.#backoff * (0.5 + Math.random() / 2)
    this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS)
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      this.#connect()
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

function parseJson(raw: string, kind: string): unknown {
  try {
    return JSON.parse(raw)
  } catch (cause) {
    console.error(`[events] dropping unparseable ${kind} payload`, cause, raw)
    return undefined
  }
}
