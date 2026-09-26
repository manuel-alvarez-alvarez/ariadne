/**
 * The Permissions screen's one read and its two writes.
 *
 * `GET /v1/permissions/laya` answers with the whole settings row — there is
 * no per-field endpoint and nothing here is paginated — so there is a single
 * detail key and no list beside it. `PUT` and the refresh `POST` both answer
 * with that same row, so both write it into the one key rather than only
 * invalidating: the install they start is `installing` before the
 * `laya_updated` event on the stream has a chance to say so.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import { api, qk, type UpdateLayaRequest, unwrap } from "@/api"

/** `GET /v1/permissions/laya` — the settings, the Python check, and the install's state. */
export function layaStatusQueryOptions() {
  return queryOptions({
    queryKey: qk.permissions.laya(),
    queryFn: () => unwrap(api().GET("/v1/permissions/laya")),
  })
}

/** `PUT /v1/permissions/laya` — a partial change; an absent field stays unchanged. */
export function useUpdateLaya() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: UpdateLayaRequest) => unwrap(api().PUT("/v1/permissions/laya", { body })),
    onSuccess: (status) => queryClient.setQueryData(qk.permissions.laya(), status),
  })
}

/** `POST /v1/permissions/laya/refresh` — run the install again on the settings as they stand. */
export function useRefreshLaya() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => unwrap(api().POST("/v1/permissions/laya/refresh")),
    onSuccess: (status) => queryClient.setQueryData(qk.permissions.laya(), status),
  })
}
