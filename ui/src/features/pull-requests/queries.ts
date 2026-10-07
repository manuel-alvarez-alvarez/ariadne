import { useMutation, useQueryClient } from "@tanstack/react-query"
import { api, qk, unwrap } from "@/api"

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
