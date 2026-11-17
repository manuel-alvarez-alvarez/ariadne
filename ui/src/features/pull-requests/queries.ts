import {
  InfiniteQueryObserver,
  infiniteQueryOptions,
  queryOptions,
  useMutation,
  useQueryClient,
} from "@tanstack/react-query"
import { useCallback, useMemo, useRef, useSyncExternalStore } from "react"
import { api, type Host, type HostRow, type PullRequestDto, qk, unwrap } from "@/api"

/**
 * What identifies a request on the screen: its repository and its number.
 * Ariadne's id names only the requests it works on, and most it does not.
 */
export function pullRequestKey(pull: Pick<PullRequestDto, "repository_id" | "number">): string {
  return `${pull.repository_id}:${pull.number}`
}

/** The repository and the number a `pullRequestKey` names. */
function splitPullRequestKey(key: string): { repository: string; number: number } {
  const at = key.lastIndexOf(":")
  return { repository: key.slice(0, at), number: Number(key.slice(at + 1)) }
}

/**
 * One request Ariadne works on, `GET /v1/pull-requests/{id}`, read off the
 * forge: where a pull request session's title and link come from.
 */
export function pullRequestQueryOptions(host: Host, id: string) {
  return queryOptions({
    queryKey: qk.pullRequests.detail(host, id),
    queryFn: () => unwrap(api(host).GET("/v1/pull-requests/{id}", { params: { path: { id } } })),
  })
}

/**
 * One open request by its repository and number, read off the forge with
 * its checks and comments: what its panel shows, whether Ariadne works on
 * it or not. `pull_requests_changed` reads it again.
 */
export function pullRequestByKeyQueryOptions(host: Host, key: string) {
  const { repository, number } = splitPullRequestKey(key)
  return queryOptions({
    queryKey: qk.pullRequests.detail(host, key),
    queryFn: () =>
      unwrap(
        api(host).GET("/v1/repositories/{id}/pull-requests/{number}", {
          params: { path: { id: repository, number } },
        }),
      ),
  })
}

/**
 * `GET /v1/pull-requests` under `filters`: the open requests the Forge
 * screen's tab lists, from the repository's last fetch, a page of 25 at a
 * time, the most recently updated first. `q` is the tab's text filter, which
 * the daemon applies to every request before it cuts the pages. Every
 * `pull_requests_changed` reads the list again from its first page.
 */
function pullRequestsQueryOptions(
  host: Host,
  filters: {
    repo?: string
    role?: string
    requested?: boolean
    q?: string
  },
) {
  return infiniteQueryOptions({
    queryKey: qk.pullRequests.list(host, filters),
    queryFn: ({ pageParam }) =>
      unwrap(
        api(host).GET("/v1/pull-requests", {
          params: { query: { ...filters, cursor: pageParam ?? undefined } },
        }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor ?? null,
  })
}

/**
 * `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-review` — ask
 * Ariadne to review any open request, or stop asking (029).
 * Asking starts Ariadne's work on it; the request comes back and lands in
 * the cache.
 */
export function useAskReview() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      host,
      repository_id,
      number,
      ...body
    }: {
      /** The host the request is asked on, and the id of its repository there. */
      host: Host
      repository_id: string
      number: number
      asked: boolean
      model?: string
      effort?: string
      skills?: string[]
    }) =>
      unwrap(
        api(host).PUT("/v1/repositories/{id}/pull-requests/{number}/ariadne-review", {
          params: { path: { id: repository_id, number } },
          body,
        }),
      ),
    onSuccess: (pull, { host }) => {
      client.setQueryData(qk.pullRequests.detail(host, pullRequestKey(pull)), pull)
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists(host) })
    },
  })
}

/**
 * `PUT /v1/repositories/{id}/pull-requests/{number}/ariadne-babysit` — ask
 * Ariadne to keep any open request by hand, or stop asking
 * (032). Asking starts a keeper session on it; the request comes back and
 * lands in the cache.
 */
export function useAskBabysit() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({
      host,
      repository_id,
      number,
      ...body
    }: {
      /** The host the request is asked on, and the id of its repository there. */
      host: Host
      repository_id: string
      number: number
      asked: boolean
      model?: string
      effort?: string
      skills?: string[]
    }) =>
      unwrap(
        api(host).PUT("/v1/repositories/{id}/pull-requests/{number}/ariadne-babysit", {
          params: { path: { id: repository_id, number } },
          body,
        }),
      ),
    onSuccess: (pull, { host }) => {
      client.setQueryData(qk.pullRequests.detail(host, pullRequestKey(pull)), pull)
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists(host) })
    },
  })
}

