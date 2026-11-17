// @vitest-environment jsdom

/**
 * How the app reaches each host of `GET /v1/hosts`: `local` at the settings
 * base URL, a named host at its `http://` or `https://` endpoint while it is
 * `ready`, and any other named host not at all, with its reason.
 */

import { QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, describe, expect, it } from "vitest"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { api, DEFAULT_BASE_URL, eventStreamUrl, setApiBaseUrl, unwrap } from "./client"
import { ApiError } from "./errors"
import { rememberHosts, useHosts } from "./hosts"
import { createQueryClient } from "./queries"
import type { HostDto } from "./types"

afterEach(() => {
  forgetHosts()
  setApiBaseUrl(DEFAULT_BASE_URL)
})

/** The hosts as `useHosts` shows them once the settings daemon has answered `rows`. */
async function viewsOf(rows: HostDto[]) {
  stubHosts({}, rows)
  const queryClient = createQueryClient()
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
  const { result } = renderHook(() => useHosts(), { wrapper })
  await waitFor(() => expect(result.current.isPending).toBe(false))
  return result.current.hosts
}

describe("the hosts", () => {
  it("maps local to the settings base URL, whatever endpoint it lists", async () => {
    setApiBaseUrl("http://127.0.0.1:7676/")
    const [local] = await viewsOf([{ ...LOCAL, endpoint: "/run/ariadne.sock" }])
    expect(local).toMatchObject({ name: "local", url: "http://127.0.0.1:7676", unreachable: null })
  })

  it("lists a host whose endpoint is not http or https as unreachable, with its error", async () => {
    const [, ssh] = await viewsOf([
      LOCAL,
      { ...BOX, kind: "ssh", endpoint: null, status: "down", error: "ssh forward not started" },
    ])
    expect(ssh).toMatchObject({ url: null, unreachable: "ssh forward not started" })
  })

  it("lists a host that is not ready as unreachable, even at an http endpoint", async () => {
    const [, box] = await viewsOf([LOCAL, { ...BOX, status: "connecting", error: null }])
    expect(box).toMatchObject({ url: null, unreachable: "host is connecting" })
  })
})

describe("api(host)", () => {
  it("calls a ready named host at its endpoint", async () => {
    const seen = stubHosts({ box: { "/v1/goals": [] } })
    rememberHosts([LOCAL, BOX])

    await unwrap(api("box").GET("/v1/goals"))

    expect(seen.map((request) => request.url.toString())).toEqual(["http://box.test:7676/v1/goals"])
  })

  it("fails a call to an unreachable host as a network error with the host's reason", () => {
    rememberHosts([LOCAL, { ...BOX, status: "down", error: "connection refused" }])

    let failure: unknown
    try {
      api("box")
    } catch (error) {
      failure = error
    }

    expect(ApiError.is(failure) && failure.isNetworkError).toBe(true)
    expect((failure as Error).message).toContain("connection refused")
  })

  it("builds a host's stream URL with its filters, and none for an unreachable host", () => {
    rememberHosts([LOCAL, BOX, { ...BOX, name: "far", status: "down", error: "down" }])

    expect(eventStreamUrl("box", { goal: "g1" })).toBe(
      "http://box.test:7676/v1/events/stream?goal=g1",
    )
    expect(eventStreamUrl("local")).toBe("http://127.0.0.1:7676/v1/events/stream")
    expect(eventStreamUrl("far")).toBeNull()
  })
})

describe("useHosts", () => {
  it("reads GET /v1/hosts from the settings daemon, and points api(host) at each host", async () => {
    const seen = stubHosts({ box: { "/v1/goals": [] } })
    const queryClient = createQueryClient()
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )

    const { result } = renderHook(() => useHosts(), { wrapper })

    await waitFor(() =>
      expect(result.current.hosts.map((host) => host.name)).toEqual(["local", "box"]),
    )
    expect(seen[0]?.url.toString()).toBe("http://127.0.0.1:7676/v1/hosts")
    await unwrap(api("box").GET("/v1/goals"))
    expect(seen.at(-1)?.origin).toBe("http://box.test:7676")
  })
})
