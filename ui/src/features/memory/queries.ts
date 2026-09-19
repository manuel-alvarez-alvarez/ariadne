/**
 * The memory screens' reads and writes: saved facts, of one repository or of
 * none (a global memory), under `/v1/memories`.
 *
 * List and search share one query: an empty query reads `GET /v1/memories`, a
 * non-empty one reads the daemon's own `/v1/memories/search`, exactly the split
 * `ariadne memory ls` and `ariadne memory search` make. That keeps the two
 * backed by the same case-insensitive substring match the daemon runs rather
 * than a client-side filter that could disagree with it.
 *
 * `memory_created` / `memory_deleted` carry no id worth keying a detail on, so
 * — like the mutations below — the dispatcher and these hooks both simply
 * refetch `qk.memories.lists()`.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, type CreateMemoryRequest, type MemoryScope, qk, unwrap } from "@/api"

/** What one memory list reads: the daemon's own `repository` and `scope`. */
export interface MemoryListFilters {
  repository?: string
  scope?: MemoryScope
}

/**
 * `GET /v1/memories` (empty `query`) or its `/search` sibling (non-empty
 * `query`), in the search's own shape: the `hits`, and whether they are the
 * `fallback` the daemon gives where no query word matched. A plain list is
 * never a fallback.
 */
export function memoriesQueryOptions(filters: MemoryListFilters, query: string) {
  const q = query.trim()
  const { repository, scope } = filters
  return queryOptions({
    queryKey: qk.memories.list({ repository, scope, q: q || undefined }),
    queryFn: () =>
      q
        ? unwrap(api().GET("/v1/memories/search", { params: { query: { q, repository, scope } } }))
        : unwrap(api().GET("/v1/memories", { params: { query: { repository, scope } } })).then(
            (hits) => ({ hits, fallback: false }),
          ),
  })
}

/** `POST /v1/memories`: a user write, which records no source. */
export function useCreateMemory() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateMemoryRequest) => unwrap(api().POST("/v1/memories", { body })),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: qk.memories.lists() }),
  })
}

export function useDeleteMemory() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/memories/{id}", { params: { path: { id } } })),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: qk.memories.lists() }),
  })
}
