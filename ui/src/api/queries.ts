/**
 * The pieces every entity's queries and mutations are built from.
 *
 * Six feature directories talk to the daemon and each of them was spelling the
 * same four things out for itself: the client that answers a read, the cache
 * work a write does, the optimistic flip a confirmed action does, and — for the
 * two screens that have a thread — how a sent message reaches the one on
 * screen. They live here once, keyed by the `qk` entry the entity already owns,
 * so a feature's `queries.ts` is left with what is actually its own: which
 * endpoint, which body, and what else a write invalidates.
 *
 * Everything below writes the same keys the SSE dispatcher does. That is not
 * redundant with the stream: REST can be up while the stream is down (the amber
 * connection state), and an action that only showed up once the stream came
 * back would look broken.
 *
 * What is *not* here is anything that refetches on a clock. The stream is the
 * only source of freshness this client has, and every gap in it already ends in
 * `invalidateEverything` — see {@link createQueryClient}.
 */

import type { QueryKey } from "@tanstack/react-query"
import { QueryClient, queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, unwrap } from "./client"
import { ApiError } from "./errors"
import { qk } from "./query-keys"
import type { CreateMessageRequest, MessageDto } from "./types"

/** How often the health probe runs while the window is focused. */
const HEALTH_POLL_MS = 10_000

/**
 * The health probe has more than one observer — the connection indicator shows
 * it, the event stream uses it as a liveness watchdog — so its options live
 * here rather than being restated at each call site, which would give them
 * different polling and retry behaviour for the same key.
 */
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

/** The daemon's version, which cannot change under a running daemon. */
export function versionQueryOptions() {
  return queryOptions({
    queryKey: qk.system.version(),
    queryFn: () => unwrap(api().GET("/v1/version")),
    retry: false,
  })
}

// ── The cache, per entity ─────────────────────────────────────────────────

/** The `qk` entry of anything the daemon lists and identifies by id. */
interface EntityKeys {
  lists: () => QueryKey
  detail: (id: string) => QueryKey
}

/**
 * A row the daemon just answered with, into its detail entry, and a refetch of
 * the lists that hold it. What the dispatcher does for that entity's
 * `*_created` / `*_updated` event, and what every create and edit does itself
 * so the result is on screen before the event confirming it arrives.
 */
export function cacheRow<T extends { id: string }>(
  queryClient: QueryClient,
  keys: EntityKeys,
  row: T,
): void {
  queryClient.setQueryData(keys.detail(row.id), row)
  void queryClient.invalidateQueries({ queryKey: keys.lists() })
}

/** The same for a delete: the row's own entries go, the lists are refetched. */
export function dropRow(queryClient: QueryClient, keys: EntityKeys, id: string): void {
  queryClient.removeQueries({ queryKey: keys.detail(id) })
  void queryClient.invalidateQueries({ queryKey: keys.lists() })
}

// ── Optimistic status flips ───────────────────────────────────────────────

/**
 * Optimistic status flips, for the three mutations that only move a row from
 * one state to another: cancel a task, cancel a goal, kill a session.
 *
 * They are worth doing optimistically because the user already confirmed them
 * and the answer is never interesting on success — the row just has to stop
 * saying "running". Create and edit are not: their result is a row the client
 * cannot invent (ids, timestamps, whatever the daemon normalised), so they wait
 * for the response.
 *
 * Both the detail entry and every list entry holding the row are patched,
 * because the status is on screen in both places (a panel header and a board
 * card can be showing the same task). The snapshot taken on the way in is what
 * {@link restoreCache} puts back when the daemon refuses — a `409` here means
 * the row moved underneath us, so the pre-flip cache is the honest thing to
 * show until the refetch lands.
 */
export type CacheSnapshot = [QueryKey, unknown][]

/** Anything the daemon returns in a list and identifies by id. */
interface StatusRow {
  id: string
  status: string
}

/**
 * Flip one row's status in the detail entry and in every cached list, and hand
 * back what those entries held.
 *
 * In-flight refetches are cancelled first: one that resolves after the patch
 * would overwrite it with the pre-cancel row and the status would flicker back.
 */
export async function optimisticStatus<S extends string>(
  queryClient: QueryClient,
  keys: EntityKeys,
  id: string,
  status: S,
): Promise<CacheSnapshot> {
  const detailKey = keys.detail(id)
  const listsKey = keys.lists()
  await Promise.all([
    queryClient.cancelQueries({ queryKey: detailKey, exact: true }),
    queryClient.cancelQueries({ queryKey: listsKey }),
  ])

  const snapshot: CacheSnapshot = [
    [detailKey, queryClient.getQueryData(detailKey)],
    ...queryClient.getQueriesData({ queryKey: listsKey }),
  ]

  queryClient.setQueryData(detailKey, (row: StatusRow | undefined) =>
    row ? { ...row, status } : row,
  )
  queryClient.setQueriesData({ queryKey: listsKey }, (rows: StatusRow[] | undefined) =>
    Array.isArray(rows) ? rows.map((row) => (row.id === id ? { ...row, status } : row)) : rows,
  )

  return snapshot
}

