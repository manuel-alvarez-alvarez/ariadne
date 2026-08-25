/**
 * `EventSource` wrapper for `GET /v1/events/stream`.
 *
 * The daemon sends each domain event with its kind as the SSE `event:` name and
 * the bare DTO as `data:`, plus one control event, `resync`, sent just before
 * the daemon hangs up on a client that fell behind.
 *
 * Reconnecting is `ReconnectingEventStream`'s; what is here is the protocol —
 * and the reason the UI has to be told a gap opened at all, which is that there
 * is no replay: a full refetch is the only way back.
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

import { parsePayload, ReconnectingEventStream } from "./reconnecting-stream"

/**
 * Every event kind the daemon can send. Declared as a total record over
 * `DomainEventKind` so a new kind in the generated types fails to compile here
 * until it is listed (and handled in `dispatch.ts`).
 */
const DOMAIN_EVENT_KINDS_PRESENT: Record<DomainEventKind, true> = {
  goal_created: true,
  goal_updated: true,
  goal_deleted: true,
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
  repository_created: true,
  repository_updated: true,
  repository_deleted: true,
}

const DOMAIN_EVENT_KINDS = Object.keys(DOMAIN_EVENT_KINDS_PRESENT) as DomainEventKind[]

/** SSE `event:` name of the control event closing a lagged connection. */
const RESYNC_EVENT = "resync"

/** A shared subscription, so it waits longer than a per-view stream would. */
const MAX_BACKOFF_MS = 10_000

export interface DomainEventStreamHandlers {
  /** A domain event arrived, already parsed into its tagged-union shape. */
  onEvent: (event: DomainEvent) => void
  /** The daemon dropped events for this connection and is closing it. */
  onResync: (payload: ResyncDto) => void
  /**
   * The stream opened. `reconnected` is true for any open that follows a failed
   * attempt or a forced drop — including a first one, because REST queries may
   * well have loaded during the gap and events since then are gone for good.
   */
  onOpen: (info: { reconnected: boolean }) => void
  /** Status changes worth showing in the UI. */
  onStatus: (status: StreamStatus, error?: string | null) => void
}

export class DomainEventStream extends ReconnectingEventStream<StreamStatus> {
  #handlers: DomainEventStreamHandlers

  constructor(url: () => string, handlers: DomainEventStreamHandlers) {
    super(url, {
      // There is no "live": the connection indicator says connected off the
      // health probe, and this stream only reports the two unhappy states.
      states: { connecting: "connecting", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      onOpen: handlers.onOpen,
      dropped: "event stream disconnected",
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
    for (const kind of DOMAIN_EVENT_KINDS) {
      source.addEventListener(kind, (message) => {
        const data = parsePayload(message.data, `events:${kind}`)
        if (data === undefined) return
        // The wire splits the union: the kind is the SSE event name and the
        // payload is the bare DTO. Put them back together.
        this.#handlers.onEvent({ event: kind, data } as DomainEvent)
      })
    }

    source.addEventListener(RESYNC_EVENT, (message) => {
      const data = parsePayload(message.data, `events:${RESYNC_EVENT}`)
      if (data === undefined) return
      this.#handlers.onResync(data as ResyncDto)
    })
  }
}
