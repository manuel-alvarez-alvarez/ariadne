// @vitest-environment jsdom

/**
 * The Stats screen against a stubbed daemon.
 *
 * What is pinned: the screen draws the six families in order, each a section
 * under its own heading, asks each `GET /v1/stats/<family>` with the filters
 * in its URL, and keeps each answer under the key `qk` names, which is the
 * one the dispatcher invalidates. What each section draws is its own test's.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { qk } from "@/api"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { StatsPage } from "./stats"

/** The six families, in the order the screen shows them. */
const FAMILIES = ["work", "time", "spend", "models", "attention", "tools"] as const

/**
 * `tools` and `attention` each answer a shaped DTO rather than the bare `{}`
 * the rest still do — an empty one holds nothing to show, the same as `{}`
 * does for them. Their sections read their own fields regardless of what
 * route this screen test is pinning, so their answer cannot be the empty
 * object either.
 */
const EMPTY_BODIES: Record<string, unknown> = {
  "/v1/stats/tools": { calls: 0, errors: 0, tools: 0, by_kind: [], top: [], other: {} },
  "/v1/stats/attention": {
    permissions: { total: 0, person_share: 0, by_decider: [] },
    flags: [],
    sessions_failed: 0,
    sessions_stalled: 0,
    exhaustions: 0,
  },
}

/** The URLs of every stats request the screen made. */
let asked: URL[] = []

function emptyBody(pathname: string): unknown {
  return EMPTY_BODIES[pathname] ?? {}
}

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
    asked.push(url)
    return jsonResponse(emptyBody(url.pathname))
  })
})

describe("StatsPage", () => {
  it("renders the six sections in order, under one heading style", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const headings = await screen.findAllByRole("heading", { level: 2 })
    expect(headings.map((heading) => heading.textContent)).toEqual([
      "Work",
      "Time",
      "Spend",
      "Models",
      "Attention",
      "Tools",
    ])
    for (const heading of headings) {
      expect(heading.className).toBe("text-sm font-medium")
    }
  })

  it("asks every family with the filters in its URL, under the key qk names", async () => {
    const { queryClient } = renderScreen(<StatsPage />, {
      route: "/stats?since=7d&repo=01JREPO",
    })

    await within(await screen.findByRole("region", { name: "Tools" })).findByText(
      "No tool ran in this span.",
    )
    const filter = { since: "7d", repo: "01JREPO" }
    for (const family of FAMILIES) {
      const url = asked.find((asked) => asked.pathname === `/v1/stats/${family}`)
      expect(url?.searchParams.get("since"), family).toBe("7d")
      expect(url?.searchParams.get("repo"), family).toBe("01JREPO")
      expect(queryClient.getQueryData(qk.stats[family](filter)), family).toEqual(
        emptyBody(`/v1/stats/${family}`),
      )
    }
  })

  it("renders an error, not the empty sentence, when the daemon refuses a read", async () => {
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
      if (url.pathname === "/v1/stats/work") {
        return errorResponse(500, "internal", "the ledger is not answering")
      }
      return jsonResponse(emptyBody(url.pathname))
    })
    renderScreen(<StatsPage />, { route: "/stats" })

    const section = await screen.findByRole("region", { name: "Work" })
    expect(await within(section).findByText("Could not load the work stats")).toBeDefined()
    expect(within(section).queryByText("Nothing got done in this span.")).toBeNull()
  })
})
