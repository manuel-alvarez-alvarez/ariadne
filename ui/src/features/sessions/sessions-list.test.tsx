// @vitest-environment jsdom

import { screen } from "@testing-library/react"
import { beforeEach, expect, it, vi } from "vitest"

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
