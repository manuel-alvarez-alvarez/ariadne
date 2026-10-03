// @vitest-environment jsdom

/**
 * The Stats screen against a stubbed daemon.
 *
 * What is pinned: the screen asks `GET /v1/stats/models`,
 * `GET /v1/stats/reviews`, `GET /v1/stats/switches`, `GET /v1/stats/tools`
 * and `GET /v1/stats/outcomes` with the filters in its URL, draws each
 * family as a chart off its own answer, and keeps each under the key `qk`
 * names, which is the one the dispatcher invalidates.
 *
 * A chart itself is Recharts' own SVG, which jsdom lays out with no real
 * geometry — nothing here reads a bar's pixels. What is read is the
 * `sr-only` table beside it, built from the same rows the bars are, which is
 * also what a screen reader gets.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, describe, expect, it } from "vitest"

import { type ModelStatDto, qk, type SwitchStatDto, type ToolStatDto } from "@/api"
import { aRepository } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
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
const TOOL: ToolStatDto = {
  tool_name: "Bash",
  calls: 4,
  errors: 1,
  median_duration_ms: 25,
  p90_duration_ms: 100,
}

const SWITCH_ROW: SwitchStatDto = {
  model: "stub:a",
  switches: 3,
  by_reason: [{ reason: "exhausted", switches: 2 }],
  exhaustions: 2,
  automatic_share: 2 / 3,
  arrivals: 1,
}

const OUTCOMES = {
  items: [
    {
      model: "stub:test-model",
      finished: 7,
      failed: 2,
      cancelled: 1,
      finish_rate: 0.7,
      median_lead_time_secs: 310,
      mean_lead_time_secs: 400,
      mean_review_requests: 2.5,
      contests_entered: 3,
      contests_won: 2,
      win_rate: 2 / 3,
    },
  ],
  totals: {
    finished: 7,
    failed: 2,
    cancelled: 1,
    finish_rate: 0.7,
    median_lead_time_secs: 310,
    mean_lead_time_secs: 400,
    mean_review_requests: 2.5,
    contests_entered: 3,
    contests_won: 2,
    win_rate: 2 / 3,
  },
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

/** `tr`'s own cells, in order — a `getByText` lookup is ambiguous once two cells share a count. */
function cellsOf(row: Element | null): string[] {
  if (!row) throw new Error("no row")
  return [...row.querySelectorAll("td")].map((cell) => cell.textContent ?? "")
}

/** The nth `sr-only` table of a panel, two or more charts deep in it otherwise. */
function tableAt(tables: HTMLElement[], index: number): HTMLElement {
  const table = tables[index]
  if (!table) throw new Error(`no table at ${index}`)
  return table
}

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
    asked.push(url)
    if (url.pathname === "/v1/stats/reviews") return jsonResponse(REVIEWS)
    if (url.pathname === "/v1/stats/tools") {
      return jsonResponse({
        tools: [TOOL],
        models: [{ model: "stub:test-model", calls: 4, mean_duration_ms: 40 }],
        permissions: [{ decided_by: "console", answer: "allow", permissions: 1, mean_wait_ms: 20 }],
      })
    }
    if (url.pathname === "/v1/stats/switches") {
      return jsonResponse({ items: [SWITCH_ROW], switches: 3, exhaustions: 2 })
    }
    if (url.pathname === "/v1/stats/outcomes") return jsonResponse(OUTCOMES)
    return jsonResponse({ items: [ROW] })
  })
})

