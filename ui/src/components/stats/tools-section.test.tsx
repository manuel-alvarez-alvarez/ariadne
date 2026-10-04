// @vitest-environment jsdom

/**
 * The Tools section against a stubbed daemon: it asks `GET /v1/stats/tools`
 * with the screen's filter, under the key `qk` names, says its empty
 * sentence under its heading and its question, and draws the tiles and both
 * charts from an answer with more tools than the limit it draws — never
 * more than the limit plus one rows.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import type { ToolStatsDto } from "@/api"
import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ToolsSection } from "./tools-section"

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
})

function serve(body: unknown) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse(body)
  })
}

const EMPTY: ToolStatsDto = {
  calls: 0,
  errors: 0,
  tools: 0,
  by_kind: [],
  top: [],
  other: { tools: 0, calls: 0, errors: 0 },
}

it("asks for its family with the filter, and says its empty sentence", async () => {
  serve(EMPTY)
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<ToolsSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Tools" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Tools")
  expect(within(section).getByText("What do the agents do?")).toBeDefined()
  expect(await within(section).findByText("No tool ran in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/tools"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.tools(filter))).toEqual(EMPTY)
})

/** A tool named `ToolN`, calling `kind` `calls` times, one of them failed. */
function tool(name: string, kind: string, calls: number) {
  return {
    tool_name: name,
    kind,
    calls,
    errors: 1,
    median_duration_ms: 20,
    p90_duration_ms: 40,
  }
}

it("draws the tiles and both charts, never more than the limit plus one rows", async () => {
  const top = Array.from({ length: 12 }, (_, i) => tool(`Tool${i}`, "execute", 30 - i))
  const stats: ToolStatsDto = {
    calls: 400,
    errors: 15,
    tools: 18,
    by_kind: [
      { kind: "execute", calls: 300, errors: 10, median_duration_ms: 20, p90_duration_ms: 50 },
      { kind: "read", calls: 100, errors: 5, median_duration_ms: 5, p90_duration_ms: 10 },
    ],
    top,
    other: { tools: 3, calls: 40, errors: 1 },
  }
  serve(stats)
  const filter = { since: "7d", repo: "01JREPO" }
  renderScreen(<ToolsSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Tools" })
  expect(await within(section).findByText("400")).toBeDefined()
  expect(within(section).getByText("15")).toBeDefined()
  expect(within(section).getByText("4%")).toBeDefined()

  const byKind = within(section).getByRole("table", { name: "Calls per kind, ok and errors" })
  expect(within(byKind).getAllByRole("row")).toHaveLength(1 + 2)

  const byTool = within(section).getByRole("table", { name: "Calls per tool, ok and errors" })
  // 12 tools came back, past the 10 the section draws; the `other` row is
  // the 11th, never a 12th or 13th.
  expect(within(byTool).getAllByRole("row")).toHaveLength(1 + 10 + 1)
})
