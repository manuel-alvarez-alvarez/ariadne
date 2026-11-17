// @vitest-environment jsdom

/** One list query per host: rows tagged with their host, one error per host that failed. */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, expect, it } from "vitest"

import { aGoal } from "@/test/fixtures"
import { errorResponse } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { api, unwrap } from "./client"
import { rememberHosts } from "./hosts"
import { useHostLists } from "./queries"
import { qk } from "./query-keys"

afterEach(() => {
  forgetHosts()
})

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
}

it("tags each host's rows with the host, and reports the host that failed on its own", async () => {
  const mine = aGoal({ id: "g-local", title: "Mine" })
  const theirs = aGoal({ id: "g-box", title: "Theirs" })
  stubHosts({ local: { "/v1/goals": [mine] }, box: { "/v1/goals": [theirs] } })
  rememberHosts([LOCAL, BOX, { ...BOX, name: "far", status: "down", error: "connection refused" }])

  const { result } = renderHook(
    () =>
      useHostLists(["local", "box", "far"], (host) => ({
        queryKey: qk.goals.list(host),
        queryFn: () => unwrap(api(host).GET("/v1/goals")),
      })),
    { wrapper },
  )

  await waitFor(() => expect(result.current.rows).toHaveLength(2))
  expect(result.current.rows).toEqual([
    { ...mine, host: "local" },
    { ...theirs, host: "box" },
  ])
  await waitFor(() => expect(result.current.errors).toHaveLength(1))
  expect(result.current.errors[0]?.host).toBe("far")
  expect(result.current.errors[0]?.error.message).toContain("connection refused")
})

it("keeps the other hosts' rows when one host answers with an error", async () => {
  const mine = aGoal({ id: "g-local" })
  stubHosts({
    local: { "/v1/goals": [mine] },
    box: { "/v1/goals": () => errorResponse(503, "unavailable", "daemon is busy") },
  })
  rememberHosts([LOCAL, BOX])

  const { result } = renderHook(
    () =>
      useHostLists(["local", "box"], (host) => ({
        queryKey: qk.goals.list(host),
        queryFn: () => unwrap(api(host).GET("/v1/goals")),
      })),
    { wrapper },
  )

  await waitFor(() => expect(result.current.errors.map((one) => one.host)).toEqual(["box"]))
  expect(result.current.rows).toEqual([{ ...mine, host: "local" }])
  expect(result.current.isPending).toBe(false)
})
