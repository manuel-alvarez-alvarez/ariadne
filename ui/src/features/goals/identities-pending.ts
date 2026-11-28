import { useQueries } from "@tanstack/react-query"
import { api, qk, unwrap, useHosts } from "@/api"

/**
 * Whether a daemon identity read is still under way. A host with no address,
 * or one whose read failed, is not pending: its identity stays unknown and the
 * rows that name it are shown.
 */
export function useIdentitiesPending(enabled: boolean): boolean {
  const { hosts } = useHosts()
  const reads = useQueries({
    queries: hosts.map((host) => ({
      queryKey: qk.remoteTaskProtocol(host.name, host.endpoint ?? ""),
      queryFn: () => unwrap(api(host.name).GET("/v1/remote-tasks/protocol")),
      enabled: enabled && host.url !== null,
      retry: false,
      staleTime: Number.POSITIVE_INFINITY,
      gcTime: Number.POSITIVE_INFINITY,
    })),
  })
  return reads.some((read) => read.isLoading)
}
