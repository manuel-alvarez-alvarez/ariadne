import { QueryClient } from "@tanstack/react-query"
import { describe, expect, it } from "vitest"

import { optimisticStatus, restoreCache } from "./optimistic"
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
  it("flips the row in the detail entry and in every cached list", async () => {
    const queryClient = seed()

    await optimisticStatus(queryClient, {
      detailKey: qk.sessions.detail("s1"),
      listsKey: qk.sessions.lists(),
      id: "s1",
      status: "exited",
    })

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toMatchObject({ status: "exited" })
    expect(queryClient.getQueryData(qk.sessions.list({}))).toEqual([
      { ...RUNNING, status: "exited" },
      // Every other row in the same list is left exactly as it was.
      OTHER,
    ])
    expect(queryClient.getQueryData(qk.sessions.list({ task: "t1" }))).toEqual([
      { ...RUNNING, status: "exited" },
    ])
  })

  it("keeps the rest of the row, so a card does not lose fields it renders", async () => {
    const queryClient = seed()

    await optimisticStatus(queryClient, {
      detailKey: qk.sessions.detail("s1"),
      listsKey: qk.sessions.lists(),
      id: "s1",
      status: "exited",
    })

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toEqual({
      ...RUNNING,
      status: "exited",
    })
  })

  it("restores what it found when the daemon refuses", async () => {
    const queryClient = seed()

    const snapshot = await optimisticStatus(queryClient, {
      detailKey: qk.sessions.detail("s1"),
      listsKey: qk.sessions.lists(),
      id: "s1",
      status: "exited",
    })
    restoreCache(queryClient, snapshot)

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toEqual(RUNNING)
    expect(queryClient.getQueryData(qk.sessions.list({}))).toEqual([RUNNING, OTHER])
    expect(queryClient.getQueryData(qk.sessions.list({ task: "t1" }))).toEqual([RUNNING])
  })

  it("patches nothing it was not holding, and invents no entry on rollback", async () => {
    const queryClient = new QueryClient()

    const snapshot = await optimisticStatus(queryClient, {
      detailKey: qk.sessions.detail("s1"),
      listsKey: qk.sessions.lists(),
      id: "s1",
      status: "exited",
    })
    restoreCache(queryClient, snapshot)

    expect(queryClient.getQueryData(qk.sessions.detail("s1"))).toBeUndefined()
    expect(queryClient.getQueryCache().getAll()).toHaveLength(0)
  })
})
