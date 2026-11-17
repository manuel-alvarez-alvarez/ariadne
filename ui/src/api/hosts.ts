/**
 * The hosts the app reads: the settings daemon's `GET /v1/hosts`.
 *
 * The settings daemon is `local`, at the base URL the settings store holds.
 * Every other row is a named host from that daemon's `config.toml`, and the
 * app reaches it at its `endpoint` — but only an `http://` or `https://`
 * endpoint of a host that is `ready`. A socket path, a forward that is not up
 * or a host that is down is listed as unreachable, with its `error`.
 *
 * What a client needs to call a host is kept beside `api(host)` in
 * `client.ts`; {@link rememberHosts} is what writes it, from the query here
 * and from the dispatcher's `host_updated`.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, hostBaseUrl, LOCAL_HOST, normalizeBaseUrl, setHostAddresses, unwrap } from "./client"
import { qk } from "./query-keys"
import type { HostDto } from "./types"

/** One host as the app shows it: the daemon's row and what it means here. */
export interface HostView extends HostDto {
  /** The base URL the app calls, or null for a host it cannot reach. */
  url: string | null
  /** Why the app cannot reach the host; null for a reachable one. */
  unreachable: string | null
}

/** What `local` is until the settings daemon has answered. */
const LOCAL_ROW: HostDto = {
  name: LOCAL_HOST,
  kind: "local",
  status: "ready",
  sync: { state: "in_sync", missing_repositories: [], skipped_permissions: 0 },
}

/** How the app reaches one host's row, or why it cannot. */
function reach(host: HostDto): Pick<HostView, "url" | "unreachable"> {
  // `local` is the settings daemon, wherever the settings point: that is the
  // daemon that answered, whatever endpoint it lists for itself.
  if (host.name === LOCAL_HOST) return { url: hostBaseUrl(LOCAL_HOST), unreachable: null }
  const endpoint = host.endpoint ?? ""
  if (!/^https?:\/\//.test(endpoint)) {
    return {
      url: null,
      unreachable:
        host.error ??
        (endpoint ? `${endpoint} is not an http:// or https:// endpoint` : "no endpoint yet"),
    }
  }
  if (host.status !== "ready")
    return { url: null, unreachable: host.error ?? `host is ${host.status}` }
  return { url: normalizeBaseUrl(endpoint), unreachable: null }
}

/** Every host as the app shows it, `local` first even before the daemon answers. */
function hostViews(rows: readonly HostDto[] | undefined): HostView[] {
  const listed = rows?.length ? rows : [LOCAL_ROW]
  const withLocal = listed.some((row) => row.name === LOCAL_HOST) ? listed : [LOCAL_ROW, ...listed]
  return withLocal.map((row) => ({ ...row, ...reach(row) }))
}

/** Point `api(host)` at every host of `rows`. */
export function rememberHosts(rows: readonly HostDto[]): void {
  setHostAddresses(
    hostViews(rows).map((host) => ({ name: host.name, url: host.url, error: host.unreachable })),
  )
}

function hostsQueryOptions() {
  return queryOptions({
    queryKey: qk.hosts.list(),
    queryFn: async ({ client }) => {
      const rows = await unwrap(api(LOCAL_HOST).GET("/v1/hosts"))
      const views = hostViews(rows)
      const before = new Map(views.map((host) => [host.name, hostBaseUrl(host.name)]))
      rememberHosts(rows)
      for (const host of views) {
        if (host.name === LOCAL_HOST || host.url === null) continue
        // A host that came up or moved since the last list — a `host_updated`
        // missed while `local`'s stream was down — answers every read again.
        // Otherwise only a read that ran before its address was known failed
        // for that alone, and a network error is not retried: read it again.
        const moved = before.get(host.name) !== host.url
        void client.invalidateQueries({
          queryKey: qk.host(host.name),
          predicate: moved ? undefined : (query) => query.state.status === "error",
        })
      }
      return rows
    },
  })
}

/**
 * The hosts, live through `host_updated`: `local` alone until the settings
 * daemon answers, and when it cannot.
 */
export function useHosts(): { hosts: HostView[]; isPending: boolean; error: Error | null } {
  const query = useQuery(hostsQueryOptions())
  return { hosts: hostViews(query.data), isPending: query.isPending, error: query.error }
}
