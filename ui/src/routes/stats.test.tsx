// @vitest-environment jsdom

/**
 * The Stats screen against a stubbed daemon.
 *
 * What is pinned: the screen asks `GET /v1/stats/models` with the filters in
 * its URL, renders the models panel off the answer — the row's model, seat,
 * counts, token figure and skills — and keeps the answer under the key `qk`
 * names, which is the one the dispatcher invalidates.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { type ModelStatDto, qk } from "@/api"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { StatsPage } from "./stats"

const ROW: ModelStatDto = {
  model: "stub:test-model",
  seat: "author",
  sessions: 4,
  failed: 1,
  stalled: 2,
  usage: { input_tokens: 1_200_000, cached_input_tokens: 1_069_200, output_tokens: 45_000 },
  cached_share: 0.891,
  mean_lifetime_secs: 260,
  skills: [
    { name: "coding", sessions: 4 },
    { name: "migration", sessions: 1 },
  ],
}

/** The URLs of every stats request the screen made. */
let asked: URL[] = []
const REVIEWS = {
  authors: [
    {
      model: "stub:test-model",
      approvals: 1,
      mean_rounds: 2,
      median_rounds: 2,
      first_pass_rate: 0,
    },
  ],
  reviewers: [],
  messages: [],
}

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
    asked.push(url)
    if (url.pathname === "/v1/stats/reviews") return jsonResponse(REVIEWS)
    return jsonResponse({ items: [ROW] })
  })
})

describe("StatsPage", () => {
  it("renders the models panel from the daemon's rows", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Models" })
    const row = (await within(panel).findByText("stub:test-model")).closest("tr")
    if (!row) throw new Error("no row for the model")
    const cells = within(row)
    expect(cells.getByText("author")).toBeDefined()
    expect(cells.getByText("4")).toBeDefined()
    expect(cells.getByText("1.2M", { exact: false })).toBeDefined()
    expect(cells.getByText("45k", { exact: false })).toBeDefined()
    expect(cells.getByText("4m")).toBeDefined()
    expect(cells.getByText("coding 4, migration 1")).toBeDefined()
    const reviews = await screen.findByRole("region", { name: "Reviews" })
    expect(within(reviews).getAllByText("2").length).toBeGreaterThan(0)
    expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/models", "/v1/stats/reviews"])
  })

  it("asks with the filters in its URL, under the key qk names", async () => {
    const { queryClient } = renderScreen(<StatsPage />, {
      route: "/stats?since=7d&repo=01JREPO",
    })

    await screen.findByText("stub:test-model")
    expect(asked[0]?.searchParams.get("since")).toBe("7d")
    expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
    expect(queryClient.getQueryData(qk.stats.models({ since: "7d", repo: "01JREPO" }))).toEqual({
      items: [ROW],
    })
  })
})
