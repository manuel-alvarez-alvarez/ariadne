import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateWorkflowRequest,
  cacheRow,
  dropRow,
  qk,
  type UpdateWorkflowRequest,
  unwrap,
} from "@/api"

export function workflowsQueryOptions() {
  return queryOptions({
    queryKey: qk.workflows.list(),
    queryFn: () => unwrap(api().GET("/v1/workflows")),
  })
}

export function useCreateWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateWorkflowRequest) => unwrap(api().POST("/v1/workflows", { body })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, workflow),
  })
}

export function useUpdateWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, body }: { name: string; body: UpdateWorkflowRequest }) =>
      unwrap(api().PUT("/v1/workflows/{name}", { params: { path: { name } }, body })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, workflow),
  })
}

export function useResetWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api().POST("/v1/workflows/{name}/reset", { params: { path: { name } } })),
    onSuccess: (workflow) => cacheRow(queryClient, qk.workflows, workflow),
  })
}

export function useDeleteWorkflow() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api().DELETE("/v1/workflows/{name}", { params: { path: { name } } })),
    onSuccess: (_result, name) => dropRow(queryClient, qk.workflows, name),
  })
}
