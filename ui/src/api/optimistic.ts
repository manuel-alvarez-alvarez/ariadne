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
 * `restoreCache` puts back when the daemon refuses — a `409` here means the row
 * moved underneath us, so the pre-flip cache is the honest thing to show until
 * the refetch lands.
 */

import type { QueryClient, QueryKey } from "@tanstack/react-query"

/** What the cache held before the flip, for putting back on failure. */
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
  {
    detailKey,
    listsKey,
    id,
    status,
  }: {
    /** The exact `qk.<entity>.detail(id)` key. */
    detailKey: QueryKey
    /** The `qk.<entity>.lists()` prefix — every list under it is patched. */
    listsKey: QueryKey
    id: string
    status: S
  },
): Promise<CacheSnapshot> {
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

/** Undo an `optimisticStatus` patch, entry by entry. */
export function restoreCache(queryClient: QueryClient, snapshot: CacheSnapshot | undefined): void {
  for (const [key, data] of snapshot ?? []) {
    // `undefined` means the entry was not cached before; setting it back is a
    // no-op in TanStack Query, which is exactly right.
    queryClient.setQueryData(key, data)
  }
}
