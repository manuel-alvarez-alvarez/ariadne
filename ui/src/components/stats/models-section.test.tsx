// @vitest-environment jsdom

/**
 * The Models section against a stubbed daemon: it asks `GET /v1/stats/models`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ModelsSection } from "./models-section"

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse({ items: [] })
  })
})

it("asks for its family with the filter, and says its empty sentence", async () => {
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<ModelsSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Models")
  expect(within(section).getByText("Which model does the job?")).toBeDefined()
  expect(await within(section).findByText("No model ran in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/models"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.models(filter))).toEqual({ items: [] })
})

const usage = { input_tokens: 2000, cached_input_tokens: 1000, output_tokens: 400 }
const base = {
  sessions: 3,
  failed_sessions: 1,
  stalled_sessions: 1,
  exhaustions: 2,
  usage,
  cached_share: 0.5,
  mean_lifetime_secs: 90,
  author: null,
  reviewer: null,
}
const author = {
  tasks_finished: 2,
  tasks_failed: 1,
  tasks_cancelled: 1,
  finish_rate: 0.5,
  first_pass_rate: 0.75,
  mean_review_rounds: 1.5,
  contests_entered: 4,
  contests_won: 1,
  win_rate: 0.25,
  tokens_per_finished_task: 1200,
}

it("draws three seat tables with formatted figures and count ordering", async () => {
  const response = {
    items: [
      { ...base, model: "writer-small", seat: "author", author },
      { ...base, model: "writer-large", seat: "author", author: { ...author, tasks_finished: 9 } },
      {
        ...base,
        model: "judge",
        seat: "reviewer",
        reviewer: { verdicts: 8, approve_share: 0.625, mean_latency_secs: 120 },
      },
      { ...base, model: "planner", seat: "orchestrator" },
    ],
  }
  daemonFetch.mockResolvedValue(jsonResponse(response))
  const filter = { since: "7d" }
  const { queryClient } = renderScreen(<ModelsSection filter={filter} />, { route: "/stats" })
  const authors = await screen.findByRole("table", { name: "Authors" })
  expect(
    screen.getAllByRole("table").map((table) => table.querySelector("caption")?.textContent),
  ).toEqual(["Authors", "Reviewers", "Orchestrators"])
  expect(
    within(authors)
      .getAllByRole("row")
      .slice(1)
      .map((row) => within(row).getAllByRole("cell")[0]?.textContent),
  ).toEqual(["writer-large", "writer-small"])
  expect(within(authors).getAllByRole("row")[2]?.textContent).toBe(
    "writer-small450.0%75.0%1.525.0%1.2k12",
  )
  expect(
    within(screen.getByRole("table", { name: "Reviewers" })).getAllByRole("row")[1]?.textContent,
  ).toBe("judge862.5%2m12")
  expect(
    within(screen.getByRole("table", { name: "Orchestrators" })).getAllByRole("row")[1]
      ?.textContent,
  ).toBe("planner32.4k1m12")
  expect(queryClient.getQueryData(qk.stats.models(filter))).toEqual(response)
})

it("omits a table when its seat has no rows", async () => {
  daemonFetch.mockResolvedValue(
    jsonResponse({ items: [{ ...base, model: "writer", seat: "author", author }] }),
  )
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })
  await screen.findByRole("table", { name: "Authors" })
  expect(screen.queryByRole("table", { name: "Reviewers" })).toBeNull()
  expect(screen.queryByRole("table", { name: "Orchestrators" })).toBeNull()
})

it("shows the empty state when the response contains only seatless rows", async () => {
  daemonFetch.mockResolvedValue(jsonResponse({ items: [{ ...base, model: "loose", seat: null }] }))
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })
  const section = await screen.findByRole("region", { name: "Models" })
  expect(await within(section).findByText("No model ran in this span.")).toBeDefined()
  expect(within(section).queryByRole("table")).toBeNull()
})
