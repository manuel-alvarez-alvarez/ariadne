/**
 * The daemon behind a host, as the last heartbeat of the app's stream to it
 * said: its version, and its uptime while that stream is open.
 */

import { LOCAL_HOST } from "@/api"
import { uptimeSecs } from "@/hooks/use-connection"
import { useNow } from "@/hooks/use-now"
import { useHostStreamStore, useStreamStore } from "@/stores/stream"

interface HostDaemon {
  version: string
  uptimeSecs: number | null
}

/** Null until a heartbeat has arrived. */
export function useHostDaemon(host: string): HostDaemon | null {
  const local = useStreamStore((state) => (host === LOCAL_HOST ? state : null))
  const named = useHostStreamStore((state) => state.streams[host])
  const now = useNow()
  const stream = host === LOCAL_HOST ? local : named
  if (!stream?.daemon) return null
  return {
    version: stream.daemon.version,
    // Only while the stream is open: a counter for a daemon the app has lost
    // would claim an uptime nobody can vouch for.
    uptimeSecs: stream.status === "open" ? uptimeSecs(stream.daemon.startedAt, now) : null,
  }
}
