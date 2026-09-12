// @vitest-environment jsdom

import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { AcpAgentDto, components, SessionDto, TaskDto } from "@/api"
import { aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { OutsideSessionsPage } from "./outside-sessions-page"

type OutsideSessionDto = components["schemas"]["OutsideSessionDto"]

const OUTSIDE: OutsideSessionDto = {
  agent_id: "claude-agent-acp",
  internal_session_id: "acp-session-1",
  working_directory: "/Users/me/dev/other",
  last_activity_at: "2026-09-10T09:00:00Z",
  first_prompt: "Fix the flaky test.",
}

/** A ready task whose author runs the session's agent. */
const READY: TaskDto = aTask({
  id: "01JTASK000000000000000READY",
  status: "ready",
  title: "Adopt an agent session",
  agents: [
    {
      id: "01JAGENT0000000000000AUTH",
      seat: "author",
      skills: ["coding"],
      model: "claude-agent-acp:sonnet-5",
    },
  ],
})

/** A ready task whose author runs another agent. */
const ELSEWHERE: TaskDto = aTask({
  id: "01JTASK0000000000000ELSEWH",
  status: "ready",
  title: "Run on another agent",
  agents: [
    {
      id: "01JAGENT0000000000000ELSE",
      seat: "author",
      skills: ["coding"],
      model: "codex-acp:gpt-5",
    },
  ],
})

function anAcpAgent(overrides: Partial<AcpAgentDto> = {}): AcpAgentDto {
  return {
    id: "claude-agent-acp",
    command: ["claude-agent-acp"],
    builtin: true,
    status: "ready",
    capabilities: {
      stdio: true,
      protocol_v1: true,
      session_new: true,
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

/** Another agent's session, for the pages the cursor walks. */
const LATER: OutsideSessionDto = {
  agent_id: "codex-acp",
  internal_session_id: "acp-session-2",
  working_directory: "/Users/me/dev/ariadne",
  last_activity_at: "2026-09-09T08:00:00Z",
  first_prompt: "Rename the store module.",
}

type OutsideSessionPageDto = components["schemas"]["OutsideSessionPageDto"]

/** One page of the daemon's answer, last by default. */
function aPage(
  sessions: OutsideSessionDto[],
  page: Partial<OutsideSessionPageDto> = {},
): OutsideSessionPageDto {
  return {
    sessions,
    next_cursor: null,
    total: sessions.length,
    snapshot_at: "2026-09-10T09:30:00Z",
    ...page,
  }
}

function stubDaemon({
  adopted,
  outside = [OUTSIDE],
  page,
  tasks = [READY],
  acpAgents = [],
}: {
  adopted?: SessionDto
  outside?: OutsideSessionDto[]
  /** What each listing request is answered with, by the query it carries. */
  page?: (query: URLSearchParams) => OutsideSessionPageDto
  tasks?: TaskDto[]
  acpAgents?: AcpAgentDto[]
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/outside-sessions") {
      return Promise.resolve(jsonResponse(page ? page(url.searchParams) : aPage(outside)))
    }
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

/** The query of every listing request the screen has made, oldest first. */
function listingQueries(): URLSearchParams[] {
  return daemonFetch.mock.calls
    .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
    .map(({ url }) => new URL(url))
    .filter(({ pathname }) => pathname === "/v1/outside-sessions")
    .map(({ searchParams }) => searchParams)
}

function field(label: string): HTMLInputElement {
  return screen.getByLabelText(label) as HTMLInputElement
}

beforeEach(() => stubDaemon())

it("lists each outside session with its agent, directory, activity, and first prompt", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  // Named by its registry id, which is what tells one ACP agent from another.
  expect(await screen.findByText(OUTSIDE.agent_id)).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "Agent" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "Working directory" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "Last activity" })).toBeTruthy()
  expect(screen.getByRole("columnheader", { name: "First prompt" })).toBeTruthy()
  expect(screen.getByText(OUTSIDE.working_directory)).toBeTruthy()
  expect(screen.getByText(OUTSIDE.first_prompt)).toBeTruthy()
  expect(document.querySelector(`time[datetime="${OUTSIDE.last_activity_at}"]`)).not.toBeNull()
})

it("adopts a stored session, sending the registry agent id along with it", async () => {
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  await user.click(await screen.findByRole("button", { name: `Adopt ${OUTSIDE.agent_id} session` }))
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
    agent_id: OUTSIDE.agent_id,
    internal_session_id: OUTSIDE.internal_session_id,
  })
})

