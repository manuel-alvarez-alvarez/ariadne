/**
 * What the repository events do to the query cache.
 *
 * This is what keeps a second window live: nothing on the repositories screen
 * polls, and nothing there handles events itself, so a repository registered,
 * edited or removed anywhere — another window, the CLI — only shows up here
 * because these three cases reach the keys the screen reads.
 *
 * `repository_updated` is asserted twice over, because it is the one case that
 * reaches outside its own entity: a goal carries its repositories inline and
 * references them live, so an edited path is wrong in every goal that works in
 * it until the goals are read again.
 */

import { QueryClient } from "@tanstack/react-query"
import { describe, expect, it } from "vitest"

import { type DomainEvent, qk, type RepositoryDto } from "@/api"

import { dispatchDomainEvent } from "./dispatch"

const STAMP = "2026-01-01T00:00:00Z"

const REPOSITORY: RepositoryDto = {
  id: "01JREPO00000000000000ARI",
  path: "/home/me/dev/ariadne",
  base_branch: "main",
  description: null,
  created_at: STAMP,
  updated_at: STAMP,
}

/** A client with a list and a detail already in it, as an open screen has. */
function seeded(): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(qk.repositories.list(), [REPOSITORY])
  queryClient.setQueryData(qk.repositories.detail(REPOSITORY.id), REPOSITORY)
  queryClient.setQueryData(qk.goals.list(), [])
  return queryClient
}

/** Whether the entry under `key` was marked for refetching. */
function stale(queryClient: QueryClient, key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key)?.isInvalidated === true
}

function dispatch(queryClient: QueryClient, event: DomainEvent): void {
  dispatchDomainEvent(queryClient, event)
}

describe("repository events", () => {
  it("writes a created repository into its detail and refetches the list", () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(qk.repositories.list(), [])

    dispatch(queryClient, { event: "repository_created", data: REPOSITORY })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toEqual(REPOSITORY)
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })

  it("patches an edited repository in place", () => {
    const queryClient = seeded()
    const moved = { ...REPOSITORY, path: "/srv/ariadne", base_branch: "trunk" }

    dispatch(queryClient, { event: "repository_updated", data: moved })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toEqual(moved)
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })

  it("refetches the goals too, because they carry the repository they reference", () => {
    const queryClient = seeded()

    dispatch(queryClient, {
      event: "repository_updated",
      data: { ...REPOSITORY, path: "/srv/ariadne" },
    })

    expect(stale(queryClient, qk.goals.list())).toBe(true)
  })

  it("drops a removed repository rather than leaving it in the cache", () => {
    const queryClient = seeded()

    dispatch(queryClient, { event: "repository_deleted", data: { id: REPOSITORY.id } })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toBeUndefined()
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })
})