/** Ask for a reviewer round on the current head. */
export function useReviewRound() {
  return useMutation({
    mutationFn: ({
      host,
      repository_id,
      number,
    }: {
      host: Host
      repository_id: string
      number: number
    }) =>
      unwrap(
        api(host).POST("/v1/repositories/{id}/pull-requests/{number}/review-round", {
          params: { path: { id: repository_id, number } },
        }),
      ),
  })
}

/** Ask for a keeper turn on the current head. */
export function useKeeperTurn() {
  return useMutation({
    mutationFn: ({
      host,
      repository_id,
      number,
    }: {
      host: Host
      repository_id: string
      number: number
    }) =>
      unwrap(
        api(host).POST("/v1/repositories/{id}/pull-requests/{number}/keeper-turn", {
          params: { path: { id: repository_id, number } },
        }),
      ),
  })
}

/** The filters of {@link pullRequestsQueryOptions}, per host: the host's own repository id. */
type PullRequestFilters = Parameters<typeof pullRequestsQueryOptions>[1]

/**
 * The open requests of several hosts as one list: one infinite query per
 * host, each its own cache entry, merged most recently updated first. The
 * next page asks every host that has one. A host that fails keeps its error
 * apart and leaves the others on screen; a new filter keeps the rows it had
 * until its own answer is in.
 */
export function usePullRequestLists(hosts: readonly { host: Host; filters: PullRequestFilters }[]) {
  const client = useQueryClient()
  const key = JSON.stringify(hosts)
  // One observer per host, made again only when the hosts or a filter change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is `hosts`, spelled stably
  const { observers, names } = useMemo(
    () => ({
      names: hosts.map(({ host }) => host),
      observers: hosts.map(
        ({ host, filters }) =>
          new InfiniteQueryObserver(client, pullRequestsQueryOptions(host, filters)),
      ),
    }),
    [client, key],
  )
  const subscribe = useCallback(
    (notify: () => void) => {
      const stops = observers.map((observer) => observer.subscribe(notify))
      return () => {
        for (const stop of stops) stop()
      }
    },
    [observers],
  )
  const last = useRef<{
    observers: typeof observers
    results: ReturnType<(typeof observers)[number]["getCurrentResult"]>[]
  } | null>(null)
  // The same object until a result changes, as `useSyncExternalStore` needs.
  const snapshot = () => {
    const results = observers.map((observer) => observer.getCurrentResult())
    const before = last.current
    if (
      before &&
      before.observers === observers &&
      before.results.every((result, index) => result === results[index])
    ) {
      return before
    }
    last.current = { observers, results }
    return last.current
  }
  const { results } = useSyncExternalStore(subscribe, snapshot)

  const rows = useMemo(
    () =>
      results
        .flatMap((result, index) =>
          (result.data?.pages ?? []).flatMap((page) =>
            page.pull_requests.map((row) => ({ ...row, host: names[index] as Host })),
          ),
        )
        .sort((a, b) => (a.updated_at < b.updated_at ? 1 : a.updated_at > b.updated_at ? -1 : 0)),
    [results, names],
  )
  const kept = useRef<HostRow<PullRequestDto>[] | undefined>(undefined)
  const answered = results.some((result) => result.data !== undefined) || results.length === 0
  if (answered) kept.current = rows
  return {
    /** Every host's rows, or the last answer while a new filter waits for its own. */
    rows: answered ? rows : kept.current,
    errors: results.flatMap((result, index) =>
      result.error ? [{ host: names[index] as Host, error: result.error as Error }] : [],
    ),
    isPending:
      !answered && kept.current === undefined && results.every((result) => result.isPending),
    hasNextPage: results.some((result) => result.hasNextPage),
    isFetchingNextPage: results.some((result) => result.isFetchingNextPage),
    pages: results.reduce((count, result) => count + (result.data?.pages.length ?? 0), 0),
    fetchNextPage: () => {
      for (const result of results) {
        if (result.hasNextPage && !result.isFetchingNextPage) void result.fetchNextPage()
      }
    },
    refetch: () => {
      for (const result of results) if (result.error) void result.refetch()
    },
  }
}
