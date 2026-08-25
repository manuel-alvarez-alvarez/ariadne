/**
 * Everything the session views read from and write to the daemon.
 *
 * Nothing here subscribes to domain events: `session_created` and
 * `session_updated` already patch `sessions.detail` and invalidate
 * `sessions.lists` in `src/events/dispatch.ts`, so these queries go live for
 * free. The mutations still write their response into the cache, because the
 * action's result should be on screen before the event that confirms it
 * arrives — and because the event never arrives at all when the stream is
 * down.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CacheSnapshot,
  cacheRow,
  optimisticStatus,
  qk,
  type Role,
  restoreCache,
  type SessionDto,
  type SessionStatus,
  unwrap,
} from "@/api"

import type { PaneSize } from "./log-stream"
import { isLiveStatus } from "./session-display"

/** What the sessions list can be narrowed by. */
export interface SessionListFilters {
  goal?: string
  task?: string
  status?: SessionStatus
  /**
   * Applied here rather than by the daemon: `GET /v1/sessions` takes no role.
   * The request — and so the cache entry — is the same one an unfiltered list
   * makes, and the role is a per-observer `select` over it.
   */
  role?: Role
  /**
   * Only the sessions with a pane that may still produce output. Client-side
   * for the same reason the role is, and for one more: the daemon's filter
   * takes *one* status, and being live is three of them (see
   * {@link isLiveStatus}). Set alongside `status` it would only narrow it
   * further, so the two are never used together.
   */
  live?: boolean
}

export function sessionsQueryOptions({ role, live, ...query }: SessionListFilters = {}) {
  const narrowed = (session: SessionDto) =>
    (!role || session.role === role) && (!live || isLiveStatus(session.status))
  return queryOptions({
    queryKey: qk.sessions.list(query),
    queryFn: () => unwrap(api().GET("/v1/sessions", { params: { query } })),
    select: role || live ? (sessions: SessionDto[]) => sessions.filter(narrowed) : undefined,
  })
}

export function sessionQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.sessions.detail(id),
    queryFn: () => unwrap(api().GET("/v1/sessions/{id}", { params: { path: { id } } })),
  })
}

/**
 * Profiles, goals and tasks are read only to turn the ids on a session into
 * names. They are owned by other screens; these keys are the shared ones from
 * `qk`, so whichever screen loads them first serves the others.
 */
export function profilesQueryOptions() {
  return queryOptions({
    queryKey: qk.profiles.list(),
    queryFn: () => unwrap(api().GET("/v1/profiles")),
  })
}

export function goalQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.goals.detail(id),
    queryFn: () => unwrap(api().GET("/v1/goals/{id}", { params: { path: { id } } })),
  })
}

export function taskQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.tasks.detail(id),
    queryFn: () => unwrap(api().GET("/v1/tasks/{id}", { params: { path: { id } } })),
  })
}

/**
 * Kill a session's tmux process. Only meaningful while the session is live.
 *
 * Optimistic: the daemon tears the pane down and marks the session `exited`,
 * which is the one status the client can know in advance, and a row that still
 * says "running" after a confirmed kill is the wrong thing to be looking at.
 * A refusal puts the previous row straight back. The session is the mutation's
 * variable rather than a binding of the hook — the sessions list kills the row
 * that was clicked — which is why this is not `useRowAction`.
 */
export function useKillSession() {
  const queryClient = useQueryClient()
  return useMutation<SessionDto, Error, string, CacheSnapshot | undefined>({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/sessions/{id}/kill", { params: { path: { id } } })),
    onMutate: (id) =>
      optimisticStatus(queryClient, qk.sessions, id, "exited" satisfies SessionStatus),
    onError: (_error, _id, snapshot) => restoreCache(queryClient, snapshot),
    onSuccess: (session) => cacheRow(queryClient, qk.sessions, session),
  })
}

/**
 * Revive an ended session. The daemon relaunches the session itself — same id,
 * same tmux name, same agent conversation — and answers with the refreshed
 * row, so the response's status is what says whether anything was revived.
 * `409` is the "not resumable" answer (no internal session id, still running,
 * agent cannot resume) and carries the reason in its envelope.
 */
export function useResumeSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/sessions/{id}/resume", { params: { path: { id } } })),
    onSuccess: (session) => cacheRow(queryClient, qk.sessions, session),
  })
}

/**
 * One request per session at a time, for the two things a frame sends a pane.
 *
 * Deliberately not mutations: both are called as fast as a person can type or
 * drag, they change nothing the cache holds, and a `useMutation`'s pending
 * state would re-render the terminal for each one. Fired in parallel they also
 * race — the browser sends them down several connections, and the pane receives
 * `ceho` for `echo` or settles at whichever size lost — so whatever arrives
 * while a request is in flight waits and rides along in the next one.
 *
 * The two differ only in how that waiting is spelled, which is what `queue`
 * says: keystrokes *accumulate*, because every one of them has to reach the
 * pane in order; a size *replaces* whatever was queued behind it, because only
 * the newest one is worth asking for.
 *
 * There is no retry either way, and a failure drops what was queued behind it.
 * For input, keeping it would contradict the no-retry rule by another route: it
 * would ride out behind the *next* keystroke, minutes later, and a Return or
 * Ctrl-C replayed out of context acts on whatever the pane is showing by then.
 * For a resize, the frame the size was measured from is a moment old and the
 * next thing that moves it measures again. The daemon answers `409` once the
 * session is over or its pane is gone, and that rejection reaches whoever is
 * waiting on the send.
 */
function coalesced<T>(
  queue: (pending: T | undefined, next: T) => T,
  send: (id: string, value: T) => Promise<unknown>,
): (id: string, value: T) => Promise<void> {
  const pending = new Map<string, T>()
  const inFlight = new Map<string, Promise<void>>()

  async function drain(id: string): Promise<void> {
    try {
      for (;;) {
        const value = pending.get(id)
        if (value === undefined) return
        pending.delete(id)
        await send(id, value)
      }
    } catch (error) {
      pending.delete(id)
      throw error
    }
  }

  return (id, value) => {
    pending.set(id, queue(pending.get(id), value))
    const running = inFlight.get(id)
    if (running) return running
    const draining = drain(id).finally(() => inFlight.delete(id))
    inFlight.set(id, draining)
    return draining
  }
}

/** Type into a live session's pane; every keystroke gets there, in order. */
export const sendSessionInput = coalesced<string>(
  (queued, data) => (queued ?? "") + data,
  (id, data) =>
    unwrap(api().POST("/v1/sessions/{id}/input", { params: { path: { id } }, body: { data } })),
)

/**
 * Ask a live session's pane to draw at `size`; only the newest one is sent.
 *
 * A pane's size is last-write-wins, and two overlapping resizes can land in
 * either order, which would leave the pane at a size nobody is showing. Until
 * the pane answers, the terminal scales its font to the grid it has — a pane
 * that was not resized is a pane rendered smaller, not one that stopped
 * working.
 */
export const sendSessionResize = coalesced<PaneSize>(
  (_queued, size) => size,
  (id, size) =>
    unwrap(api().POST("/v1/sessions/{id}/resize", { params: { path: { id } }, body: size })),
)

/** Index a list response by id, for turning the ids on a session into names. */
export function byId<T extends { id: string }>(items: T[] | undefined): Map<string, T> {
  return new Map((items ?? []).map((item) => [item.id, item]))
}
