import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import { api, type PullRequestDto, qk, unwrap } from "@/api"

/**
 * What identifies a request on the screen: its repository and its number.
 * Ariadne's id names only the requests it works on, and most it does not.
 */
export function pullRequestKey(pull: Pick<PullRequestDto, "repository_id" | "number">): string {
  return `${pull.repository_id}:${pull.number}`
}

/** The repository and the number a `pullRequestKey` names. */
function splitPullRequestKey(key: string): { repository: string; number: number } {
  const at = key.lastIndexOf(":")
  return { repository: key.slice(0, at), number: Number(key.slice(at + 1)) }
}

/**
 * One request Ariadne works on, `GET /v1/pull-requests/{id}`, read off the
 * forge: where a pull request session's title and link come from.
 */
export function pullRequestQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.pullRequests.detail(id),
    queryFn: () => unwrap(api().GET("/v1/pull-requests/{id}", { params: { path: { id } } })),
  })
}

/**
 * One open request by its repository and number, read off the forge with
 * its checks and comments: what its panel shows, whether Ariadne works on
 * it or not. `pull_requests_changed` reads it again.
 */
export function pullRequestByKeyQueryOptions(key: string) {
  const { repository, number } = splitPullRequestKey(key)
  return queryOptions({
    queryKey: qk.pullRequests.detail(key),
    queryFn: () =>
      unwrap(
        api().GET("/v1/repositories/{id}/pull-requests/{number}", {
          params: { path: { id: repository, number } },
        }),
      ),
  })
}

/**
 * `GET /v1/pull-requests` under `filters`: the open requests the Forge
 * screen's tab lists, read live off the forge, and read again on every
 * `pull_requests_changed`.
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

/**
 * `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review` — ask
 * Ariadne to review a request of the user's own, or stop asking (029).
 * Asking starts Ariadne's work on it; the request comes back and lands in
 * the cache.
 */
export function useAskReview() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      repository_id,
      number,
      ...body
    }: {
      repository_id: string
      number: number
      asked: boolean
      model?: string
      effort?: string
      skills?: string[]
    }) =>
      unwrap(
        api().PUT("/v1/repositories/{id}/pull-requests/{number}/ariadne-review", {
          params: { path: { id: repository_id, number } },
          body,
        }),
      ),
    onSuccess: (pull) => {
      client.setQueryData(qk.pullRequests.detail(pullRequestKey(pull)), pull)
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists() })
    },
  })
}
