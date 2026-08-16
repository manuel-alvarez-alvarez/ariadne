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
 *
 * `EventSource` is not enough on its own to notice a dead daemon. A socket can
 * stay open long after the daemon stopped answering — `ariadned` in particular
 * keeps an SSE connection alive through its own shutdown (see `forceReconnect`
 * callers) — and then no `error` ever fires and the UI goes quietly stale. So
 * the stream also accepts an outside liveness signal: the REST health probe
 * calls [`forceReconnect`] when it stops getting answers.
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
  /**
   * The stream opened.
   *
   * `reconnected` is false only for a first connection that succeeded straight
   * away, where nothing can have been missed. Any open that follows a failed
   * attempt or a forced drop reports true — including the first one, because
   * REST queries may well have loaded during the gap and events since then are
   * gone for good.
   */
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
  /**
   * Whether anything went wrong since the last successful open: a failed
   * attempt, or a drop forced from outside. This — not `#everOpened` — is what
   * makes an open count as a reconnect, because a first connection that only
   * came up after a few failures had a gap just the same.
   */
  #interrupted = false
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

  /** True while a source is open and delivering. */
  get isOpen(): boolean {
    return this.#source?.readyState === EventSource.OPEN
  }

  /**
   * Drop the current connection and reconnect straight away.
   *
   * For when something outside the stream knows it is dead — the health probe
   * losing the daemon — because a socket can sit in `OPEN` indefinitely without
   * `error` ever firing.
   */
  forceReconnect(reason: string): void {
    if (this.#stopped) return
    this.#clearRetry()
    this.#closeSource()
    this.#backoff = INITIAL_BACKOFF_MS
    // Whatever the old connection was doing, we are not sure we saw all of it.
    this.#interrupted = true
    this.#connect(reason)
  }

  /**
   * Retry now instead of waiting out the backoff — but only when nothing is in
   * flight, so this never interrupts a connection that is still being made.
   */
  reconnectIfClosed(reason: string): void {
    if (this.#stopped) return
    if (this.#source !== null && this.#source.readyState !== EventSource.CLOSED) return
    this.forceReconnect(reason)
  }

  #connect(reason?: string): void {
    if (this.#stopped) return
    this.#closeSource()
    const retrying = this.#everOpened || this.#interrupted
    this.#handlers.onStatus(retrying ? "reconnecting" : "connecting", reason)

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
      const reconnected = this.#everOpened || this.#interrupted
      this.#everOpened = true
      this.#interrupted = false
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
    // An attempt failed, so the next open is a reconnect even if no attempt has
    // ever succeeded: events during the gap are gone, there is no replay.
    this.#interrupted = true
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
