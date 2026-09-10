/**
 * The memory screen's reads and writes: one repository's saved entries.
 *
 * There is no create here — an agent session is what saves a memory (019), and
 * the desktop side only ever lists, searches and deletes. List and search share
 * one query: an empty query reads `GET .../memories`, a non-empty one reads the
 * daemon's own `.../memories/search`, exactly the split `ariadne memory ls` and
 * `ariadne memory search` make. That keeps the two backed by the same
 * case-insensitive substring match the daemon runs rather than a client-side
 * filter that could disagree with it.
 *
 * `memory_created` / `memory_deleted` carry no repository of their own worth
 * keying a detail on, so — like the mutations below — the dispatcher and these
 * hooks both simply refetch `qk.memories.lists()`.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, qk, unwrap } from "@/api"

/**
 * `GET /v1/repositories/{repositoryId}/memories` (empty `query`) or its
 * `/search` sibling (non-empty `query`), newest first.
 */
export function memoriesQueryOptions(repositoryId: string, query: string) {
  const q = query.trim()
  return queryOptions({
    queryKey: qk.memories.list({ repository: repositoryId, q: q || undefined }),
    queryFn: () =>
      q
        ? unwrap(
            api().GET("/v1/repositories/{repository_id}/memories/search", {
              params: { path: { repository_id: repositoryId }, query: { q } },
            }),
          )
        : unwrap(
            api().GET("/v1/repositories/{repository_id}/memories", {
              params: { path: { repository_id: repositoryId } },
            }),
          ),
  })
}

export function useDeleteMemory(repositoryId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(
        api().DELETE("/v1/repositories/{repository_id}/memories/{id}", {
          params: { path: { repository_id: repositoryId, id } },
        }),
      ),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: qk.memories.lists() }),
  })
}
