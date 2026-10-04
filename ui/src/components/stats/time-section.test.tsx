// @vitest-environment jsdom

/**
 * The Time section against a stubbed daemon: it asks `GET /v1/stats/time`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TimeSection } from "./time-section"

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
