import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateWorkflowRequest,
  cacheRow,
  dropRow,
  type Host,
  qk,
  type UpdateWorkflowRequest,
  unwrap,
} from "@/api"

export function workflowsQueryOptions(host: Host) {
  return queryOptions({
    queryKey: qk.workflows.list(host),
    queryFn: () => unwrap(api(host).GET("/v1/workflows")),
  })
}

export function useCreateWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateWorkflowRequest) =>
      unwrap(api("local").POST("/v1/workflows", { body })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, "local", workflow),
  })
}

export function useUpdateWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, body }: { name: string; body: UpdateWorkflowRequest }) =>
      unwrap(api("local").PUT("/v1/workflows/{name}", { params: { path: { name } }, body })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, "local", workflow),
  })
}

export function useResetWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api("local").POST("/v1/workflows/{name}/reset", { params: { path: { name } } })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, "local", workflow),
  })
}

export function useDeleteWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api("local").DELETE("/v1/workflows/{name}", { params: { path: { name } } })),
    onSuccess: (_result, name) => dropRow(queryClient, qk.workflows, "local", name),
  })
}