it("offers only the ready tasks whose author runs the session's agent", async () => {
  stubDaemon({ tasks: [READY, ELSEWHERE] })
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  await user.click(await screen.findByRole("button", { name: `Adopt ${OUTSIDE.agent_id} session` }))

  expect(await screen.findByRole("button", { name: `Use ${READY.title}` })).toBeTruthy()
  expect(screen.queryByRole("button", { name: `Use ${ELSEWHERE.title}` })).toBeNull()
})

it("sends each filter to the daemon under the name that filter has", async () => {
  stubDaemon({ acpAgents: [anAcpAgent()] })
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  await screen.findByText(OUTSIDE.first_prompt)

  await user.click(screen.getByRole("button", { name: "Filter by agent" }))
  await user.click(await screen.findByRole("menuitemradio", { name: OUTSIDE.agent_id }))
  await user.type(field("Working directory"), "/Users/me/dev")
  await user.type(field("Search first prompts"), "flaky")
  // Five keystrokes are in and the typing has not settled, so no request
  // carries a search yet. A field that asked per letter would have sent five
  // by now — `q=f`, `q=fl`, and so on.
  expect(listingQueries().filter((query) => query.get("q") !== null)).toHaveLength(0)
  // A date picker commits a whole day at once, which is what the daemon is
  // asked for: the first instant of the day as `since`, its last as `until`.
  fireEvent.change(field("Active since"), { target: { value: "2026-09-01" } })
  fireEvent.change(field("Active until"), { target: { value: "2026-09-10" } })

  await waitFor(
    () => {
      const query = listingQueries().at(-1)
      expect(query?.get("agent")).toBe(OUTSIDE.agent_id)
      expect(query?.get("dir")).toBe("/Users/me/dev")
      expect(query?.get("q")).toBe("flaky")
      expect(query?.get("since")).toBe("2026-09-01T00:00:00Z")
      expect(query?.get("until")).toBe("2026-09-10T23:59:59.999999999Z")
    },
    { timeout: 3000 },
  )
})

it("opens on the filters its URL carries, and asks the daemon for them", async () => {
  renderScreen(<OutsideSessionsPage />, {
    route:
      "/sessions/outside?agent=claude-agent-acp&dir=%2FUsers%2Fme%2Fdev&since=2026-09-01&until=2026-09-10&q=flaky",
  })

  await screen.findByText(OUTSIDE.first_prompt)
  expect(screen.getByRole("button", { name: "Filter by agent" }).textContent).toContain(
    "claude-agent-acp",
  )
  expect(field("Working directory").value).toBe("/Users/me/dev")
  expect(field("Active since").value).toBe("2026-09-01")
  expect(field("Active until").value).toBe("2026-09-10")
  expect(field("Search first prompts").value).toBe("flaky")

  const query = listingQueries().at(-1)
  expect(query?.get("agent")).toBe("claude-agent-acp")
  expect(query?.get("dir")).toBe("/Users/me/dev")
  expect(query?.get("since")).toBe("2026-09-01T00:00:00Z")
  expect(query?.get("until")).toBe("2026-09-10T23:59:59.999999999Z")
  expect(query?.get("q")).toBe("flaky")
})

it("loads the page after the cursor the daemon gave, keeping the rows above it", async () => {
  stubDaemon({
    page: (query) =>
      query.get("cursor") === "cursor-1"
        ? aPage([LATER], { total: 2 })
        : aPage([OUTSIDE], { next_cursor: "cursor-1", total: 2 }),
  })
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  await screen.findByText(OUTSIDE.first_prompt)

  await user.click(screen.getByRole("button", { name: "Load more" }))

  expect(await screen.findByText(LATER.first_prompt)).toBeTruthy()
  expect(screen.getByText(OUTSIDE.first_prompt)).toBeTruthy()
  expect(listingQueries().at(-1)?.get("cursor")).toBe("cursor-1")
})

it("asks every agent again when Refresh is pressed", async () => {
  const user = userEvent.setup()
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  // Closed while the first page is in flight: a refetch over a running request
  // is that request, which carries no `refresh` of its own.
  expect((screen.getByRole("button", { name: "Refresh" }) as HTMLButtonElement).disabled).toBe(true)

  await screen.findByText(OUTSIDE.first_prompt)
  expect(listingQueries().at(-1)?.get("refresh")).toBeNull()

  await user.click(screen.getByRole("button", { name: "Refresh" }))

  await waitFor(() => expect(listingQueries().at(-1)?.get("refresh")).toBe("true"))
  // Spent on that request alone: the page after it is cut from the snapshot
  // that answer took.
  expect(listingQueries().filter((query) => query.get("refresh") === "true")).toHaveLength(1)
})

it("counts the sessions on screen out of every one the filters leave", async () => {
  stubDaemon({ page: () => aPage([OUTSIDE], { next_cursor: "cursor-1", total: 3 }) })
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })

  expect(await screen.findByText("1 of 3")).toBeTruthy()
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
