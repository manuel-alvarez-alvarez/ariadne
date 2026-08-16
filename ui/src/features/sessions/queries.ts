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

import { api, qk, type SessionDto, type SessionStatus, unwrap } from "@/api"

/** The filters `GET /v1/sessions` actually takes. */
export interface SessionListFilters {
  goal?: string
  task?: string
  status?: SessionStatus
}

export function sessionsQueryOptions(filters: SessionListFilters = {}) {
  return queryOptions({
    queryKey: qk.sessions.list(filters),
    queryFn: () => unwrap(api().GET("/v1/sessions", { params: { query: filters } })),
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

/** Kill a session's tmux process. Only meaningful while the session is live. */
export function useKillSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/sessions/{id}/kill", { params: { path: { id } } })),
    onSuccess: (session) => cacheSession(queryClient, session),
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
    onSuccess: (session) => cacheSession(queryClient, session),
  })
}

/** Keystrokes typed but not yet handed to a request, per session. */
const inputQueue = new Map<string, string>()
/** The in-flight send for a session, while one is running. */
const inputInFlight = new Map<string, Promise<void>>()

/**
 * Type into a live session's pane. Deliberately not a mutation: this is called
 * per keystroke, it changes nothing the cache holds, and a `useMutation`'s
 * pending state would re-render the terminal on every character.
 *
 * One request per session at a time. A POST per keystroke fired in parallel
 * races — the browser sends them down several connections and the pane
 * receives `ceho` for `echo` — so anything typed while a request is in flight
 * rides along in the next one. That also keeps a paste or a fast typist to a
 * handful of requests instead of one per character.
 *
 * There is no retry: a keystroke that arrives late is worse than one that
 * never arrives. The daemon answers `409` once the session is over or its pane
 * is gone, and that rejection reaches whoever is waiting on the send.
 */
export function sendSessionInput(id: string, data: string): Promise<void> {
  inputQueue.set(id, (inputQueue.get(id) ?? "") + data)
  const running = inputInFlight.get(id)
  if (running) return running
  const drain = drainSessionInput(id).finally(() => inputInFlight.delete(id))
  inputInFlight.set(id, drain)
  return drain
}

async function drainSessionInput(id: string): Promise<void> {
  try {
    for (;;) {
      const data = inputQueue.get(id)
      if (data === undefined) return
      inputQueue.delete(id)
      await unwrap(
        api().POST("/v1/sessions/{id}/input", { params: { path: { id } }, body: { data } }),
      )
    }
  } catch (error) {
    // Everything typed while the failed request was in flight goes with it.
    // Keeping it would contradict the no-retry rule by another route: it
    // would ride out behind the *next* keystroke, minutes later, and a
    // Return or Ctrl-C replayed out of context acts on whatever the pane is
    // showing by then. A session that briefly has no pane while it starts up
    // is exactly where this happens.
    inputQueue.delete(id)
    throw error
  }
}

function cacheSession(queryClient: ReturnType<typeof useQueryClient>, session: SessionDto): void {
  queryClient.setQueryData(qk.sessions.detail(session.id), session)
  void queryClient.invalidateQueries({ queryKey: qk.sessions.lists() })
}

/** Index a list response by id, for turning the ids on a session into names. */
export function byId<T extends { id: string }>(items: T[] | undefined): Map<string, T> {
  return new Map((items ?? []).map((item) => [item.id, item]))
}
