/**
 * The host mutations of the Hosts screen: `POST`, `PATCH` and `DELETE` of the
 * settings daemon's `/v1/hosts`. The table does not read their answers —
 * `host_updated` and `host_deleted` move it.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api, type CreateHost, LOCAL_HOST, type PatchHost, qk, unwrap } from "@/api"
import type { components } from "@/api/schema"

export type HostSetup = components["schemas"]["HostSetupDto"]

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

/** Whether a saved operation still runs in the daemon. */
function isActive(state: HostSetup["state"]): boolean {
  return state === "pending" || state === "running"
}

/**
 * `GET /v1/host-setups`. No event carries a saved setup, so the list is read
 * again every second while one operation is `pending` or `running`.
 */
export function useHostSetups() {
  return useQuery({
    queryKey: qk.hosts.setups(),
    queryFn: () => unwrap(api(LOCAL_HOST).GET("/v1/host-setups")),
    refetchInterval: (query) =>
      query.state.data?.some((one) => isActive(one.state)) ? 1000 : false,
  })
}

export function useDiscardSetup() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api(LOCAL_HOST).DELETE("/v1/hosts/{name}/setup", { params: { path: { name } } })),
    onSuccess: () => client.invalidateQueries({ queryKey: qk.hosts.setups() }),
  })
}
