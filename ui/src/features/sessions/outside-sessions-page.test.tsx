// @vitest-environment jsdom

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { AcpAgentDto, components, SessionDto, TaskDto } from "@/api"
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

const ACP_OUTSIDE: OutsideSessionDto = {
  agent_kind: "acp",
  agent_id: "claude-code-acp",
  internal_session_id: "acp-session-1",
  working_directory: "/Users/me/dev/other",
  last_activity_at: "2026-09-10T09:00:00Z",
  first_prompt: "Fix the flaky test.",
}

const READY: TaskDto = aTask({
  id: "01JTASK000000000000000READY",
  status: "ready",
  title: "Show outside sessions",
  agents: [
    { id: "01JAGENT0000000000000AUTH", seat: "author", skills: ["coding"], model: "codex:gpt-5" },
  ],
})

const ACP_READY: TaskDto = aTask({
  id: "01JTASK00000000000000ACPRDY",
  status: "ready",
  title: "Adopt an acp session",
  agents: [
    {
      id: "01JAGENT0000000000000ACPAU",
      seat: "author",
      skills: ["coding"],
      model: "acp:claude-code-acp:sonnet-5",
    },
  ],
})

function anAcpAgent(overrides: Partial<AcpAgentDto> = {}): AcpAgentDto {
  return {
    id: "claude-code-acp",
    command: ["claude-code-acp"],
    builtin: true,
    status: "ready",
    capabilities: {
      stdio: true,
      protocol_v1: true,
      session_new: true,
      session_prompt: true,
      model: true,
      thought_level: true,
      session_list: true,
      session_load: true,
    },
    degraded: [],
    rejection_reason: null,
    ...overrides,
  }
}

function stubDaemon({
  adopted,
  outside = [OUTSIDE],
  tasks = [READY],
  acpAgents = [],
}: {
  adopted?: SessionDto
  outside?: OutsideSessionDto[]
  tasks?: TaskDto[]
  acpAgents?: AcpAgentDto[]
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/outside-sessions") return Promise.resolve(jsonResponse(outside))
    if (url.pathname === "/v1/acp-agents") return Promise.resolve(jsonResponse(acpAgents))
    if (url.pathname === "/v1/tasks") return Promise.resolve(jsonResponse(tasks))
    for (const task of tasks) {
      if (url.pathname === `/v1/tasks/${task.id}/author-session` && request.method === "POST") {
        return Promise.resolve(jsonResponse(adopted ?? aSession({ task_id: task.id })))
      }
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

it("names an ACP session's row by its registry agent id, not by the ACP kind", async () => {
  stubDaemon({ outside: [ACP_OUTSIDE], tasks: [ACP_READY] })
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  expect(await screen.findByText("claude-code-acp")).toBeTruthy()
  expect(screen.queryByText("ACP")).toBeNull()
})

it("adopts a stored ACP session, sending the registry agent id along with it", async () => {
  stubDaemon({ outside: [ACP_OUTSIDE], tasks: [ACP_READY] })
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  await user.click(
    await screen.findByRole("button", { name: `Adopt ${ACP_OUTSIDE.agent_id} session` }),
  )
  await user.click(await screen.findByRole("button", { name: `Use ${ACP_READY.title}` }))
  await user.click(screen.getByRole("button", { name: "Adopt session" }))

  let request: Request | undefined
  await waitFor(() => {
    request = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
      .find(({ url }) => new URL(url).pathname === `/v1/tasks/${ACP_READY.id}/author-session`)
    expect(request).toBeDefined()
  })
  if (!request) throw new Error("no adoption request")
  expect(JSON.parse(await request.text())).toEqual({
    agent_kind: ACP_OUTSIDE.agent_kind,
    agent_id: ACP_OUTSIDE.agent_id,
    internal_session_id: ACP_OUTSIDE.internal_session_id,
  })
})

it("shows why an ACP agent without the session-listing capability offers no adoption", async () => {
  stubDaemon({
    outside: [],
    acpAgents: [
      anAcpAgent({
        id: "codex-acp",
        capabilities: { ...anAcpAgent().capabilities, session_list: false },
      }),
    ],
  })
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  expect(await screen.findByText("codex-acp", { exact: false })).toBeTruthy()
  expect(
    screen.getByText("the agent does not support listing sessions", { exact: false }),
  ).toBeTruthy()
})