describe("StatsPage", () => {
  it("draws the models panel's charts from the daemon's rows", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Models" })
    const label = "stub:test-model (author)"
    const tables = await within(panel).findAllByRole("table", { hidden: true })
    expect(tables).toHaveLength(2)
    // Sessions, by how they ended: ended = 4 sessions - 1 failed, `stalled`
    // left alone since it can overlap `failed` rather than subtracting twice.
    expect(cellsOf(within(tableAt(tables, 0)).getByText(label).closest("tr"))).toEqual([
      label,
      "3",
      "1",
      "2",
    ])
    // Tokens: 1,200,000 input + 45,000 output.
    expect(cellsOf(within(tableAt(tables, 1)).getByText(label).closest("tr"))).toEqual([
      label,
      "1245000",
    ])
    expect(asked.map((url) => url.pathname).sort()).toEqual([
      "/v1/stats/models",
      "/v1/stats/outcomes",
      "/v1/stats/reviews",
      "/v1/stats/switches",
      "/v1/stats/tools",
    ])
  })

  it("draws the reviews panel's chart from the daemon's rows", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Reviews" })
    const row = (await within(panel).findByText("stub:test-model")).closest("tr")
    expect(cellsOf(row)).toEqual(["stub:test-model", "0.0%", "1", "2", "2"])
  })

  it("draws the switches panel's chart from the daemon's rows", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Switches" })
    const row = (await within(panel).findByText("stub:a")).closest("tr")
    // Exhausted 2 of 3 switches; automatic_share (2/3) accounts for no more
    // than the exhausted ones, so automatic-other is 0 and other is 1.
    expect(cellsOf(row)).toEqual(["stub:a", "2", "0", "1", "1"])
  })

  it("draws the tools panel's charts from the daemon's rows", async () => {
    const { queryClient } = renderScreen(<StatsPage />, { route: "/stats?since=7d" })

    const panel = await screen.findByRole("region", { name: "Tools" })
    const tables = await within(panel).findAllByRole("table", { hidden: true })
    expect(tables).toHaveLength(3)
    // Calls per tool: 4 calls, 1 of them an error.
    expect(cellsOf(within(tableAt(tables, 0)).getByText("Bash").closest("tr"))).toEqual([
      "Bash",
      "4",
      "1",
      "25ms",
      "100ms",
    ])
    // Permission answers per decider.
    expect(cellsOf(within(tableAt(tables, 1)).getByText("console").closest("tr"))).toEqual([
      "console",
      "1",
      "0",
      "0",
    ])
    // Calls per model.
    expect(cellsOf(within(tableAt(tables, 2)).getByText("stub:test-model").closest("tr"))).toEqual([
      "stub:test-model",
      "4",
      "40ms",
    ])
    expect(queryClient.getQueryData(qk.stats.tools({ since: "7d" }))).toEqual({
      tools: [TOOL],
      models: [{ model: "stub:test-model", calls: 4, mean_duration_ms: 40 }],
      permissions: [{ decided_by: "console", answer: "allow", permissions: 1, mean_wait_ms: 20 }],
    })
  })

  it("draws the permission chart even where no tool call completed", async () => {
    // A denied or cancelled permission can end with no `tool_call` fact
    // behind it, so `tools` is empty while `permissions` still has rows.
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
      if (url.pathname === "/v1/stats/reviews") return jsonResponse(REVIEWS)
      if (url.pathname === "/v1/stats/tools") {
        return jsonResponse({
          tools: [],
          models: [],
          permissions: [
            { decided_by: "console", answer: "deny", permissions: 1, mean_wait_ms: 20 },
          ],
        })
      }
      if (url.pathname === "/v1/stats/switches") return jsonResponse({ items: [] })
      if (url.pathname === "/v1/stats/outcomes") return jsonResponse(OUTCOMES)
      return jsonResponse({ items: [ROW] })
    })
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Tools" })
    const tables = await within(panel).findAllByRole("table", { hidden: true })
    expect(tables).toHaveLength(1)
    expect(cellsOf(within(tableAt(tables, 0)).getByText("console").closest("tr"))).toEqual([
      "console",
      "0",
      "1",
      "0",
    ])
    expect(within(panel).queryByText("No tool calls in this span")).toBeNull()
  })

  it("draws the outcomes panel's charts from the daemon's rows", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Outcomes" })
    const tables = await within(panel).findAllByRole("table", { hidden: true })
    expect(tables).toHaveLength(2)
    expect(cellsOf(within(tableAt(tables, 0)).getByText("stub:test-model").closest("tr"))).toEqual([
      "stub:test-model",
      "7",
      "2",
      "1",
      "70.0%",
      "66.7%",
      "2/3",
      "6m",
      "2.5",
    ])
    expect(cellsOf(within(tableAt(tables, 1)).getByText("stub:test-model").closest("tr"))).toEqual([
      "stub:test-model",
      "5m",
    ])
  })

  it("gives the five panels the same heading style", async () => {
    renderScreen(<StatsPage />, { route: "/stats" })

    const headings = await screen.findAllByRole("heading", { level: 2 })
    expect(headings.map((heading) => heading.textContent)).toEqual([
      "Models",
      "Reviews",
      "Switches",
      "Tools",
      "Outcomes",
    ])
    for (const heading of headings) {
      expect(heading.className).toBe("text-sm font-medium")
    }
  })

  it("renders an error, not an empty chart, when the daemon refuses the read", async () => {
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
      if (url.pathname === "/v1/stats/models") {
        return errorResponse(500, "internal", "the ledger is not answering")
      }
      if (url.pathname === "/v1/stats/reviews") return jsonResponse(REVIEWS)
      if (url.pathname === "/v1/stats/tools") {
        return jsonResponse({ tools: [], models: [], permissions: [] })
      }
      if (url.pathname === "/v1/stats/switches") return jsonResponse({ items: [] })
      if (url.pathname === "/v1/stats/outcomes") return jsonResponse(OUTCOMES)
      return jsonResponse({ items: [] })
    })
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Models" })
    expect(await within(panel).findByText("Could not load the model stats")).toBeDefined()
    expect(within(panel).queryAllByRole("table", { hidden: true })).toHaveLength(0)
    expect(within(panel).queryByText("No session has ended in this span")).toBeNull()
  })

  it("renders an empty family as its heading and one sentence, with no chart", async () => {
    daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const url = new URL(request.url)
      if (url.pathname === "/v1/repositories") return jsonResponse([aRepository()])
      if (url.pathname === "/v1/stats/reviews") return jsonResponse(REVIEWS)
      if (url.pathname === "/v1/stats/tools") {
        return jsonResponse({ tools: [], models: [], permissions: [] })
      }
      if (url.pathname === "/v1/stats/switches") return jsonResponse({ items: [] })
      if (url.pathname === "/v1/stats/outcomes") return jsonResponse(OUTCOMES)
      return jsonResponse({ items: [] })
    })
    renderScreen(<StatsPage />, { route: "/stats" })

    const panel = await screen.findByRole("region", { name: "Models" })
    expect(await within(panel).findByText("No session has ended in this span")).toBeDefined()
    expect(within(panel).queryAllByRole("table", { hidden: true })).toHaveLength(0)
  })

  it("asks with the filters in its URL, under the key qk names", async () => {
    const { queryClient } = renderScreen(<StatsPage />, {
      route: "/stats?since=7d&repo=01JREPO",
    })

    // Scoped to the `sr-only` table: the chart draws the same label on its
    // y-axis, and a bare `findByText` would see both.
    const modelsPanel = await screen.findByRole("region", { name: "Models" })
    const modelsTables = await within(modelsPanel).findAllByRole("table", { hidden: true })
    await within(tableAt(modelsTables, 0)).findByText("stub:test-model (author)")
    const switchesPanel = await screen.findByRole("region", { name: "Switches" })
    const switchesTables = await within(switchesPanel).findAllByRole("table", { hidden: true })
    await within(tableAt(switchesTables, 0)).findByText("stub:a")
    const models = asked.find((url) => url.pathname === "/v1/stats/models")
    const switches = asked.find((url) => url.pathname === "/v1/stats/switches")
    expect(models?.searchParams.get("since")).toBe("7d")
    expect(models?.searchParams.get("repo")).toBe("01JREPO")
    expect(switches?.searchParams.get("since")).toBe("7d")
    expect(switches?.searchParams.get("repo")).toBe("01JREPO")
    expect(queryClient.getQueryData(qk.stats.models({ since: "7d", repo: "01JREPO" }))).toEqual({
      items: [ROW],
    })
    expect(queryClient.getQueryData(qk.stats.switches({ since: "7d", repo: "01JREPO" }))).toEqual({
      items: [SWITCH_ROW],
      switches: 3,
      exhaustions: 2,
    })
    expect(queryClient.getQueryData(qk.stats.outcomes({ since: "7d", repo: "01JREPO" }))).toEqual(
      OUTCOMES,
    )
  })
})
