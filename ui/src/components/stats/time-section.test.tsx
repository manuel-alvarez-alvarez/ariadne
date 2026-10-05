// @vitest-environment jsdom

/**
 * The Time section against a stubbed daemon: it asks `GET /v1/stats/time`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TimeSection } from "./time-section"

/**
 * The explanation behind a labelled figure, read off its tooltip on hover. A
 * label such as "Total" also names a plain `sr-only` table header with no
 * tooltip of its own, so the explained trigger is picked out by the one
 * thing that marks it: its `aria-describedby`.
 */
async function explanationOf(container: HTMLElement, label: string): Promise<string | null> {
  const user = userEvent.setup()
  const trigger = within(container)
    .getAllByText(label)
    .find((el) => el.hasAttribute("aria-describedby"))
  if (!trigger) throw new Error(`no explained trigger for "${label}"`)
  await user.hover(trigger)
  const id = trigger.getAttribute("aria-describedby")
  return id ? (document.getElementById(id)?.textContent ?? null) : null
}

/** The URLs of every request the section made. */
let asked: URL[] = []

beforeEach(() => {
  asked = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse({
      tasks: 2,
      lead_time: { median_secs: 120, p90_secs: 300, mean_secs: 180 },
      in_status: [{ status: "in_progress", total_secs: 240, median_secs: 120, share: 1 }],
      waiting_on_person: { prompts: 3, total_secs: 60, median_secs: 20 },
    })
  })
})

it("asks for its family with the filter and draws its time figures", async () => {
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<TimeSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Time" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Time")
  expect(within(section).getByText("How long does it take?")).toBeDefined()
  expect(await within(section).findByText("Median lead time")).toBeDefined()
  expect(within(section).getByText("P90 lead time")).toBeDefined()
  expect(within(section).getByText("Time waiting on a person")).toBeDefined()
  expect(within(section).getByText("Prompts waited on")).toBeDefined()
  expect(within(section).getByText("Where the time goes")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/time"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.time(filter))).toMatchObject({ tasks: 2 })
})

it("explains every tile and every chart series, on hover", async () => {
  renderScreen(<TimeSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Time" })
  await within(section).findByText("Median lead time")

  expect(await explanationOf(section, "Median lead time")).toBe(
    "Median lead time: the median time from a finished task's creation to its finish.",
  )
  expect(await explanationOf(section, "P90 lead time")).toBe(
    "P90 lead time: the nearest-rank 90th percentile of that same lead time.",
  )
  expect(await explanationOf(section, "Time waiting on a person")).toBe(
    "Time waiting on a person: the total time spent on permission prompts you answered.",
  )
  expect(await explanationOf(section, "Prompts waited on")).toBe(
    "Prompts waited on: the count of permission prompts you answered.",
  )
  // The chart's bar is "Total"; "Median" and "Share" have no bar of their
  // own, carried only in the tooltip and the sr-only table until now.
  expect(await explanationOf(section, "Total")).toBe(
    "Total: the summed time tasks spent in this status.",
  )
  expect(await explanationOf(section, "Median")).toBe(
    "Median: the median time a task spent in this status.",
  )
  expect(await explanationOf(section, "Share")).toBe(
    "Share: this status's share of the total time measured here.",
  )
})
