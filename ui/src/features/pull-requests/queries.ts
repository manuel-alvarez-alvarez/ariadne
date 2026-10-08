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
