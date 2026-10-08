import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import { api, qk, unwrap } from "@/api"

/**
 * One pull request, `GET /v1/pull-requests/{id}`: what its panel shows, and
 * where a pull request session's title and link come from. The dispatcher
 * patches this key on `pull_request_updated`, so every reader stays live.
 */
export function pullRequestQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.pullRequests.detail(id),
    queryFn: () => unwrap(api().GET("/v1/pull-requests/{id}", { params: { path: { id } } })),
  })
}

/**
 * `GET /v1/pull-requests` under `filters`: the list the Forge screen's tab
 * reads, kept live by the dispatcher's `pullRequests.lists()` invalidations.
 */
export function pullRequestsQueryOptions(filters: {
  repo?: string
  role?: string
  requested?: boolean
}) {
  return queryOptions({
    queryKey: qk.pullRequests.list(filters),
    queryFn: () => unwrap(api().GET("/v1/pull-requests", { params: { query: filters } })),
  })
}

export function useRemovePullRequest(onRemoved?: () => void) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/pull-requests/{id}", { params: { path: { id } } })),
    onSuccess: (_, id) => {
      client.removeQueries({ queryKey: qk.pullRequests.detail(id) })
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists() })
      onRemoved?.()
    },
  })
}

/**
 * `PUT /v1/pull-requests/{id}/ariadne-review` — ask Ariadne to review a
 * request of the user's own, or stop asking (029). The row comes back and
 * the `pull_request_updated` event follows; both land in the cache.
 */
export function useAskReview() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      id,
      ...body
    }: {
      id: string
      asked: boolean
      model?: string
      effort?: string
      skills?: string[]
    }) =>
      unwrap(
        api().PUT("/v1/pull-requests/{id}/ariadne-review", {
          params: { path: { id } },
          body,
        }),
      ),
    onSuccess: (row) => {
      client.setQueryData(qk.pullRequests.detail(row.id), row)
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists() })
    },
  })
}
