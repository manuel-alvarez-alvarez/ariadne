import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateProfileRequest,
  cacheRow,
  dropRow,
  type PageFilters,
  qk,
  type Role,
  type UpdateProfileRequest,
  unwrap,
} from "@/api"

export function profilesQueryOptions(role?: Role) {
  const filters: PageFilters & { role?: Role } = role ? { role } : {}
  return queryOptions({
    queryKey: qk.profiles.list(filters),
    queryFn: () => unwrap(api().GET("/v1/profiles", { params: { query: { role } } })),
  })
}

export function modelsQueryOptions() {
  return queryOptions({
    queryKey: qk.models.list(),
    queryFn: () => unwrap(api().GET("/v1/models")),
  })
}

export function useCreateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateProfileRequest) => unwrap(api().POST("/v1/profiles", { body })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}

export function useUpdateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateProfileRequest }) =>
      unwrap(api().PUT("/v1/profiles/{id}", { params: { path: { id } }, body })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}

export function useDeleteProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/profiles/{id}", { params: { path: { id } } })),
    onSuccess: (_result, id) => dropRow(queryClient, qk.profiles, id),
  })
}

export function useResetSystemPrompt() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/profiles/{id}/system-prompt/reset", { params: { path: { id } } })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}
