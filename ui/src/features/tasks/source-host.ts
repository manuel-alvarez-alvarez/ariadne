import { useQueries } from "@tanstack/react-query"
import { api, type Host, qk, unwrap, useHosts } from "@/api"

/** Discover identities for configured connections and retain them through outages. */
export function useDaemonIdentities(enabled: boolean): Map<Host, string | undefined> {
  const { hosts } = useHosts()
  const identities = useQueries({
    queries: hosts.map((host) => ({
      queryKey: qk.remoteTaskProtocol(host.name, host.endpoint ?? ""),
      queryFn: () => unwrap(api(host.name).GET("/v1/remote-tasks/protocol")),
      enabled: enabled && host.url !== null,
      retry: false,
      staleTime: Number.POSITIVE_INFINITY,
      gcTime: Number.POSITIVE_INFINITY,
    })),
  })
  return new Map(
    hosts.map((host, index) => [
      host.name,
      identities[index]?.data?.protocol === "remote-tasks-v1"
        ? identities[index]?.data?.daemon_id
        : undefined,
    ]),
  )
}

/** Find a configured connection by its persistent daemon identity. */
export function useSourceHost(daemonId: string | undefined): Host | undefined {
  const identities = useDaemonIdentities(!!daemonId)
  return [...identities].find(([, id]) => id === daemonId && id !== undefined)?.[0]
}
