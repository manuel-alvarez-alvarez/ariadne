/**
 * `EventSource` wrapper for `GET /v1/events/stream`.
 *
 * The daemon sends each domain event with its kind as the SSE `event:` name and
 * the bare DTO as `data:`, plus two control events: `resync`, sent just before
 * the daemon hangs up on a client that fell behind, and `heartbeat`, sent as
 * the connection opens and every 15 s an idle one goes without a frame.
 *
 * Reconnecting is `ReconnectingEventStream`'s; what is here is the protocol —
 * and the reason the UI has to be told a gap opened at all, which is that there
 * is no replay: a full refetch is the only way back.
 *
 * This is the app's only link to the daemon, so it is also the only evidence
 * the daemon is there: the heartbeat's cadence arms the idle budget below, and
 * what it carries is what the connection indicator shows.
 */

import type { DomainEvent, DomainEventKind, HeartbeatDto, ResyncDto } from "@/api"
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
  task_branch_updated: true,
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

/** SSE `event:` name of the daemon's liveness beat. */
const HEARTBEAT_EVENT = "heartbeat"

/**
 * How long the stream may hear nothing before it calls the daemon gone.
 *
 * Two and a half beats of the daemon's 15 s heartbeat: one missed beat is a
 * slow moment, three consecutive ones are not. This is the app's only timer,
 * and it is the daemon's own promise that makes it sound — see
 * {@link ReconnectingEventStream}'s idle budget for why the socket cannot be
 * trusted to notice on its own.
 */
const IDLE_BUDGET_MS = 2.5 * 15_000

/** A shared subscription, so it waits longer than a per-view stream would. */
const MAX_BACKOFF_MS = 10_000

export interface DomainEventStreamHandlers {
  /** A domain event arrived, already parsed into its tagged-union shape. */
  onEvent: (event: DomainEvent) => void
  /** The daemon dropped events for this connection and is closing it. */
  onResync: (payload: ResyncDto) => void
  /**
   * The daemon said it is alive, and which daemon it is. A control event, not a
   * domain event: it never reaches the dispatcher, it only proves the link.
   */
  onHeartbeat: (payload: HeartbeatDto) => void
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
      // There is no "live" here: an open is reported through `onOpen`, which
      // records more about it than a status does, and the two would fight.
      states: { connecting: "connecting", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      onOpen: handlers.onOpen,
      dropped: "event stream disconnected",
      idle: { timeoutMs: IDLE_BUDGET_MS, reason: "no heartbeat from the daemon" },
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
    for (const kind of DOMAIN_EVENT_KINDS) {
      source.addEventListener(kind, (message) => {
        this.noteFrame()
        const data = parsePayload(message.data, `events:${kind}`)
        if (data === undefined) return
        // The wire splits the union: the kind is the SSE event name and the
        // payload is the bare DTO. Put them back together.
        this.#handlers.onEvent({ event: kind, data } as DomainEvent)
      })
    }

    source.addEventListener(RESYNC_EVENT, (message) => {
      this.noteFrame()
      const data = parsePayload(message.data, `events:${RESYNC_EVENT}`)
      if (data === undefined) return
      this.#handlers.onResync(data as ResyncDto)
    })

    source.addEventListener(HEARTBEAT_EVENT, (message) => {
      this.noteFrame()
      const data = parsePayload(message.data, `events:${HEARTBEAT_EVENT}`)
      if (data === undefined) return
      this.#handlers.onHeartbeat(data as HeartbeatDto)
    })
  }
}
