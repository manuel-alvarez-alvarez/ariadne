/**
 * Wires the single app-wide domain-event stream to the query cache.
 *
 * Mounted once, under the `QueryClientProvider`. There is deliberately exactly
 * one stream for the whole app: screens never open their own `EventSource`,
 * they just read the cache.
 *
 * It is also the app's only link to the daemon. Nothing polls: the heartbeat
 * the stream carries is what the connection indicator reads, and the stream's
 * own idle budget is what notices a daemon that went away.
 */

import { useQueryClient } from "@tanstack/react-query"
import { type ReactNode, useEffect, useRef } from "react"

import { eventStreamUrl } from "@/api"
import { dispatchDomainEvent, invalidateEverything } from "@/events/dispatch"
import { setDomainStream } from "@/events/handle"
import { DomainEventStream } from "@/events/stream"
import { useBaseUrl } from "@/stores/settings"
import { useStreamStore } from "@/stores/stream"

export function EventStreamProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const baseUrl = useBaseUrl()
  const prevBaseUrl = useRef(baseUrl)

  useEffect(() => {
    const store = useStreamStore.getState()
    // Nothing cached from a previously configured daemon may survive a
    // base-URL *switch* — and only a switch. On first mount the cache is
    // brand new, and wiping it here would destroy the queries the routed
    // screen already started (this effect runs after theirs, effects being
    // bottom-up), leaving every cold load stuck on skeletons.
    // `resetQueries`, not `clear`: it refetches for the observers that are
    // mounted instead of stranding them on removed queries.
    if (prevBaseUrl.current !== baseUrl) {
      prevBaseUrl.current = baseUrl
      void queryClient.resetQueries()
    }
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
      onHeartbeat: (beat) => {
        useStreamStore.getState().markHeartbeat(beat)
      },
      onResync: ({ missed }) => {
        console.warn(`[events] daemon dropped ${missed} events for this client; resyncing`)
        useStreamStore.getState().markResync()
        invalidateEverything(queryClient)
      },
    })
    setDomainStream(stream)
    stream.start()
    return () => {
      setDomainStream(null)
      stream.stop()
    }
  }, [queryClient, baseUrl])

  return children
}
