// @vitest-environment jsdom

import { fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type {
  AcpAgentDto,
  components,
  GoalDto,
  ModelDto,
  RepositoryDto,
  SessionDto,
  TaskDto,
} from "@/api"
import { aGoal, aModel, aRepository, aSession, aTask } from "@/test/fixtures"
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

const ACTIVE_GOAL: GoalDto = aGoal({
  id: "01JGOAL00000000000000ACTIVE",
  title: "Active adoption goal",
  status: "active",
})

const COMPLETED_GOAL: GoalDto = aGoal({
  id: "01JGOAL000000000000COMPLETE",
  title: "Finished goal",
  status: "completed",
})

const CONTAINING_REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO000000000000CONTAIN",
  path: "/Users/me/dev",
})

const OTHER_REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO000000000000000OTHER",
  path: "/Users/me/elsewhere",
})

const AUTHOR_MODEL: ModelDto = aModel({
  id: "claude-agent-acp:sonnet-5",
  agent_id: "claude-agent-acp",
})

const REVIEWER_MODEL: ModelDto = aModel({
  id: "codex-acp:gpt-5",
  agent_id: "codex-acp",
})

/** The task returned after adoption. */
const ADOPTED_TASK: TaskDto = aTask({
  id: "01JTASK000000000000000READY",
  goal_id: ACTIVE_GOAL.id,
  status: "in_progress",
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
  goals = [ACTIVE_GOAL],
  repositories = [CONTAINING_REPOSITORY, OTHER_REPOSITORY],
  models = [AUTHOR_MODEL, REVIEWER_MODEL],
  acpAgents = [],
}: {
  adopted?: SessionDto
  outside?: OutsideSessionDto[]
  /** What each listing request is answered with, by the query it carries. */
  page?: (query: URLSearchParams) => OutsideSessionPageDto
  goals?: GoalDto[]
  repositories?: RepositoryDto[]
  models?: ModelDto[]
  acpAgents?: AcpAgentDto[]
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    if (url.pathname === "/v1/outside-sessions") {
      return Promise.resolve(jsonResponse(page ? page(url.searchParams) : aPage(outside)))
    }
    if (url.pathname === "/v1/acp-agents") return Promise.resolve(jsonResponse(acpAgents))
    if (url.pathname === "/v1/goals") return Promise.resolve(jsonResponse(goals))
    if (url.pathname === "/v1/repositories") return Promise.resolve(jsonResponse(repositories))
    if (url.pathname === "/v1/models") return Promise.resolve(jsonResponse(models))
    if (url.pathname === "/v1/skills") return Promise.resolve(jsonResponse([]))
    if (url.pathname === "/v1/outside-sessions/adopt" && request.method === "POST") {
      return Promise.resolve(
        jsonResponse({
          goal: ACTIVE_GOAL,
          task: ADOPTED_TASK,
          session: adopted ?? aSession({ task_id: ADOPTED_TASK.id }),
        }),
      )
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

async function openAdoption() {
  const user = userEvent.setup()
  await user.click(await screen.findByRole("button", { name: `Adopt ${OUTSIDE.agent_id} session` }))
  return user
}

async function pickModel(user: ReturnType<typeof userEvent.setup>, label: string, model: string) {
  await user.click(screen.getByRole("button", { name: label }))
  await user.click(await screen.findByRole("option", { name: model }))
}

it("offers only active goals", async () => {
  stubDaemon({ goals: [ACTIVE_GOAL, COMPLETED_GOAL] })
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  const user = await openAdoption()

  await user.click(await screen.findByRole("combobox", { name: "Active goal" }))

  expect(await screen.findByRole("option", { name: ACTIVE_GOAL.title })).toBeTruthy()
  expect(screen.queryByRole("option", { name: COMPLETED_GOAL.title })).toBeNull()
})

it("prefills a new goal with the repository containing the working directory", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  const user = await openAdoption()

  await user.click(screen.getByRole("tab", { name: "New goal" }))

  expect(
    await screen.findByRole("button", { name: `Remove ${CONTAINING_REPOSITORY.path}` }),
  ).toBeTruthy()
  expect(screen.queryByRole("button", { name: `Remove ${OTHER_REPOSITORY.path}` })).toBeNull()
})

it("offers only models from the outside session's agent", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  const user = await openAdoption()

  await user.click(await screen.findByRole("button", { name: "Author runs on" }))

  expect(await screen.findByRole("option", { name: AUTHOR_MODEL.id })).toBeTruthy()
  expect(screen.queryByRole("option", { name: REVIEWER_MODEL.id })).toBeNull()
  expect(screen.queryByRole("option", { name: /Other/ })).toBeNull()
})

it("sends the outside session, existing goal, and author before reviewers", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  const user = await openAdoption()

  await user.click(await screen.findByRole("combobox", { name: "Active goal" }))
  await user.click(await screen.findByRole("option", { name: ACTIVE_GOAL.title }))
  await pickModel(user, "Author runs on", AUTHOR_MODEL.id)
  await pickModel(user, "Reviewer 1 runs on", REVIEWER_MODEL.id)
  await user.click(screen.getByRole("combobox", { name: "Permission mode" }))
  await user.click(await screen.findByRole("option", { name: "Ask every time" }))
  await user.click(screen.getByRole("button", { name: "Adopt session" }))

  let request: Request | undefined
  await waitFor(() => {
    request = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
      .find(({ url }) => new URL(url).pathname === "/v1/outside-sessions/adopt")
    expect(request).toBeDefined()
  })
  if (!request) throw new Error("no adoption request")
  expect(request.method).toBe("POST")
  expect(JSON.parse(await request.text())).toEqual({
    agent_id: OUTSIDE.agent_id,
    internal_session_id: OUTSIDE.internal_session_id,
    goal: { id: ACTIVE_GOAL.id },
    title: OUTSIDE.first_prompt,
    description: "",
    agents: [
      { seat: "author", skills: ["coding"], model: AUTHOR_MODEL.id },
      { seat: "reviewer", skills: ["code-review"], model: REVIEWER_MODEL.id },
    ],
    landing: "merge",
    permission_mode: "ask",
  })
})

