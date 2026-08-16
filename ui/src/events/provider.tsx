/**
 * Wires the single app-wide domain-event stream to the query cache.
 *
 * Mounted once, under the `QueryClientProvider`. There is deliberately exactly
 * one stream for the whole app: screens never open their own `EventSource`,
 * they just read the cache.
 */

import { useQuery, useQueryClient } from "@tanstack/react-query"
import { type ReactNode, useEffect, useRef } from "react"

import { eventStreamUrl, healthQueryOptions } from "@/api"
import { dispatchDomainEvent, invalidateEverything } from "@/events/dispatch"
import { DomainEventStream } from "@/events/stream"
import { useBaseUrl } from "@/stores/settings"
import { useStreamStore } from "@/stores/stream"

export function EventStreamProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const baseUrl = useBaseUrl()
  const streamRef = useRef<DomainEventStream | null>(null)

  // The same health probe the connection indicator shows, reused as the
  // stream's liveness watchdog (see below).
  const daemonUnreachable = useQuery({
    ...healthQueryOptions(),
    notifyOnChangeProps: ["isError"],
  }).isError

  useEffect(() => {
    const store = useStreamStore.getState()
    // Nothing cached from a previously configured daemon may survive.
    queryClient.clear()
    store.reset()

    const stream = new DomainEventStream(() => eventStreamUrl(baseUrl), {
      onStatus: (status, error) => {
        useStreamStore.getState().setStatus(status, error)
        if (import.meta.env.DEV) console.debug(`[events] stream ${status}`, error ?? "")
      },
      onOpen: ({ reconnected }) => {
        useStreamStore.getState().markOpen()
        if (import.meta.env.DEV) {
          console.debug(`[events] stream ${reconnected ? "reconnected" : "connected"}`)
        }
        // No replay: whatever happened while we were away has to be refetched.
        if (reconnected) invalidateEverything(queryClient)
      },
      onEvent: (event) => {
        useStreamStore.getState().markEvent(event.event)
        if (import.meta.env.DEV) console.debug("[events]", event.event, event.data)
        dispatchDomainEvent(queryClient, event)
      },
      onResync: ({ missed }) => {
        console.warn(`[events] daemon dropped ${missed} events for this client; resyncing`)
        useStreamStore.getState().markResync()
        invalidateEverything(queryClient)
      },
    })
    streamRef.current = stream
    stream.start()
    return () => {
      streamRef.current = null
      stream.stop()
    }
  }, [queryClient, baseUrl])

  // An `EventSource` does not reliably notice a daemon that went away: the
  // socket can sit in OPEN with no `error` ever firing, and the UI would go
  // quietly stale. `ariadned` makes this the normal case rather than the
  // exception — its graceful shutdown waits for in-flight requests, and an SSE
  // stream never finishes, so the connection outlives the daemon that is
  // stopping. The REST probe is the independent signal: when it loses the
  // daemon, drop the stream; when it finds it again, stop waiting out the
  // backoff.
  useEffect(() => {
    const stream = streamRef.current
    if (!stream) return
    if (daemonUnreachable) {
      stream.forceReconnect("daemon health probe failed")
    } else {
      stream.reconnectIfClosed("daemon health probe recovered")
    }
  }, [daemonUnreachable])

  return children
}
