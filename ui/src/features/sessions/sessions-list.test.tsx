// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

import { shortId } from "@/lib/format"
import { aSession, aSessionPage } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { SessionsList } from "./sessions-list"

const MODEL = "claude-agent-acp:claude-opus-4-5-20251101"
const SESSION = aSession({ model: MODEL, effort: "high" })

beforeEach(() => {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET" && new URL(request.url).pathname === "/v1/sessions") {
      return Promise.resolve(jsonResponse(aSessionPage([SESSION])))
    }
    return Promise.resolve(jsonResponse([]))
  })
})

it("keeps the effort beside a middle-cut model and gives the row a whole-pin hint", async () => {
  renderScreen(<SessionsList filters={{}} onSelect={vi.fn()} />, { route: "/goals" })

  const pin = await screen.findByTitle(MODEL)
  const row = pin.closest("tr")
  if (!row) throw new Error("no session row around model pin")

  expect(row.textContent).toContain(`${MODEL} @ high`)
  expect(pin.closest("[data-slot='tooltip-trigger']")).not.toBeNull()
})

it("names a row by its workflow step where the caller gives one, and names none where it gives none", async () => {
  const staffed = aSession({ id: "01JSESS0000000000000000002", task_agent_id: "agent-build" })
  const loose = aSession({ id: "01JSESS0000000000000000003", task_agent_id: null })
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET" && new URL(request.url).pathname === "/v1/sessions") {
      return Promise.resolve(jsonResponse(aSessionPage([staffed, loose])))
    }
    return Promise.resolve(jsonResponse([]))
  })

  renderScreen(
    <SessionsList
      filters={{}}
      sessionStep={(session) => (session.id === staffed.id ? "Build" : undefined)}
      onSelect={vi.fn()}
    />,
    { route: "/goals" },
  )

  const step = await screen.findByText("Build")
  const staffedRow = step.closest("tr")
  if (!staffedRow) throw new Error("no row around the step title")

  const looseRow = screen.getByText(shortId(loose.id)).closest("tr")
  if (!looseRow) throw new Error("no row around the loose session's id")
  expect(looseRow.textContent).not.toContain("Build")
})