/** Undo an {@link optimisticStatus} patch, entry by entry. */
export function restoreCache(queryClient: QueryClient, snapshot: CacheSnapshot | undefined): void {
  for (const [key, data] of snapshot ?? []) {
    // `undefined` means the entry was not cached before; setting it back is a
    // no-op in TanStack Query, which is exactly right.
    queryClient.setQueryData(key, data)
  }
}

/**
 * A confirmed action that answers with the updated row: optimistic when the
 * status it lands in is known in advance, and never for one the daemon decides
 * (retrying a task schedules a session, and its next status is the daemon's
 * answer, not ours).
 */
export function useRowAction<T extends { id: string }, V = void>(
  keys: EntityKeys,
  id: string,
  mutationFn: (variables: V) => Promise<T>,
  options: {
    /** The status the row lands in, when the client can know it in advance. */
    optimistic?: string
    /**
     * Anything else the action moved, invalidated once the daemon has accepted
     * it. Not react-query's `onSettled`: this runs on success alone, because a
     * refused action moved nothing.
     */
    alsoInvalidates?: (queryClient: QueryClient) => void
  } = {},
) {
  const queryClient = useQueryClient()
  return useMutation<T, Error, V, CacheSnapshot | undefined>({
    mutationFn,
    onMutate: options.optimistic
      ? () => optimisticStatus(queryClient, keys, id, options.optimistic as string)
      : undefined,
    onError: (_error, _variables, snapshot) => restoreCache(queryClient, snapshot),
    onSuccess: (row) => {
      cacheRow(queryClient, keys, row)
      options.alsoInvalidates?.(queryClient)
    },
  })
}

// ── Message threads ───────────────────────────────────────────────────────

/**
 * The compose box of a goal's or a task's thread.
 *
 * The request is the whole `CreateMessageRequest`, body and optional addressee:
 * the daemon resolves `to` against the thread's participants and answers 400 if
 * it names anyone else, which is the error the box draws.
 *
 * The daemon answers with the created message, which is appended straight to
 * the cached thread: it is the newest by construction (ids are ordered and it
 * was just minted), so the send shows up without waiting for the
 * `message_created` event to invalidate. The invalidation still runs for the
 * messages an offline stream may have missed; the append is deduped by id, so
 * event and refetch land on the same thread.
 */
export function usePostMessage(
  messagesKey: QueryKey,
  post: (message: CreateMessageRequest) => Promise<MessageDto>,
) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: post,
    onSuccess: (message) => {
      queryClient.setQueryData<MessageDto[]>(messagesKey, (thread) =>
        thread && !thread.some((existing) => existing.id === message.id)
          ? [...thread, message]
          : thread,
      )
      void queryClient.invalidateQueries({ queryKey: messagesKey })
    },
  })
}

// ── The client itself ─────────────────────────────────────────────────────

/**
 * Whether a failed read is worth asking the daemon about again.
 *
 * Exported for the one screen-level property it decides: how long a screen
 * shimmers before it admits it has nothing. That has to be the *same* length
 * everywhere, because a daemon that went away takes every screen with it, and
 * a board that gave up at four seconds next to a table still shimmering at
 * fifteen reads as one of them being broken rather than the daemon being down.
 *
 * So the two hopeless cases are not retried at all:
 *
 * - a 4xx will not fix itself;
 * - a network failure means the request never reached a daemon, which the
 *   health probe is already polling for and the connection banner is already
 *   saying — retrying it only delays the same answer.
 *
 * Everything else — a 5xx, a dropped response mid-flight — is transient and
 * still gets its two retries.
 */
export function shouldRetryQuery(failureCount: number, error: unknown): boolean {
  if (ApiError.is(error) && (error.isNetworkError || (error.status >= 400 && error.status < 500))) {
    return false
  }
  return failureCount < 2
}

/**
 * Defaults for a client whose freshness comes from the event stream alone.
 *
 * The dispatcher in `src/events/dispatch.ts` patches details and invalidates
 * lists as the daemon changes, and every gap in the stream — a reconnect, a
 * `resync` — ends in its `invalidateEverything`, which refetches what is on
 * screen and marks the rest stale. So a cached answer that nothing invalidated
 * is the daemon's current one, and no amount of elapsed time makes that less
 * true: refetching it because the window regained focus, or because thirty
 * seconds passed, would ask a question the stream has already answered.
 *
 * Hence data that never goes stale by itself and no refetch on focus. The two
 * reads with no event behind them — the health probe and a task's diff — say so
 * at their own call sites rather than making everything else pay for it.
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Number.POSITIVE_INFINITY,
        refetchOnWindowFocus: false,
        retry: shouldRetryQuery,
      },
      mutations: {
        retry: false,
      },
    },
  })
}
