/**
 * The Permissions screen's one read and its two writes.
 *
 * `GET /v1/permissions/ai` answers with the whole settings row — there is
 * no per-field endpoint and nothing here is paginated — so there is a single
 * detail key and no list beside it. `PUT` and the refresh `POST` both answer
 * with that same row, so both write it into the one key rather than only
 * invalidating: the install they start is `installing` before the
 * `ai_permissions_updated` event on the stream has a chance to say so.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import { api, qk, type UpdateAiPermissionsRequest, unwrap } from "@/api"

/** `GET /v1/permissions/ai` — the settings, the Python check, and the install's state. */
export function aiPermissionsStatusQueryOptions() {
  return queryOptions({
    queryKey: qk.permissions.ai(),
    queryFn: () => unwrap(api().GET("/v1/permissions/ai")),
  })
}

/** `PUT /v1/permissions/ai` — a partial change; an absent field stays unchanged. */
export function useUpdateAiPermissions() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateAiPermissionsRequest) =>
      unwrap(api().PUT("/v1/permissions/ai", { body })),
    onSuccess: (status) => queryClient.setQueryData(qk.permissions.ai(), status),
  })
}

/** `POST /v1/permissions/ai/refresh` — run the install again on the settings as they stand. */
export function useRefreshAiPermissions() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => unwrap(api().POST("/v1/permissions/ai/refresh")),
    onSuccess: (status) => queryClient.setQueryData(qk.permissions.ai(), status),
  })
}
