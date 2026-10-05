// @vitest-environment jsdom

/**
 * The Work section against a stubbed daemon: it asks `GET /v1/stats/work`
 * with the screen's filter, under the key `qk` names, draws its tiles and
 * its chart from what came back, and says its empty sentence under its
 * heading and its question when there is nothing to show.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { WorkStatsDto } from "@/api"
import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { WorkSection } from "./work-section"

/**
 * Every explanation behind a labelled figure, read off its tooltip on hover.
 * A label such as "Finished" also names a plain `sr-only` table header with
 * no tooltip of its own, so an explained trigger is picked out by the one
 * thing that marks it: its `aria-describedby`. "Goals completed" and
 * "Landed" each name two: the tile's own span total, and the chart legend's
 * per-bucket one, so this returns every explanation rather than one.
 */
async function explanationsOf(container: HTMLElement, label: string): Promise<string[]> {
  const user = userEvent.setup()
  const triggers = within(container)
    .getAllByText(label)
    .filter((el) => el.hasAttribute("aria-describedby"))
  if (!triggers.length) throw new Error(`no explained trigger for "${label}"`)
  const texts: string[] = []
  for (const trigger of triggers) {
    await user.hover(trigger)
    const id = trigger.getAttribute("aria-describedby")
    texts.push((id ? document.getElementById(id)?.textContent : null) ?? "")
  }
  return texts
}

/** The one explanation behind a labelled figure that names only one trigger. */
async function explanationOf(container: HTMLElement, label: string): Promise<string | null> {
  const [text] = await explanationsOf(container, label)
  return text ?? null
}

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

  // A tile's label is now a tooltip trigger wrapping the text, so the `dt`
  // carries no direct text node of its own: find it by its full text instead
  // of a text-only selector match.
  const tile = (label: string) =>
    within(section)
      .getAllByRole("term")
      .find((term) => term.textContent === label)?.nextSibling?.textContent
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

it("explains every tile and every chart series, on hover", async () => {
  serve(STATS)
  renderScreen(<WorkSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Work" })
  await within(section).findByText("7")

  expect(await explanationOf(section, "Tasks finished")).toBe(
    "Tasks finished: the count of tasks that reached finished in this span.",
  )
  // "Goals completed" names two triggers: the tile's own span total, and the
  // chart legend's per-bucket figure, carried in the tooltip and the
  // sr-only table beside the stacked counts (spec rule 40).
  expect(await explanationsOf(section, "Goals completed")).toEqual([
    "Goals completed: the count of goals that reached completed in this span.",
    "Goals completed: the count of goals that reached completed in this bucket.",
  ])
  expect(await explanationOf(section, "Changes landed")).toBe(
    "Changes landed: the count of finished tasks whose landing was a merge or a pull request.",
  )
  expect(await explanationOf(section, "Finish rate")).toBe(
    "Finish rate: the share of finished, failed and cancelled tasks that finished.",
  )
  expect(await explanationOf(section, "Median goal lead time")).toBe(
    "Median goal lead time: the median time from a goal's creation to its completion.",
  )

  expect(await explanationOf(section, "Finished")).toBe(
    "Finished: tasks that reached finished in this bucket.",
  )
  expect(await explanationOf(section, "Failed")).toBe(
    "Failed: tasks that reached failed in this bucket.",
  )
  expect(await explanationOf(section, "Cancelled")).toBe(
    "Cancelled: tasks that reached cancelled in this bucket.",
  )
  expect(await explanationOf(section, "Landed")).toBe(
    "Landed: the count of finished tasks in this bucket whose landing was a merge or a pull request.",
  )
})
