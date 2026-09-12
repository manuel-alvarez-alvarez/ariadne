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
 *
 * Only the session calls are here. A session view also reads the goal and the
 * task behind the ids it carries, and those are asked for where they are
 * owned (`goals/queries.ts`, `tasks/queries.ts`) rather than declared a second
 * time here: the key is the same either way, so a second declaration bought
 * nothing and was one `select` or one `staleTime` away from disagreeing with
 * the original.
 */

import {
  infiniteQueryOptions,
  queryOptions,
  useMutation,
  useQueryClient,
} from "@tanstack/react-query"

import {
  api,
  type CacheSnapshot,
  type ConsoleInputRequest,
  cacheRow,
  type OutsideSessionDto,
  optimisticStatus,
  qk,
  restoreCache,
  type Seat,
  type SessionDto,
  type SessionStatus,
  unwrap,
} from "@/api"

import { isLiveStatus, sessionAttention } from "./session-display"

/** What the sessions list can be narrowed by. */
export interface SessionListFilters {
  goal?: string
  task?: string
  status?: SessionStatus
  /**
   * Applied here rather than by the daemon: `GET /v1/sessions` takes no seat.
   * The request — and so the cache entry — is the same one an unfiltered list
   * makes, and the seat is a per-observer `select` over it.
   */
  seat?: Seat
  /**
   * Only the sessions with an agent that may still produce output. Client-side
   * for the same reason the seat is, and for one more: the daemon's filter
   * takes *one* status, and being live is three of them (see
   * {@link isLiveStatus}). Set alongside `status` it would only narrow it
   * further, so the two are never used together.
   */
  live?: boolean
  /**
   * Only the sessions the daemon has raised a reason on. Client-side again,
   * and by {@link sessionAttention} rather than by a rule of its own: the
   * sessions screen and the attention strip have to agree on which agents are
   * asking for a person. The request is the same one an unfiltered list makes,
   * so this costs no round trip of its own.
   */
  attention?: boolean
}

export function sessionsQueryOptions({ seat, live, attention, ...query }: SessionListFilters = {}) {
  const narrowed = (session: SessionDto) =>
    (!seat || session.seat === seat) &&
    (!live || isLiveStatus(session.status)) &&
    (!attention || sessionAttention(session) !== null)
  return queryOptions({
    queryKey: qk.sessions.list(query),
    queryFn: () => unwrap(api().GET("/v1/sessions", { params: { query } })),
    select:
      seat || live || attention ? (sessions: SessionDto[]) => sessions.filter(narrowed) : undefined,
  })
}

export function sessionQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.sessions.detail(id),
    queryFn: () => unwrap(api().GET("/v1/sessions/{id}", { params: { path: { id } } })),
  })
}

/** What the outside-sessions list can be narrowed by, as the daemon takes it. */
export interface OutsideSessionListFilters {
  agent?: string
  dir?: string
  /** RFC 3339, not a day: see `outside-filters.ts`. */
  since?: string
  until?: string
  q?: string
}

/**
 * CLI conversations Ariadne did not start, ready for the user to adopt: the
 * pages the daemon cuts from its snapshot, under one filter.
 *
 * `next_cursor` is the next page's parameter and a null one is the last page,
 * so the cursor never reaches the key — the pages of one filter are one cache
 * entry, which is what lets the table grow by a page rather than reload.
 *
 * `takeRefresh` is read at request time rather than keyed on: asking every
 * agent again is what the Refresh button does, not what the list is narrowed
 * by, and its answer belongs in the entry the filters already name. It is
 * taken, not read — the flag it answers from is spent on the first request of
 * a refetch, which is the first page.
 */
export function outsideSessionsQueryOptions(
  filters: OutsideSessionListFilters = {},
  takeRefresh: () => boolean = () => false,
) {
  return infiniteQueryOptions({
    queryKey: qk.outsideSessions.list(filters),
    queryFn: ({ pageParam }) =>
      unwrap(
        api().GET("/v1/outside-sessions", {
          params: {
            query: {
              ...filters,
              cursor: pageParam ?? undefined,
              refresh: takeRefresh() || undefined,
            },
          },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor ?? null,
  })
}

/** Make an outside agent conversation the author of a ready task. */
export function useAdoptOutsideSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ taskId, ...body }: { taskId: string } & OutsideSessionDto) =>
      unwrap(
        api().POST("/v1/tasks/{id}/author-session", {
          params: { path: { id: taskId } },
          body: {
            agent_id: body.agent_id,
            internal_session_id: body.internal_session_id,
          },
        }),
      ),
    onSuccess: async (session) => {
      cacheRow(queryClient, qk.sessions, session)
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: qk.outsideSessions.lists() }),
        queryClient.invalidateQueries({ queryKey: qk.tasks.all() }),
      ])
    },
  })
}

/**
 * Kill a session's agent process. Only meaningful while the session is live.
 *
 * Optimistic: the daemon stops the agent and marks the session `exited`,
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
 * same agent conversation — and answers with the refreshed
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
 * Post text into a session's console. Each call is a whole prompt of its own,
 * sent at once or queued behind a running turn.
 */
export function sendConsoleInput(id: string, text: string): Promise<void> {
  return unwrap(
    api().POST("/v1/sessions/{id}/console/input", {
      params: { path: { id } },
      body: { text } satisfies ConsoleInputRequest,
    }),
  )
}

/** Index a list response by id, for turning the ids on a session into names. */
export function byId<T extends { id: string }>(items: T[] | undefined): Map<string, T> {
  return new Map((items ?? []).map((item) => [item.id, item]))
}
