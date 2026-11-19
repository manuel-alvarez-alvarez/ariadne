/**
 * Wires the app-wide domain-event streams to the query cache: one per
 * reachable host.
 *
 * Mounted once, under the `QueryClientProvider`. There is deliberately exactly
 * one stream per host for the whole app: screens never open their own
 * `EventSource`, they just read the cache. Each stream dispatches under its
 * own host's keys.
 *
 * The `local` stream is also the app's only link to the settings daemon.
 * Nothing polls: the heartbeat it carries is what the connection indicator
 * reads, and its own idle budget is what notices a daemon that went away. A
 * named host's stream publishes nothing to that indicator — the connection
 * banner and status stay about `local`. A named host's stream state and
 * heartbeat go to `useHostStreamStore`, which the host picker and the Hosts
 * table read.
 */

import { useQueryClient } from "@tanstack/react-query"
import { type ReactNode, useEffect, useRef } from "react"

import { eventStreamUrl, type Host, LOCAL_HOST, useHosts } from "@/api"
import { dispatchDomainEvent, invalidateEverything } from "@/events/dispatch"
import { setDomainStream } from "@/events/handle"
import { DomainEventStream } from "@/events/stream"
import { useBaseUrl } from "@/stores/settings"
import { useHostStreamStore, useStreamStore } from "@/stores/stream"
import { recordStreamReconnect } from "@/telemetry/telemetry"

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

    const stream = new DomainEventStream(() => eventStreamUrl(LOCAL_HOST) ?? "", {
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
        if (reconnected) invalidateEverything(queryClient, LOCAL_HOST)
      },
      onReconnect: recordStreamReconnect,
      onEvent: (event) => {
        useStreamStore.getState().markEvent(event.event)
        if (import.meta.env.DEV) console.debug("[events]", event.event, event.data)
        dispatchDomainEvent(queryClient, LOCAL_HOST, event)
      },
      onHeartbeat: (beat) => {
        useStreamStore.getState().markHeartbeat(beat)
      },
      onResync: ({ missed }) => {
        console.warn(`[events] daemon dropped ${missed} events for this client; resyncing`)
        useStreamStore.getState().markResync()
        invalidateEverything(queryClient, LOCAL_HOST)
      },
    })
    setDomainStream(stream)
    stream.start()
    return () => {
      setDomainStream(null)
      stream.stop()
    }
  }, [queryClient, baseUrl])

  const { hosts } = useHosts()
  return (
    <>
      {hosts
        .filter((host) => host.name !== LOCAL_HOST && host.url !== null)
        .map((host) => (
          // Keyed by the address too: a host that moved is a new stream.
          <HostEventStream key={`${host.name}\n${host.url}`} host={host.name} />
        ))}
      {children}
    </>
  )
}

/**
 * One named host's stream: the same protocol and recovery as `local`'s, under
 * that host's keys, and its state kept in the host stream store, not the
 * connection store.
 */
function HostEventStream({ host }: { host: Host }) {
  const queryClient = useQueryClient()

  useEffect(() => {
    const url = eventStreamUrl(host)
    if (url === null) return
    const stream = new DomainEventStream(() => url, {
      onStatus: (status, error) => {
        useHostStreamStore.getState().setStatus(host, status, error)
        if (import.meta.env.DEV) console.debug(`[events:${host}] stream ${status}`, error ?? "")
      },
      onOpen: ({ reconnected }) => {
        useHostStreamStore.getState().markOpen(host)
        if (reconnected) invalidateEverything(queryClient, host)
      },
      onReconnect: recordStreamReconnect,
      onEvent: (event) => {
        if (import.meta.env.DEV) console.debug(`[events:${host}]`, event.event, event.data)
        dispatchDomainEvent(queryClient, host, event)
      },
      onHeartbeat: (beat) => {
        useHostStreamStore.getState().markHeartbeat(host, beat)
      },
      onResync: ({ missed }) => {
        console.warn(`[events:${host}] daemon dropped ${missed} events for this client; resyncing`)
        invalidateEverything(queryClient, host)
      },
    })
    stream.start()
    return () => {
      stream.stop()
      useHostStreamStore.getState().forget(host)
    }
  }, [queryClient, host])

  return null
}
