import { QueryClient } from "@tanstack/react-query"
import { describe, expect, it } from "vitest"

import { ApiError } from "./errors"
import { cacheRow, dropRow, optimisticStatus, restoreCache, shouldRetryQuery } from "./queries"
import { qk } from "./query-keys"

const RUNNING = { id: "s1", status: "running", tmux_session: "ariadne-s1" }
const OTHER = { id: "s2", status: "running", tmux_session: "ariadne-s2" }

function seed(): QueryClient {
  const queryClient = new QueryClient()
  queryClient.setQueryData(qk.sessions.detail("s1"), RUNNING)
  queryClient.setQueryData(qk.sessions.list({}), [RUNNING, OTHER])
  queryClient.setQueryData(qk.sessions.list({ task: "t1" }), [RUNNING])
  return queryClient
}

describe("optimisticStatus", () => {
  it("flips the row in the detail entry and in every cached list, keeping its other fields", async () => {
    const queryClient = seed()

    await optimisticStatus(queryClient, qk.sessions, "s1", "exited")

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toEqual({
      ...RUNNING,
      status: "exited",
    })
    expect(queryClient.getQueryData(qk.sessions.list({}))).toEqual([
      { ...RUNNING, status: "exited" },
      // Every other row in the same list is left exactly as it was.
      OTHER,
    ])
    expect(queryClient.getQueryData(qk.sessions.list({ task: "t1" }))).toEqual([
      { ...RUNNING, status: "exited" },
    ])
  })

  it("restores what it found when the daemon refuses", async () => {
    const queryClient = seed()

    const snapshot = await optimisticStatus(queryClient, qk.sessions, "s1", "exited")
    restoreCache(queryClient, snapshot)

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toEqual(RUNNING)
    expect(queryClient.getQueryData(qk.sessions.list({}))).toEqual([RUNNING, OTHER])
    expect(queryClient.getQueryData(qk.sessions.list({ task: "t1" }))).toEqual([RUNNING])
  })

  it("patches nothing it was not holding, and invents no entry on rollback", async () => {
    const queryClient = new QueryClient()

    const snapshot = await optimisticStatus(queryClient, qk.sessions, "s1", "exited")
    restoreCache(queryClient, snapshot)

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toBeUndefined()
    expect(queryClient.getQueryCache().getAll()).toHaveLength(0)
  })
})

describe("cacheRow", () => {
  it("writes the answered row into its detail entry and stales the lists", () => {
    const queryClient = seed()
    const renamed = { ...RUNNING, status: "exited" }

    cacheRow(queryClient, qk.sessions, renamed)

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toEqual(renamed)
    // The lists are refetched rather than patched — the daemon decides the
    // order and which of them the row still belongs to.
    expect(queryClient.getQueryState(qk.sessions.list({}))?.isInvalidated).toBe(true)
  })
})

describe("dropRow", () => {
  it("drops the row's own entry and stales the lists", () => {
    const queryClient = seed()

    dropRow(queryClient, qk.sessions, "s1")

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toBeUndefined()
    expect(queryClient.getQueryState(qk.sessions.list({}))?.isInvalidated).toBe(true)
  })
})

describe("shouldRetryQuery", () => {
  it("gives up at once on a daemon that cannot be reached", () => {
    // Every screen is looking at the same dead daemon, so they all have to
    // give up together — the connection banner is what says why.
    const down = ApiError.network(new Error("connection refused"))
    expect(shouldRetryQuery(0, down)).toBe(false)
  })

  it("gives up at once on a 4xx, which will not fix itself", () => {
    expect(
      shouldRetryQuery(0, new ApiError({ status: 404, code: "task_not_found", message: "gone" })),
    ).toBe(false)
  })

  it("retries a 5xx twice, which is transient", () => {
    const flaky = new ApiError({ status: 503, code: "http_error", message: "503 Unavailable" })
    expect(shouldRetryQuery(0, flaky)).toBe(true)
    expect(shouldRetryQuery(1, flaky)).toBe(true)
    expect(shouldRetryQuery(2, flaky)).toBe(false)
  })

  it("retries anything that is not an ApiError at all", () => {
    expect(shouldRetryQuery(0, new Error("boom"))).toBe(true)
    expect(shouldRetryQuery(2, new Error("boom"))).toBe(false)
  })
})
