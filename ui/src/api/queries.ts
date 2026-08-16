/**
 * Shared query definitions for the daemon's system endpoints.
 *
 * The health probe has more than one observer — the connection indicator shows
 * it, the event stream uses it as a liveness watchdog — so its options live
 * here rather than being restated at each call site, which would give them
 * different polling and retry behaviour for the same key.
 */

import { queryOptions } from "@tanstack/react-query"

import { api, unwrap } from "./client"
import { qk } from "./query-keys"

/** How often the health probe runs while the window is focused. */
export const HEALTH_POLL_MS = 10_000

export function healthQueryOptions() {
  return queryOptions({
    queryKey: qk.system.health(),
    queryFn: () => unwrap(api().GET("/v1/health")),
    refetchInterval: HEALTH_POLL_MS,
    // Keep probing while the daemon is down, but do not let a retry backoff
    // stretch out how long "disconnected" takes to show up.
    refetchIntervalInBackground: true,
    retry: false,
    // A stale "connected" badge is worse than a brief "connecting" one.
    gcTime: 0,
    staleTime: 0,
  })
}

export function versionQueryOptions() {
  return queryOptions({
    queryKey: qk.system.version(),
    queryFn: () => unwrap(api().GET("/v1/version")),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  })
}
