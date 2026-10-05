// @vitest-environment jsdom

/**
 * The Spend section against a stubbed daemon: it asks `GET /v1/stats/spend`
 * with the screen's filter, under the key `qk` names, says its empty
 * sentence under its heading and its question when nothing was spent, and
 * otherwise draws the tiles and the time chart off the answer.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"
import type { SpendStatsDto } from "@/api"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SpendSection } from "./spend-section"

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
})

function mockSpend(body: SpendStatsDto) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse(body)
  })
}

it("asks for its family with the filter, and says its empty sentence", async () => {
  const filter = { since: "7d", repo: "01JREPO" }
  mockSpend({
    totals: {
      sessions: 0,
      input_tokens: 0,
      cached_input_tokens: 0,
      output_tokens: 0,
      cached_share: 0,
    },
    per_finished_task: { tasks: 0, input_tokens: 0, output_tokens: 0 },
    bucket: "week",
    buckets: [],
    by_model: [],
  })
  const { queryClient } = renderScreen(<SpendSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Spend" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Spend")
  expect(within(section).getByText("What did it spend?")).toBeDefined()
  expect(await within(section).findByText("Nothing was spent in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/spend"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.spend(filter))).toEqual(
    expect.objectContaining({ totals: expect.objectContaining({ sessions: 0 }) }),
  )
})

it("draws the tiles and the time chart from a mocked response, with no by-model chart", async () => {
  const filter = {}
  mockSpend({
    totals: {
      sessions: 3,
      input_tokens: 1_000_000,
      cached_input_tokens: 800_000,
      output_tokens: 100_000,
      cached_share: 0.8,
    },
    per_finished_task: { tasks: 2, input_tokens: 400_000, output_tokens: 50_000 },
    bucket: "week",
    buckets: [
      {
        start: "2026-09-28T00:00:00Z",
        input_tokens: 300_000,
        cached_input_tokens: 240_000,
        output_tokens: 40_000,
      },
    ],
    by_model: [
      {
        model: "stub:heavy",
        input_tokens: 250_000,
        cached_input_tokens: 150_000,
        output_tokens: 25_000,
        share: 0.6,
      },
    ],
  })
  renderScreen(<SpendSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Spend" })
  expect(await within(section).findByText("1M")).toBeDefined()
  expect(within(section).getByText("80.0%")).toBeDefined()
  expect(within(section).getByText("100k")).toBeDefined()
  expect(within(section).getByText("450k")).toBeDefined()
  expect(within(section).getByText("of 2 tasks")).toBeDefined()

  const tables = within(section).getAllByRole("table", { hidden: true })
  expect(tables).toHaveLength(1)
  const joined = tables.map((table) => table.textContent).join("\n")
  expect(joined).toContain("Sep 28")
  expect(joined).toContain("300k")
  expect(joined).toContain("40k")
  expect(joined).not.toContain("300000")
  expect(joined).not.toContain("stub:heavy")
})
