// @vitest-environment jsdom

/**
 * The Work section against a stubbed daemon: it asks `GET /v1/stats/work`
 * with the screen's filter, under the key `qk` names, draws its tiles and
 * its chart from what came back, and says its empty sentence under its
 * heading and its question when there is nothing to show.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import type { WorkStatsDto } from "@/api"
import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkSection } from "./work-section"

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
})

function serve(stats: WorkStatsDto) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse(stats)
  })
}

const STATS: WorkStatsDto = {
  totals: {
    goals_completed: 2,
    goals_cancelled: 1,
    median_goal_lead_time_secs: 3_600,
    tasks_finished: 7,
    tasks_failed: 2,
    tasks_cancelled: 1,
    finish_rate: 0.7,
    landed: 5,
  },
  bucket: "day",
  buckets: [
    {
      start: "2026-09-28T00:00:00Z",
      tasks_finished: 3,
      tasks_failed: 1,
      tasks_cancelled: 0,
      goals_completed: 1,
      landed: 2,
    },
    {
      start: "2026-09-29T00:00:00Z",
      tasks_finished: 4,
      tasks_failed: 1,
      tasks_cancelled: 1,
      goals_completed: 1,
      landed: 3,
    },
  ],
}

it("asks for its family with the filter, and says its empty sentence", async () => {
  serve({ totals: STATS.totals, bucket: "week", buckets: [] })
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<WorkSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Work" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Work")
  expect(within(section).getByText("What got done?")).toBeDefined()
  expect(await within(section).findByText("Nothing got done in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/work"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.work(filter))).toEqual({
    totals: STATS.totals,
    bucket: "week",
    buckets: [],
  })
})

it("draws the tiles and the chart from what the daemon answers", async () => {
  serve(STATS)
  const filter = { since: "7d" }
  renderScreen(<WorkSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Work" })
  await within(section).findByText("7") // tasks finished tile renders once loaded

  const tile = (label: string) =>
    within(section).getByText(label, { selector: "dt" }).nextSibling?.textContent
  expect(tile("Tasks finished")).toBe("7")
  expect(tile("Goals completed")).toBe("2")
  expect(tile("Changes landed")).toBe("5")
  expect(tile("Finish rate")).toBe("70.0%")
  expect(tile("Median goal lead time")).toBe("1h")

  // The sr-only table beside the chart carries the same numbers: one row per
  // bucket, oldest first, with the stacked counts and the extra tooltip
  // figures beside them.
  const [row0, row1, ...rest] = within(section).getAllByRole("row").slice(1)
  expect(rest).toHaveLength(0)
  expect(
    within(row0 as HTMLElement)
      .getAllByRole("cell")
      .map((cell) => cell.textContent),
  ).toEqual(["Sep 28", "3", "1", "0", "1", "2"])
  expect(
    within(row1 as HTMLElement)
      .getAllByRole("cell")
      .map((cell) => cell.textContent),
  ).toEqual(["Sep 29", "4", "1", "1", "1", "3"])
})
