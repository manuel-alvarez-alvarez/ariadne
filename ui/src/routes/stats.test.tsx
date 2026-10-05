// @vitest-environment jsdom

/**
 * The Stats screen against a stubbed daemon.
 *
 * What is pinned: the screen draws the five families in order, each a section
 * under its own heading, asks each `GET /v1/stats/<family>` with the filters
 * in its URL, and keeps each answer under the key `qk` names, which is the
 * one the dispatcher invalidates. Above them stands the key-figure row, read
 * off the same answers, and the sections lie in a card grid. What each
 * section draws is its own test's.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { qk } from "@/api"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { StatsPage } from "./stats"

/** The five families, in the order the screen shows them. */
const FAMILIES = ["models", "work", "time", "spend", "attention"] as const

/**
 * `attention` answers a shaped DTO rather than the bare `{}`
 * the rest still do — an empty one holds nothing to show, the same as `{}`
 * does for them. Their sections read their own fields regardless of what
 * route this screen test is pinning, so their answer cannot be the empty
 * object either.
 */
const EMPTY_BODIES: Record<string, unknown> = {
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
  it("renders the five sections in order, under one heading style", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const headings = await screen.findAllByRole("heading", { level: 2 })
    expect(headings.map((heading) => heading.textContent)).toEqual([
      "Models",
      "Work",
      "Time",
      "Spend",
      "Attention",
    ])
    for (const heading of headings) {
      expect(heading.className).toBe("text-sm font-medium")
    }
  })

  it("asks every family with the filters in its URL, under the key qk names", async () => {
    const { queryClient } = renderScreen(<StatsPage />, {
      route: "/stats?since=7d&repo=01JREPO",
    })

    await within(await screen.findByRole("region", { name: "Attention" })).findByText(
      "Nothing needed you in this span.",
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

  it("leads with the key figures, read off the work, spend and models answers", async () => {
    const bodies: Record<string, unknown> = {
      "/v1/stats/work": {
        totals: { tasks_finished: 7, finish_rate: 0.7, median_goal_lead_time_secs: 3_600 },
        buckets: [],
      },
      "/v1/stats/spend": {
        totals: { sessions: 0, input_tokens: 1_200_000, output_tokens: 300_000 },
      },
      "/v1/stats/models": {
        items: [
          { seat: "other", model: "a", interventions: { total: 4, person_secs: 600 } },
          { seat: "other", model: "b", interventions: { total: 3, person_secs: 1_200 } },
        ],
      },
    }
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
      return jsonResponse(bodies[url.pathname] ?? emptyBody(url.pathname))
    })
    renderScreen(<StatsPage />, { route: "/stats" })

    const row = await screen.findByRole("region", { name: "Key figures" })
    await within(row).findByText("30m")
    const figures = within(row)
      .getAllByRole("term")
      .map((term) => [term.textContent, term.nextElementSibling?.textContent])
    expect(figures).toEqual([
      ["Tasks finished", "7"],
      ["Finish rate", "70.0%"],
      ["Median goal lead time", "1h"],
      ["Total tokens", "1.5M"],
      ["Interventions", "7"],
      ["Person time", "30m"],
    ])
  })

  it("lays Models across the full width, and the other four out as a two-column card grid", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const models = await screen.findByRole("region", { name: "Models" })
    const grid = models.parentElement
    expect(grid?.className).toBe("grid grid-cols-1 items-stretch gap-4 xl:grid-cols-2")
    expect(models.className).toContain("xl:col-span-2")
    for (const name of ["Models", "Work", "Time", "Spend", "Attention"]) {
      const card = screen.getByRole("region", { name })
      expect(card.parentElement, name).toBe(grid)
      expect(card.className, name).toContain("rounded-xl border bg-card p-4")
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
