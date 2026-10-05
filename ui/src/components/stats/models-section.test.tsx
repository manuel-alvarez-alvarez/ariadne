// @vitest-environment jsdom

/**
 * The Models section against a stubbed daemon: it asks `GET /v1/stats/models`
 * with the screen's filter, under the key `qk` names, and says its empty
 * sentence under its heading and its question.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ModelsSection } from "./models-section"

/** The explanation behind a column header, read off its tooltip on hover. */
async function explanationOf(container: HTMLElement, label: string): Promise<string | null> {
  const user = userEvent.setup()
  const trigger = within(container).getByText(label)
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

const base = {
  tasks: 4,
  goals: 2,
  tokens: 2400,
  time_secs: 270,
  messages: 6,
  rounds_per_task: null,
  changes_per_task: null,
}

/** Each column header of a table, and the cells of each of its rows. */
function cells(table: HTMLElement) {
  return within(table)
    .getAllByRole("row")
    .map((row) =>
      within(row)
        .queryAllByRole(row.querySelector("th") ? "columnheader" : "cell")
        .map((cell) => cell.textContent),
    )
}

it("draws three seat tables with their columns, formats and count ordering", async () => {
  const response = {
    items: [
      { ...base, model: "writer-small", seat: "author", rounds_per_task: 1.25 },
      { ...base, model: "writer-large", seat: "author", tasks: 9, rounds_per_task: 2 },
      { ...base, model: "judge", seat: "reviewer", tokens: 1_500_000, changes_per_task: 0.4 },
      { ...base, model: "planner", seat: "orchestrator", time_secs: 7200, messages: 12 },
    ],
  }
  daemonFetch.mockResolvedValue(jsonResponse(response))
  const filter = { since: "7d" }
  const { queryClient } = renderScreen(<ModelsSection filter={filter} />, { route: "/stats" })
  const authors = await screen.findByRole("table", { name: "Authors" })
  expect(
    screen.getAllByRole("table").map((table) => table.querySelector("caption")?.textContent),
  ).toEqual(["Authors", "Reviewers", "Orchestrators"])
  expect(cells(authors)).toEqual([
    ["MODEL", "TASKS", "TOKENS", "TIME", "MESSAGES", "ROUNDS/TASK"],
    ["writer-large", "9", "2.4k", "4m", "6", "2.0"],
    ["writer-small", "4", "2.4k", "4m", "6", "1.3"],
  ])
  expect(cells(screen.getByRole("table", { name: "Reviewers" }))).toEqual([
    ["MODEL", "TASKS", "TOKENS", "TIME", "MESSAGES", "CHANGES/TASK"],
    ["judge", "4", "1.5M", "4m", "6", "0.4"],
  ])
  expect(cells(screen.getByRole("table", { name: "Orchestrators" }))).toEqual([
    ["MODEL", "GOALS", "TOKENS", "TIME", "MESSAGES"],
    ["planner", "2", "2.4k", "2h", "12"],
  ])
  expect(queryClient.getQueryData(qk.stats.models(filter))).toEqual(response)
})

it("explains every column of every seat table, on hover", async () => {
  const response = {
    items: [
      { ...base, model: "writer-small", seat: "author", rounds_per_task: 1.25 },
      { ...base, model: "judge", seat: "reviewer", changes_per_task: 0.4 },
      { ...base, model: "planner", seat: "orchestrator" },
    ],
  }
  daemonFetch.mockResolvedValue(jsonResponse(response))
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })

  const authors = await screen.findByRole("table", { name: "Authors" })
  expect(await explanationOf(authors, "MODEL")).toBe(
    "MODEL: the name of the model this row is about.",
  )
  expect(await explanationOf(authors, "TASKS")).toBe(
    "TASKS: the count of distinct tasks this model worked as this seat.",
  )
  expect(await explanationOf(authors, "TOKENS")).toBe(
    "TOKENS: input and output tokens this model used in this seat, cache not counted twice.",
  )
  expect(await explanationOf(authors, "TIME")).toBe(
    "TIME: the summed session time this model ran in this seat.",
  )
  expect(await explanationOf(authors, "MESSAGES")).toBe(
    "MESSAGES: the count of messages this model sent in this seat.",
  )
  expect(await explanationOf(authors, "ROUNDS/TASK")).toBe(
    "ROUNDS/TASK: the mean number of review requests on each finished task.",
  )

  const reviewers = screen.getByRole("table", { name: "Reviewers" })
  expect(await explanationOf(reviewers, "CHANGES/TASK")).toBe(
    "CHANGES/TASK: the mean number of changes-requested verdicts on each task this model reviewed.",
  )

  const orchestrators = screen.getByRole("table", { name: "Orchestrators" })
  expect(await explanationOf(orchestrators, "GOALS")).toBe(
    "GOALS: the count of distinct goals this model orchestrated.",
  )
})

it("omits a table when its seat has no rows", async () => {
  daemonFetch.mockResolvedValue(
    jsonResponse({ items: [{ ...base, model: "writer", seat: "author", rounds_per_task: 0 }] }),
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
