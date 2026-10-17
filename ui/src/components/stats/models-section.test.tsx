// @vitest-environment jsdom

/**
 * The Models section against a stubbed daemon: it asks `GET /v1/stats/models`
 * with the screen's filter, under the key `qk` names, draws a summary card
 * and a ranked bar chart per seat, and says its empty sentence when neither
 * seat has a row.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { ModelStatsDto } from "@/api"
import { qk } from "@/api"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { ModelsSection } from "./models-section"

/** The explanation behind a labelled figure, read off its tooltip on hover. */
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
})

function serve(stats: ModelStatsDto) {
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    asked.push(new URL(request.url))
    return jsonResponse(stats)
  })
}

it("asks for its family with the filter, and says its empty sentence", async () => {
  serve({ items: [] })
  const filter = { since: "7d", repo: "01JREPO" }
  const { queryClient } = renderScreen(<ModelsSection filter={filter} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  expect(within(section).getByRole("heading", { level: 2 }).textContent).toBe("Models")
  expect(await within(section).findByText("No model ran in this span.")).toBeDefined()
  expect(asked.map((url) => url.pathname)).toEqual(["/v1/stats/models"])
  expect(asked[0]?.searchParams.get("since")).toBe("7d")
  expect(asked[0]?.searchParams.get("repo")).toBe("01JREPO")
  expect(queryClient.getQueryData(qk.stats.models(filter))).toEqual({ items: [] })
})

it("says its empty sentence for rows with no recognized seat", async () => {
  serve({
    items: [
      {
        model: "stub:loose",
        seat: null,
        tasks: 1,
        goals: 0,
        tokens: 10,
        time_secs: 1,
        messages: 1,
        tasks_finished: 0,
      },
    ],
  })
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })
  expect(await screen.findByText("No model ran in this span.")).toBeDefined()
})

const STATS: ModelStatsDto = {
  items: [
    {
      model: "stub:light",
      seat: "agent",
      tasks: 3,
      goals: 0,
      tokens: 100_000,
      time_secs: 600,
      messages: 5,
      tasks_finished: 2,
    },
    {
      model: "stub:heavy",
      seat: "agent",
      tasks: 7,
      goals: 0,
      tokens: 400_000,
      time_secs: 1_800,
      messages: 9,
      tasks_finished: 6,
    },
    {
      model: "stub:orchestrator",
      seat: "orchestrator",
      tasks: 0,
      goals: 4,
      tokens: 50_000,
      time_secs: 300,
      messages: 2,
      tasks_finished: 0,
    },
  ],
}

it("draws a summary card and a ranked chart for each seat, heaviest model first", async () => {
  serve(STATS)
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  await within(section).findByText("Agents")
  expect(within(section).getByText("Orchestrators")).toBeDefined()

  const tile = (label: string) =>
    within(section)
      .getAllByRole("term")
      .find((term) => term.textContent === label)?.nextElementSibling?.textContent
  expect(tile("Agent models")).toBe("2")
  expect(tile("Agent tasks")).toBe("10")
  expect(tile("Agent tokens")).toBe("500k")
  expect(tile("Orchestrator models")).toBe("1")
  expect(tile("Orchestrator goals")).toBe("4")

  const tables = within(section).getAllByRole("table", { hidden: true })
  const agentTable = tables.find((table) => table.textContent?.includes("stub:heavy"))
  if (!agentTable) throw new Error("no agent table found")
  const rows = within(agentTable).getAllByRole("row").slice(1)
  expect(rows.map((row) => within(row).getAllByRole("cell")[0]?.textContent)).toEqual([
    "stub:heavy",
    "stub:light",
  ])
})

it("names only the drawn bar in each legend, with every other figure kept in the table", async () => {
  serve(STATS)
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  await within(section).findByText("Agents")

  const outsideTables = (text: string) =>
    within(section)
      .queryAllByText(text)
      .filter((element) => !element.closest("table"))
  expect(outsideTables("Tasks")).toHaveLength(1)
  expect(outsideTables("Goals")).toHaveLength(1)
  for (const figure of ["Tokens", "Time", "Messages", "Finished"]) {
    expect(outsideTables(figure)).toHaveLength(0)
    expect(
      within(section).getAllByRole("columnheader", { hidden: true, name: figure }).length,
    ).toBeGreaterThan(0)
  }
})

it("omits a seat with no row", async () => {
  serve({ items: [STATS.items[0] as ModelStatsDto["items"][number]] })
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  await within(section).findByText("Agents")
  expect(within(section).queryByText("Orchestrators")).toBeNull()
})

it("explains every tile and every chart series, on hover", async () => {
  serve(STATS)
  renderScreen(<ModelsSection filter={{}} />, { route: "/stats" })

  const section = await screen.findByRole("region", { name: "Models" })
  await within(section).findByText("Agent models")

  expect(await explanationOf(section, "Agent models")).toBe(
    "Agent models: the count of distinct models that ran as an agent in this span.",
  )
  expect(await explanationOf(section, "Tasks")).toBe(
    "Tasks: the count of distinct tasks this model worked as this seat.",
  )
  expect(await explanationOf(section, "Orchestrator goals")).toBe(
    "Orchestrator goals: the count of distinct goals an orchestrator model ran in this span.",
  )
  expect(await explanationOf(section, "Goals")).toBe(
    "Goals: the count of distinct goals this model orchestrated.",
  )
})
