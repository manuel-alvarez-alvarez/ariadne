import { QueryClient } from "@tanstack/react-query"
import { describe, expect, it } from "vitest"

import { cacheRow, dropRow, optimisticStatus, restoreCache } from "./queries"
import { qk } from "./query-keys"

const RUNNING = { id: "s1", status: "running", model: "stub:s1" }
const OTHER = { id: "s2", status: "running", model: "stub:s2" }

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
