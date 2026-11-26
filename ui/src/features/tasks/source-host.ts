import { useQueries } from "@tanstack/react-query"
import { api, type Host, type HostRow, qk, type TaskDto, unwrap, useHosts } from "@/api"

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

/**
 * The task rows with each worker copy of a far task left out where its source
 * copy is also listed. Both daemons list a far task under one id: the source
 * as the task it owns, the worker as the copy it runs. A worker copy stays
 * when its source host is not configured, so the task is still shown.
 */
export function withoutWorkerCopies<T extends HostRow<TaskDto>>(
  rows: readonly T[],
  daemonIds: ReadonlyMap<Host, string | undefined>,
): T[] {
  const sourceDaemonOf = (task: T) =>
    task.execution_context?.source_goal?.daemon_id ?? task.execution_context?.source_daemon_id
  const isSourceCopy = (task: T) =>
    !!task.execution_context && daemonIds.get(task.host) === sourceDaemonOf(task)
  const listedSources = new Set(
    rows
      .filter(isSourceCopy)
      .map((task) => `${task.execution_context?.execution_owner}:${task.id}`),
  )
  return rows.filter(
    (task) =>
      !task.execution_context ||
      isSourceCopy(task) ||
      !listedSources.has(`${task.execution_context.execution_owner}:${task.id}`),
  )
}
