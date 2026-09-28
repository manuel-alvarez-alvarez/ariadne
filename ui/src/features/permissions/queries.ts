/**
 * The Permissions screen's reads and writes: the AI tab's one settings row,
 * and the Learned tab's list and its three writes.
 *
 * `GET /v1/permissions/ai` answers with the whole settings row — there is
 * no per-field endpoint and nothing here is paginated — so there is a single
 * detail key and no list beside it. `PUT` and the refresh `POST` both answer
 * with that same row, so both write it into the one key rather than only
 * invalidating: the install they start is `installing` before the
 * `ai_permissions_updated` event on the stream has a chance to say so.
 *
 * The learned approvals follow the app-wide list/detail convention instead:
 * `GET /v1/permissions/learned` answers `{ items }`, unwrapped here so every
 * caller reads a plain array, and each write patches the row into the cache
 * itself (`cacheRow` / `dropRow`) for the same reason the repository screen
 * does — REST can land before the `learned_permission_*` event does.
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import {
  api,
  type CreateLearnedPermissionRequest,
  cacheRow,
  dropRow,
  qk,
  type TestAiPermissionRequest,
  type UpdateAiPermissionsRequest,
  type UpdateLearnedPermissionRequest,
  unwrap,
} from "@/api"

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

/** `POST /v1/permissions/ai/test` — score one request, without recording anything. */
export function useTestAiPermission() {
  return useMutation({
    mutationFn: (body: TestAiPermissionRequest) =>
      unwrap(api().POST("/v1/permissions/ai/test", { body })),
  })
}

/** `GET /v1/permissions/learned` — every approval, or one repository's. */
export function learnedPermissionsQueryOptions(filters: { repository?: string } = {}) {
  return queryOptions({
    queryKey: qk.learnedPermissions.list(filters),
    queryFn: () =>
      unwrap(
        api().GET("/v1/permissions/learned", {
          params: { query: { repository: filters.repository } },
        }),
      ).then((response) => response.items),
  })
}

/** `GET /v1/permissions/learned/{id}` — one approval, for the detail view. */
export function learnedPermissionQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.learnedPermissions.detail(id),
    queryFn: () => unwrap(api().GET("/v1/permissions/learned/{id}", { params: { path: { id } } })),
  })
}

/** `POST /v1/permissions/learned` — a manual approval, ahead of anything an agent runs. */
export function useCreateLearnedPermission() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateLearnedPermissionRequest) =>
      unwrap(api().POST("/v1/permissions/learned", { body })),
    onSuccess: (row) => cacheRow(queryClient, qk.learnedPermissions, row),
  })
}

/** `PUT /v1/permissions/learned/{id}` — the tool name and the kind; nothing else moves. */
export function useUpdateLearnedPermission() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateLearnedPermissionRequest }) =>
      unwrap(api().PUT("/v1/permissions/learned/{id}", { params: { path: { id } }, body })),
    onSuccess: (row) => cacheRow(queryClient, qk.learnedPermissions, row),
  })
}

/** `DELETE /v1/permissions/learned/{id}` — the next matching request is asked again. */
export function useDeleteLearnedPermission() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/permissions/learned/{id}", { params: { path: { id } } })),
    onSuccess: (_result, id) => dropRow(queryClient, qk.learnedPermissions, id),
  })
}
