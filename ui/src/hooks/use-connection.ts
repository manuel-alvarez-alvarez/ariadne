/**
 * Is the daemon reachable, and which version is it?
 *
 * `/v1/health` is polled (it is the cheapest endpoint the daemon has) and
 * `/v1/version` is fetched alongside it, so the shell can show both the state
 * of the link and what it is talking to.
 */

import { useQuery } from "@tanstack/react-query"

import { type ApiError, healthQueryOptions, versionQueryOptions } from "@/api"
import { useBaseUrl } from "@/stores/settings"
import { type StreamStatus, useStreamStore } from "@/stores/stream"

export type ConnectionStatus = "connecting" | "connected" | "disconnected"

export interface Connection {
  status: ConnectionStatus
  /** Daemon base URL currently configured. */
  baseUrl: string
  /** e.g. `"0.1.0"`, once `/v1/version` answered. */
  version: string | null
  /** Daemon uptime in seconds, from the last successful health probe. */
  uptimeSecs: number | null
  /** Why the last probe failed, when it did. */
  error: ApiError | null
  /** State of the domain-event stream, which is reported separately. */
  streamStatus: StreamStatus
  refetch: () => void
}

export function useConnection(): Connection {
  const baseUrl = useBaseUrl()
  const streamStatus = useStreamStore((state) => state.status)

  const health = useQuery(healthQueryOptions())
  const version = useQuery({ ...versionQueryOptions(), enabled: health.isSuccess })

  const status: ConnectionStatus = health.isSuccess
    ? "connected"
    : health.isError
      ? "disconnected"
      : "connecting"

  return {
    status,
    baseUrl,
    version: version.data?.version ?? null,
    uptimeSecs: health.data?.uptime_secs ?? null,
    error: (health.error as ApiError | null) ?? null,
    streamStatus,
    refetch: () => {
      void health.refetch()
      void version.refetch()
    },
  }
}
