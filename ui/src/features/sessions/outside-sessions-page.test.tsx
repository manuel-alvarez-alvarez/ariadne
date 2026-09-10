// @vitest-environment jsdom

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { components, SessionDto, TaskDto } from "@/api"
import { aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { OutsideSessionsPage } from "./outside-sessions-page"

type OutsideSessionDto = components["schemas"]["OutsideSessionDto"]

const OUTSIDE: OutsideSessionDto = {
  agent_kind: "codex",
  internal_session_id: "thread-123",
  working_directory: "/Users/me/dev/ariadne",
  last_activity_at: "2026-09-10T08:30:00Z",
  first_prompt: "Implement session adoption.",
}

const READY: TaskDto = aTask({
  id: "01JTASK000000000000000READY",
  status: "ready",
  title: "Show outside sessions",
  agents: [
    { id: "01JAGENT0000000000000AUTH", seat: "author", skills: ["coding"], model: "codex:gpt-5" },
  ],
})

function stubDaemon({ adopted }: { adopted?: SessionDto } = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/outside-sessions") return Promise.resolve(jsonResponse([OUTSIDE]))
    if (url.pathname === "/v1/tasks") return Promise.resolve(jsonResponse([READY]))
    if (url.pathname === `/v1/tasks/${READY.id}/author-session` && request.method === "POST") {
      return Promise.resolve(jsonResponse(adopted ?? aSession({ task_id: READY.id })))
    }
    return Promise.resolve(jsonResponse([]))
  })
}

beforeEach(() => stubDaemon())

it("lists each outside session with its CLI, directory, activity, and first prompt", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  expect(await screen.findByText("Codex")).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "CLI" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "Working directory" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "Last activity" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "First prompt" })).toBeTruthy()
  expect(screen.getByText(OUTSIDE.working_directory)).toBeTruthy()
  expect(screen.getByText(OUTSIDE.first_prompt)).toBeTruthy()
  expect(document.querySelector(`time[datetime="${OUTSIDE.last_activity_at}"]`)).not.toBeNull()
})

it("adopts an outside session as the author of a matching ready task", async () => {
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  await user.click(await screen.findByRole("button", { name: "Adopt Codex session" }))
  await user.click(await screen.findByRole("button", { name: `Use ${READY.title}` }))
  await user.click(screen.getByRole("button", { name: "Adopt session" }))

  let request: Request | undefined
  await waitFor(() => {
    request = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
      .find(({ url }) => new URL(url).pathname === `/v1/tasks/${READY.id}/author-session`)
    expect(request).toBeDefined()
  })
  if (!request) throw new Error("no adoption request")
  expect(request.method).toBe("POST")
  expect(JSON.parse(await request.text())).toEqual({
    agent_kind: OUTSIDE.agent_kind,
    internal_session_id: OUTSIDE.internal_session_id,
  })
})
