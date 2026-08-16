/**
 * Wires the single app-wide domain-event stream to the query cache.
 *
 * Mounted once, under the `QueryClientProvider`. There is deliberately exactly
 * one stream for the whole app: screens never open their own `EventSource`,
 * they just read the cache.
 */

import { useQueryClient } from "@tanstack/react-query"
import { type ReactNode, useEffect } from "react"

import { eventStreamUrl } from "@/api"
import { dispatchDomainEvent, invalidateEverything } from "@/events/dispatch"
import { DomainEventStream } from "@/events/stream"
import { useBaseUrl } from "@/stores/settings"
import { useStreamStore } from "@/stores/stream"

export function EventStreamProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const baseUrl = useBaseUrl()

  useEffect(() => {
    const store = useStreamStore.getState()
    // Nothing cached from a previously configured daemon may survive.
    queryClient.clear()
    store.reset()

    const stream = new DomainEventStream(() => eventStreamUrl(baseUrl), {
      onStatus: (status, error) => useStreamStore.getState().setStatus(status, error),
      onOpen: ({ reconnected }) => {
        useStreamStore.getState().markOpen()
        // No replay: whatever happened while we were away has to be refetched.
        if (reconnected) invalidateEverything(queryClient)
      },
      onEvent: (event) => {
        useStreamStore.getState().markEvent(event.event)
        dispatchDomainEvent(queryClient, event)
      },
      onResync: ({ missed }) => {
        console.warn(`[events] daemon dropped ${missed} events for this client; resyncing`)
        useStreamStore.getState().markResync()
        invalidateEverything(queryClient)
      },
    })
    stream.start()
    return () => stream.stop()
  }, [queryClient, baseUrl])

  return children
}
