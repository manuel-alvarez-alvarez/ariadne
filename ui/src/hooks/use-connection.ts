/**
 * Is the daemon reachable, and which daemon is it?
 *
 * There is one link to answer that with — the domain event stream — and it
 * answers all of it: the stream being open *is* the connection, and the daemon
 * names itself in the `heartbeat` it sends on that stream. Nothing here asks
 * the daemon anything, so an idle window makes no requests at all; the uptime
 * ticks off the shared clock rather than off a probe.
 */

import { retryDomainStream } from "@/events/handle"
import { useNow } from "@/hooks/use-now"
import { useBaseUrl } from "@/stores/settings"
import { useStreamStore } from "@/stores/stream"

export type ConnectionStatus = "connecting" | "connected" | "disconnected"

export interface Connection {
  status: ConnectionStatus
  /** Daemon base URL currently configured. */
  baseUrl: string
  /** e.g. `"0.1.0"`, from the last heartbeat. */
  version: string | null
  /** Daemon uptime in seconds, counted from the `started_at` it reported. */
  uptimeSecs: number | null
  /** Why the connection dropped, when the browser said. */
  error: string | null
  /** Reconnect now instead of waiting out the backoff. */
  retry: () => void
}

export function useConnection(): Connection {
  const baseUrl = useBaseUrl()
  const status = useStreamStore((state) => state.status)
  const daemon = useStreamStore((state) => state.daemon)
  const lastError = useStreamStore((state) => state.lastError)
  const now = useNow()

  const connected = status === "open"

  return {
    // `idle` is the tick before the provider's effect runs, and it is a
    // connection about to be attempted, not one that failed.
    status: connected ? "connected" : status === "reconnecting" ? "disconnected" : "connecting",
    baseUrl,
    version: daemon?.version ?? null,
    // Only while connected: a counter still running for a daemon we have lost
    // would be claiming an uptime nobody can vouch for.
    uptimeSecs: connected && daemon ? uptimeSecs(daemon.startedAt, now) : null,
    error: connected ? null : lastError,
    retry: () => retryDomainStream("retry requested"),
  }
}

/** Seconds since `startedAt`, or null if the daemon sent a date we cannot read. */
function uptimeSecs(startedAt: string, now: number): number | null {
  const started = Date.parse(startedAt)
  if (Number.isNaN(started)) return null
  return Math.max(0, Math.round((now - started) / 1000))
}
