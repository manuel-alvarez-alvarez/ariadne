/**
 * `EventSource` wrapper for `GET /v1/sessions/{id}/trajectory/stream` (040):
 * one session's turns, for as long as its console is on screen.
 *
 * The daemon opens with a `snapshot` and sends an `update` per revision. A
 * client that falls behind gets a `resync` and the daemon hangs up; that,
 * like a drop, is retried on the backoff, and the next snapshot replaces
 * what the console shows.
 */

import { normalizeBaseUrl, type TrajectorySnapshotDto, type TrajectoryUpdateDto } from "@/api"
import { parsePayload, ReconnectingEventStream } from "@/events/reconnecting-stream"

import type { ConsoleStreamStatus } from "../console/console-stream"

const MAX_BACKOFF_MS = 5_000

interface TrajectoryStreamHandlers {
  onSnapshot: (snapshot: TrajectorySnapshotDto) => void
  onUpdate: (update: TrajectoryUpdateDto) => void
  onStatus: (status: ConsoleStreamStatus, error?: string | null) => void
}

/** URL of a session's trajectory stream on the given daemon. */
export function trajectoryStreamUrl(baseUrl: string, sessionId: string): string {
  return `${normalizeBaseUrl(baseUrl)}/v1/sessions/${encodeURIComponent(sessionId)}/trajectory/stream`
}

export class TrajectoryStream extends ReconnectingEventStream<ConsoleStreamStatus> {
  #handlers: TrajectoryStreamHandlers

  constructor(url: string, handlers: TrajectoryStreamHandlers) {
    super(() => url, {
      states: { connecting: "connecting", live: "live", reconnecting: "reconnecting" },
      maxBackoffMs: MAX_BACKOFF_MS,
      onStatus: handlers.onStatus,
      dropped: "trajectory stream disconnected",
    })
    this.#handlers = handlers
  }

  protected listen(source: EventSource): void {
    source.addEventListener("snapshot", (message) => {
      const snapshot = parsePayload(message.data, "trajectory")
      if (isSnapshot(snapshot)) this.#handlers.onSnapshot(snapshot)
    })
    source.addEventListener("update", (message) => {
      const update = parsePayload(message.data, "trajectory")
      if (isUpdate(update)) this.#handlers.onUpdate(update)
    })
    source.addEventListener("resync", () => this.retry("resync"))
  }
}

function isSnapshot(value: unknown): value is TrajectorySnapshotDto {
  if (typeof value !== "object" || value === null) return false
  const { rev, turns } = value as Record<string, unknown>
  return typeof rev === "number" && Array.isArray(turns)
}

function isUpdate(value: unknown): value is TrajectoryUpdateDto {
  if (typeof value !== "object" || value === null) return false
  const { rev, ops } = value as Record<string, unknown>
  return typeof rev === "number" && Array.isArray(ops)
}