it("sends a new goal with its description and repositories", async () => {
  renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside" })
  const user = await openAdoption()

  await user.click(screen.getByRole("tab", { name: "New goal" }))
  await user.type(screen.getByLabelText("Goal title"), "Continue the outside work")
  await user.type(screen.getByLabelText("Goal description"), "Keep its original context.")
  await pickModel(user, "Author runs on", AUTHOR_MODEL.id)
  await user.click(screen.getByRole("button", { name: "Remove reviewer 1" }))
  await user.click(screen.getByRole("button", { name: "Adopt session" }))

  const request = await waitFor(() => {
    const found = daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
      .find(({ url }) => new URL(url).pathname === "/v1/outside-sessions/adopt")
    expect(found).toBeDefined()
    return found
  })
  if (!request) throw new Error("no adoption request")
  expect(JSON.parse(await request.text())).toMatchObject({
    goal: {
      title: "Continue the outside work",
      description: "Keep its original context.",
      repository_ids: [CONTAINING_REPOSITORY.id],
    },
  })
})

it("opens the adopted task's panel after success", async () => {
  const { location } = renderScreen(<OutsideSessionsPage />, { route: "/sessions/outside?q=flaky" })
  const user = await openAdoption()

  await user.click(await screen.findByRole("combobox", { name: "Active goal" }))
  await user.click(await screen.findByRole("option", { name: ACTIVE_GOAL.title }))
  await pickModel(user, "Author runs on", AUTHOR_MODEL.id)
  await user.click(screen.getByRole("button", { name: "Remove reviewer 1" }))
  await user.click(screen.getByRole("button", { name: "Adopt session" }))

  await waitFor(() =>
    expect(location.url).toBe(`/sessions/outside?q=flaky&task=${ADOPTED_TASK.id}`),
  )
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
