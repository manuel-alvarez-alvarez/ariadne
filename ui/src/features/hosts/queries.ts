/**
 * The host mutations of the Hosts screen: `POST`, `PATCH` and `DELETE` of the
 * settings daemon's `/v1/hosts`. The table does not read their answers —
 * `host_updated` and `host_deleted` move it.
 */

import { useMutation } from "@tanstack/react-query"

import { api, type CreateHost, LOCAL_HOST, type PatchHost, unwrap } from "@/api"

export function useCreateHost() {
  return useMutation({
    mutationFn: (body: CreateHost) => unwrap(api(LOCAL_HOST).POST("/v1/hosts", { body })),
  })
}

export function useUpdateHost() {
  return useMutation({
    mutationFn: ({ name, body }: { name: string; body: PatchHost }) =>
      unwrap(api(LOCAL_HOST).PATCH("/v1/hosts/{name}", { params: { path: { name } }, body })),
  })
}

export function useDeleteHost() {
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api(LOCAL_HOST).DELETE("/v1/hosts/{name}", { params: { path: { name } } })),
  })
}
