/**
 * The profile screen's reads and writes.
 *
 * Reads follow the app-wide key convention so the SSE dispatcher keeps them
 * live: `profile_created` / `profile_updated` / `profile_deleted` patch
 * `profiles.detail` and invalidate `profiles.lists`, which is every list here
 * whatever role it is filtered by.
 *
 * The mutations then do the same thing to the cache themselves. That is not
 * redundant with the stream: REST can be up while the stream is down (the amber
 * connection state), and a create that only showed up once the stream came back
 * would look broken.
 */

import type { QueryClient } from "@tanstack/react-query"
import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateProfileRequest,
  type PageFilters,
  type ProfileDto,
  qk,
  type Role,
  type UpdateProfileRequest,
  unwrap,
} from "@/api"

/**
 * `GET /v1/profiles`, narrowed to one role by the daemon when given.
 *
 * The role goes into the key as a filter like any other: TanStack hashes the
 * filter object structurally, so each role gets its own cache entry and all of
 * them sit under `qk.profiles.lists()`.
 */
export function profilesQueryOptions(role?: Role) {
  const filters: PageFilters & { role?: Role } = role ? { role } : {}
  return queryOptions({
    queryKey: qk.profiles.list(filters),
    queryFn: () => unwrap(api().GET("/v1/profiles", { params: { query: { role } } })),
  })
}

export function useCreateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateProfileRequest) => unwrap(api().POST("/v1/profiles", { body })),
    onSuccess: (profile) => cacheProfile(queryClient, profile),
  })
}

export function useUpdateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateProfileRequest }) =>
      unwrap(api().PUT("/v1/profiles/{id}", { params: { path: { id } }, body })),
    onSuccess: (profile) => cacheProfile(queryClient, profile),
  })
}

export function useDeleteProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/profiles/{id}", { params: { path: { id } } })),
    onSuccess: (_result, id) => {
      queryClient.removeQueries({ queryKey: qk.profiles.detail(id) })
      void queryClient.invalidateQueries({ queryKey: qk.profiles.lists() })
    },
  })
}

/** What the dispatcher does for a `profile_created` / `profile_updated` event. */
function cacheProfile(queryClient: QueryClient, profile: ProfileDto): void {
  queryClient.setQueryData(qk.profiles.detail(profile.id), profile)
  void queryClient.invalidateQueries({ queryKey: qk.profiles.lists() })
}
