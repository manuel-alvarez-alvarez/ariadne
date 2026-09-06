/**
 * The repository screen's reads and writes.
 *
 * Reads follow the app-wide key convention so the SSE dispatcher keeps them
 * live: `repository_created` / `repository_updated` / `repository_deleted`
 * patch `repositories.detail` and invalidate `repositories.lists()`, which is
 * the one list here — the daemon takes no filters on it.
 *
 * The mutations then do the same thing to the cache themselves — `cacheRow` and
 * `dropRow` — for the reason `@/api/queries.ts` gives: REST can be up while the
 * stream is down, and a repository that only appeared once the stream came back
 * would look broken.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import {
  api,
  type CreateRepositoryRequest,
  cacheRow,
  dropRow,
  qk,
  type UpdateRepositoryRequest,
  unwrap,
} from "@/api"

/** `GET /v1/repositories` — every registered checkout, path-ordered by the daemon. */
export function repositoriesQueryOptions() {
  return queryOptions({
    queryKey: qk.repositories.list(),
    queryFn: () => unwrap(api().GET("/v1/repositories")),
  })
}

export function useCreateRepository() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateRepositoryRequest) => unwrap(api().POST("/v1/repositories", { body })),
    onSuccess: (repository) => cacheRow(queryClient, qk.repositories, repository),
  })
}

export function useUpdateRepository() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateRepositoryRequest }) =>
      unwrap(api().PUT("/v1/repositories/{id}", { params: { path: { id } }, body })),
    onSuccess: (repository) => {
      cacheRow(queryClient, qk.repositories, repository)
      // Goals reference repositories live and carry them inline as
      // `GoalDto.repos`, so an edit here is stale in every goal that works in
      // this one until the goals are read again. The dispatcher does the same
      // for `repository_updated`.
      void queryClient.invalidateQueries({ queryKey: qk.goals.all() })
    },
  })
}

export function useDeleteRepository() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/repositories/{id}", { params: { path: { id } } })),
    onSuccess: (_result, id) => dropRow(queryClient, qk.repositories, id),
  })
}
