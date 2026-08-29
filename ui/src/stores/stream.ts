/**
 * Live status of the domain-event stream, published for the connection
 * indicator and anything else that wants to know whether the UI is following
 * the daemon or is stale.
 *
 * Written only by the event stream layer (`src/events`).
 */

import { create } from "zustand"

import type { DomainEventKind, HeartbeatDto } from "@/api"

export type StreamStatus =
  /** No stream requested yet. */
  | "idle"
  /** First connection attempt in flight. */
  | "connecting"
  /** Connected and receiving. */
  | "open"
  /** Dropped; a reconnect is scheduled. */
  | "reconnecting"

/** Which daemon is on the other end, as its last heartbeat said. */
interface DaemonInfo {
  version: string
  /** RFC 3339, as the daemon reported it; a new one means it restarted. */
  startedAt: string
}

interface StreamState {
  status: StreamStatus
  /**
   * The daemon behind the current connection, from its heartbeat. Null until
   * the first one arrives — the connection is the only place this is known.
   */
  daemon: DaemonInfo | null
  /** Consecutive failed attempts; reset to 0 once the stream opens. */
  attempts: number
  /** `Date.now()` of the last successful open. */
  openedAt: number | null
  /** `Date.now()` of the last event of any kind. */
  lastEventAt: number | null
  lastEventKind: DomainEventKind | null
  /** Events received since the app started; handy while debugging. */
  eventCount: number
  /** Reason of the last disconnect, if the browser gave one. */
  lastError: string | null
  /**
   * How many times the daemon told us we fell behind (`resync` control event).
   * Each one costs a full cache invalidation.
   */
  resyncCount: number
}

interface StreamActions {
  setStatus: (status: StreamStatus, error?: string | null) => void
  markOpen: () => void
  markEvent: (kind: DomainEventKind) => void
  markHeartbeat: (beat: HeartbeatDto) => void
  markResync: () => void
  reset: () => void
}

const initialState: StreamState = {
  status: "idle",
  daemon: null,
  attempts: 0,
  openedAt: null,
  lastEventAt: null,
  lastEventKind: null,
  eventCount: 0,
  lastError: null,
  resyncCount: 0,
}

export const useStreamStore = create<StreamState & StreamActions>()((set) => ({
  ...initialState,
  setStatus: (status, error) =>
    set((state) => ({
      status,
      lastError: error === undefined ? state.lastError : error,
      attempts: status === "reconnecting" ? state.attempts + 1 : state.attempts,
    })),
  markOpen: () => set({ status: "open", attempts: 0, openedAt: Date.now(), lastError: null }),
  markEvent: (kind) =>
    set((state) => ({
      lastEventAt: Date.now(),
      lastEventKind: kind,
      eventCount: state.eventCount + 1,
    })),
  markHeartbeat: ({ version, started_at }) => set({ daemon: { version, startedAt: started_at } }),
  markResync: () => set((state) => ({ resyncCount: state.resyncCount + 1 })),
  reset: () => set(initialState),
}))
