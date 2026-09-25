// @vitest-environment jsdom

/**
 * The sessions screen: the merged table over `GET /v1/sessions` (Ariadne's
 * own, whole) and `GET /v1/sessions?kind=outside` (a page at a time).
 *
 * What is worth pinning: the two kinds read down one list, newest activity
 * first; an outside row's empty status, goal and task, and its agent and
 * directory shown as every row's is; every filter reaching the daemon under its own
 * name, on the endpoint that takes it; paging through `next_cursor` with the
 * total kept in view, the Ariadne half never asked to page since it never
 * arrives in pages; and picking a row — which resumes an outside session
 * first, then opens the console of the session the daemon hands back, where
 * an Ariadne row opens straight away.
 */

import { cleanup, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { components, GoalDto, SessionDto, TaskDto } from "@/api"
import { SEAT_LABELS, shortId } from "@/lib/format"
import { useSettingsStore } from "@/stores/settings"
import {
  aGoal,
  aModel,
  anOutsideSession,
  anOutsideSessionPage,
  aSession,
  aSessionPage,
  aTask,
} from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import type { OutsideSessionDto } from "./queries"
import { SessionsPage } from "./sessions-page"

type AcpAgentDto = components["schemas"]["AcpAgentDto"]
type SessionPageDto = components["schemas"]["SessionPageDto"]

const GOAL: GoalDto = aGoal()
const TASK: TaskDto = aTask({ goal_id: GOAL.id, title: "Wire the review flow" })

const ENGINEER: SessionDto = aSession({
  id: "01JSESS0000000000000000ENG",
  goal_id: GOAL.id,
  task_id: TASK.id,
  worktree_path: "/Users/me/.ariadne/worktrees/goal/task-eng",
  last_activity_at: "2026-01-03T00:00:00Z",
})

const PLANNER: SessionDto = aSession({
  id: "01JSESS0000000000000000PLA",
  goal_id: GOAL.id,
  task_id: null,
  seat: "orchestrator",
  status: "failed",
  attention_reason: "disconnected",
  last_activity_at: "2026-01-02T00:00:00Z",
})

const OUTSIDE: OutsideSessionDto = anOutsideSession({
  agent_id: "claude-agent-acp",
  internal_session_id: "acp-session-1",
  working_directory: "/Users/me/dev/ariadne",
  last_activity_at: "2026-01-01T00:00:00Z",
  first_prompt: "Fix the flaky test.",
})

/** The live session `POST /v1/outside-sessions/resume` hands back. */
const RESUMED: SessionDto = aSession({
  id: "01JSESS0000000000RESUMED1",
  goal_id: null,
  task_id: null,
  seat: null,
  task_agent_id: null,
  model: "claude-agent-acp:claude-sonnet-5",
  internal_session_id: OUTSIDE.internal_session_id,
  worktree_path: OUTSIDE.working_directory,
  status: "running",
})

function anAcpAgent(overrides: Partial<AcpAgentDto> = {}): AcpAgentDto {
  return {
    id: "claude-agent-acp",
    command: ["claude-agent-acp"],
    source: "registry",
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

function anOutsidePage(
  sessions: OutsideSessionDto[],
  page: Partial<SessionPageDto> = {},
): SessionPageDto {
  return anOutsideSessionPage(sessions, { snapshot_at: "2026-01-03T00:30:00Z", ...page })
}

function stubDaemon({
  sessions = [ENGINEER, PLANNER],
  outside = [OUTSIDE],
  outsidePage,
  goals = [GOAL],
  tasks = [TASK],
  acpAgents = [],
  resumed = RESUMED,
}: {
  sessions?: SessionDto[]
  outside?: OutsideSessionDto[]
  outsidePage?: (query: URLSearchParams) => SessionPageDto
  goals?: GoalDto[]
  tasks?: TaskDto[]
  acpAgents?: AcpAgentDto[]
  resumed?: SessionDto
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    const kind = url.searchParams.get("kind")
    if (url.pathname === "/v1/sessions" && request.method === "GET" && kind === "ariadne") {
      const status = url.searchParams.get("status")
      const goal = url.searchParams.get("goal")
      const task = url.searchParams.get("task")
      const filtered = sessions.filter(
        (session) =>
          (!status || session.status === status) &&
          (!goal || session.goal_id === goal) &&
          (!task || session.task_id === task),
      )
      return Promise.resolve(jsonResponse(aSessionPage(filtered)))
    }
    if (url.pathname === "/v1/sessions" && request.method === "GET" && kind === "outside") {
      return Promise.resolve(
        jsonResponse(outsidePage ? outsidePage(url.searchParams) : anOutsidePage(outside)),
      )
    }
    if (url.pathname === "/v1/sessions" && request.method === "POST") {
      return Promise.resolve(jsonResponse(resumed))
    }
    if (url.pathname === "/v1/models") {
      return Promise.resolve(
        jsonResponse([
          aModel({ id: "claude-agent-acp:claude-sonnet-5", agent_id: "claude-agent-acp" }),
        ]),
      )
    }
    if (url.pathname === "/v1/outside-sessions/resume" && request.method === "POST") {
      return Promise.resolve(jsonResponse(resumed))
    }
    if (url.pathname === "/v1/goals") return Promise.resolve(jsonResponse(goals))
    if (url.pathname === "/v1/tasks") return Promise.resolve(jsonResponse(tasks))
    if (url.pathname === "/v1/acp-agents") return Promise.resolve(jsonResponse(acpAgents))
    return Promise.resolve(jsonResponse([]))
  })
}

/** The query of every listing of one kind of session, oldest first. */
function queriesTo(kind: "ariadne" | "outside"): URLSearchParams[] {
  return daemonFetch.mock.calls
    .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
    .filter((request) => request.method === "GET")
    .map(({ url }) => new URL(url))
    .filter((url) => url.pathname === "/v1/sessions" && url.searchParams.get("kind") === kind)
    .map((url) => url.searchParams)
}

function renderPage(entry = "/sessions") {
  return renderScreen(<SessionsPage />, { route: entry }).location
}

/** The row a session is on, found by its title cell's own `title=` attribute
 * — distinct from the goal or task cell, which can show the same text. */
function row(title: string): HTMLElement {
  const cell = screen.getByTitle(title)
  const found = cell.closest("tr")
  if (!found) throw new Error(`no row around ${title}`)
  return found
}

beforeEach(() => {
  stubDaemon()
  localStorage.clear()
  useSettingsStore.setState({
    sessionStatusFilter: "",
    sessionRoleFilter: "",
    sessionGoalFilter: "",
    sessionTaskFilter: "",
  })
})

it("lists Ariadne sessions and outside sessions together, newest activity first", async () => {
  renderPage()

  await waitFor(() => row(TASK.title))
  const titles = screen
    .getAllByRole("row")
    .slice(1)
    .map((tr) => tr.textContent ?? "")

  // TASK.title (ENGINEER) moved last, OUTSIDE's first prompt first: the
  // order the two lists merge into.
  const engineerIndex = titles.findIndex((text) => text.includes(TASK.title))
  const outsideIndex = titles.findIndex((text) => text.includes(OUTSIDE.first_prompt))
  expect(engineerIndex).toBeGreaterThanOrEqual(0)
  expect(outsideIndex).toBeGreaterThan(engineerIndex)
})

it("shows an outside row's empty status, goal and task, and names its agent and directory", async () => {
  renderPage()

  const found = await waitFor(() => row(OUTSIDE.first_prompt))
  expect(found.textContent).toContain(OUTSIDE.agent_id)
  expect(found.textContent).toContain(OUTSIDE.working_directory)
  // No status badge, and no goal or task title or id: the cells read as the
  // dash every other empty fact in the app does.
  expect(found.textContent).toContain("—")
})

it("names a loose session by its own title, and one without a title by what it is", async () => {
  stubDaemon({
    sessions: [
      aSession({ ...RESUMED, title: "Fix the flaky test." }),
      aSession({ ...RESUMED, id: "01JSESS000000000000UNTITLED", title: null }),
    ],
    outside: [],
  })
  renderPage()

  await waitFor(() => row("Fix the flaky test."))
  expect(row("Loose session")).not.toBeNull()
})

it("sends each filter to the daemon under the name that filter has, on the endpoint that takes it", async () => {
  stubDaemon({ acpAgents: [anAcpAgent()] })
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(TASK.title))

  await user.click(screen.getByRole("button", { name: "Filter by agent" }))
  await user.click(await screen.findByRole("menuitemradio", { name: OUTSIDE.agent_id }))
  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Failed" }))
  await user.click(screen.getByRole("button", { name: "Filter by directory" }))
  await user.type(await screen.findByLabelText("Working directory"), "/Users/me/dev")
  // Straight on to the search, closing the popover mid-settle: what was
  // typed in it is sent all the same.
  await user.type(screen.getByLabelText("Search titles"), "flaky")

  await waitFor(
    () => {
      const ariadneQuery = queriesTo("ariadne").at(-1)
      expect(ariadneQuery?.get("status")).toBe("failed")
      const outsideQuery = queriesTo("outside").at(-1)
      expect(outsideQuery?.get("agent")).toBe(OUTSIDE.agent_id)
      expect(outsideQuery?.get("dir")).toBe("/Users/me/dev")
      expect(outsideQuery?.get("q")).toBe("flaky")
    },
    { timeout: 3000 },
  )
})

it("sends a day's activity window as the moments that bound it, in UTC", async () => {
  const { fireEvent } = await import("@testing-library/react")
  renderPage()
  await waitFor(() => row(TASK.title))

  await userEvent.setup().click(screen.getByRole("button", { name: "Filter by activity" }))
  fireEvent.change(await screen.findByLabelText("Active since"), {
    target: { value: "2026-01-01" },
  })
  fireEvent.change(screen.getByLabelText("Active until"), { target: { value: "2026-01-03" } })

  await waitFor(() => {
    const query = queriesTo("outside").at(-1)
    expect(query?.get("since")).toBe("2026-01-01T00:00:00Z")
    expect(query?.get("until")).toBe("2026-01-03T23:59:59.999999999Z")
  })
})

it("sends a preset activity window as the day it starts on, and names it on the trigger", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(TASK.title))

  await user.click(screen.getByRole("button", { name: "Filter by activity" }))
  await user.click(await screen.findByRole("button", { name: "Last 7 days" }))

  await waitFor(() => {
    const query = queriesTo("outside").at(-1)
    expect(query?.get("since")).toMatch(/^\d{4}-\d{2}-\d{2}T00:00:00Z$/)
    expect(query?.get("until")).toBeNull()
  })
  expect(screen.getByRole("button", { name: "Filter by activity" }).textContent).toContain(
    "Last 7 days",
  )
})

it("clears every filter of the bar at once, the search field included, and keeps the scope", async () => {
  stubDaemon({ sessions: [ENGINEER] })
  const user = userEvent.setup()
  const seen = renderPage(`/sessions?goal=${GOAL.id}&status=failed&agent=codex-acp&q=flaky`)
  expect(await screen.findByDisplayValue("flaky")).toBeTruthy()

  await user.click(screen.getByRole("button", { name: "Clear filters" }))

  await waitFor(() => expect(seen.url).toBe(`/sessions?goal=${GOAL.id}`))
  expect((screen.getByLabelText("Search titles") as HTMLInputElement).value).toBe("")
  expect(screen.queryByRole("button", { name: "Clear filters" })).toBeNull()
  expect(useSettingsStore.getState().sessionStatusFilter).toBe("")
})

it("pages the outside half through next_cursor, keeping the Ariadne rows, and counts the total", async () => {
  const LATER = anOutsideSession({
    agent_id: "codex-acp",
    internal_session_id: "acp-session-2",
    first_prompt: "Rename the store module.",
  })
  stubDaemon({
    outsidePage: (query) =>
      query.get("cursor") === "cursor-1"
        ? anOutsidePage([LATER], { total: 2 })
        : anOutsidePage([OUTSIDE], { next_cursor: "cursor-1", total: 2 }),
  })
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(TASK.title))
  // Two Ariadne rows plus the one outside page loaded so far, out of the
  // two Ariadne rows plus the outside total.
  expect(await screen.findByText("3 of 4 sessions")).toBeTruthy()

  await user.click(screen.getByRole("button", { name: "Load more" }))

  expect(await screen.findByText(LATER.first_prompt)).toBeTruthy()
  expect(screen.getByText(OUTSIDE.first_prompt)).toBeTruthy()
  expect(await screen.findByText("4 of 4 sessions")).toBeTruthy()
  expect(queriesTo("outside").at(-1)?.get("cursor")).toBe("cursor-1")
})

it("asks every outside agent again when Refresh is pressed", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(TASK.title))
  expect(queriesTo("outside").at(-1)?.get("refresh")).toBeNull()

  await user.click(screen.getByRole("button", { name: "Refresh" }))

  await waitFor(() => expect(queriesTo("outside").at(-1)?.get("refresh")).toBe("true"))
})

it("opens an Ariadne row's own panel directly, asking the resume endpoint for nothing", async () => {
  const user = userEvent.setup()
  const seen = renderPage()
  await waitFor(() => row(TASK.title))

  await user.click(row(TASK.title))

  await waitFor(() => expect(seen.url).toBe(`/sessions?session=${ENGINEER.id}`))
  expect(
    daemonFetch.mock.calls.some(([input]) => {
      const url = new URL(input instanceof Request ? input.url : String(input))
      return url.pathname === "/v1/outside-sessions/resume"
    }),
  ).toBe(false)
})

it("resumes an outside row once, then opens the console of the session it answers", async () => {
  const user = userEvent.setup()
  const seen = renderPage()
  await waitFor(() => row(OUTSIDE.first_prompt))

  await user.click(row(OUTSIDE.first_prompt))

  const resumeRequests = () =>
    daemonFetch.mock.calls
      .map(([input, init]) => (input instanceof Request ? input : new Request(input, init)))
      .filter((request) => new URL(request.url).pathname === "/v1/outside-sessions/resume")
  await waitFor(() => expect(resumeRequests()).toHaveLength(1))
  const [request] = resumeRequests()
  if (!request) throw new Error("no resume request")
  const body = JSON.parse(await request.text())
  expect(body).toEqual({
    agent_id: OUTSIDE.agent_id,
    internal_session_id: OUTSIDE.internal_session_id,
  })
  await waitFor(() => expect(seen.url).toBe(`/sessions?session=${RESUMED.id}`))
})

it("shows the seat, the goal and the task of an Ariadne row together, in one Work column", async () => {
  renderPage()
  await waitFor(() => row(TASK.title))

  const cells = within(row(TASK.title))
  expect(cells.getByText(SEAT_LABELS[ENGINEER.seat ?? "author"])).toBeTruthy()
  expect(
    cells.getByRole("button", { name: `Show the sessions of the goal ${GOAL.title}` }),
  ).toBeTruthy()
  expect(
    cells.getByRole("button", { name: `Show the sessions of the task ${TASK.title}` }),
  ).toBeTruthy()
  const headers = screen.getAllByRole("columnheader").map((header) => header.textContent)
  expect(headers).toContain("Work")
  expect(headers).not.toContain("Goal")
  expect(headers).not.toContain("Task")
})

it("narrows the table to a goal picked in the Work column, opening no panel", async () => {
  stubDaemon({ sessions: [ENGINEER] })
  const user = userEvent.setup()
  const seen = renderPage()
  await waitFor(() => row(TASK.title))

  await user.click(
    within(row(TASK.title)).getByRole("button", {
      name: `Show the sessions of the goal ${GOAL.title}`,
    }),
  )

  await waitFor(() => expect(seen.url).toBe(`/sessions?goal=${GOAL.id}`))
})

it("shows where a task's agent runs under its title, as it does for every row", async () => {
  renderPage()

  await waitFor(() => row(TASK.title))
  expect(within(row(TASK.title)).getByText(ENGINEER.worktree_path ?? "")).toBeTruthy()
})

it("shows a resumed outside row once, as the session that holds it, with its directory", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(OUTSIDE.first_prompt))
  // From the resume on, the daemon lists the conversation as the session
  // that holds it, titled by its first prompt, and no longer as outside.
  stubDaemon({
    sessions: [ENGINEER, PLANNER, { ...RESUMED, title: OUTSIDE.first_prompt }],
    outside: [],
  })

  await user.click(row(OUTSIDE.first_prompt))

  await waitFor(() => expect(within(row(OUTSIDE.first_prompt)).getByText("Running")).toBeTruthy())
  expect(screen.getAllByTitle(OUTSIDE.first_prompt)).toHaveLength(1)
  expect(within(row(OUTSIDE.first_prompt)).getByText(OUTSIDE.working_directory)).toBeTruthy()
})

it("starts a new session from the header and opens its console", async () => {
  const user = userEvent.setup()
  const seen = renderPage()

  await user.click(screen.getByRole("button", { name: "New session" }))
  await user.click(await screen.findByRole("button", { name: "Runs on" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText("claude-agent-acp:claude-sonnet-5"))
  await user.keyboard("{Escape}")
  const dialog = screen.getByRole("dialog", { name: "New session" })
  await user.type(within(dialog).getByLabelText("Working directory"), "/Users/me/dev/ariadne")
  await user.click(screen.getByRole("button", { name: "Start session" }))

  await waitFor(() =>
    expect(seen.url).toBe(`/sessions?session=${RESUMED.id}&tab=terminal&focus=terminal`),
  )
})

it("narrows to one goal from a scope chip, skipping the outside half, and clears it", async () => {
  // Only the task-titled row: the orchestrator's own row's title falls back
  // to the goal's name too, which would otherwise collide with the chip.
  stubDaemon({ sessions: [ENGINEER] })
  const user = userEvent.setup()
  const seen = renderPage(`/sessions?goal=${GOAL.id}`)
  await waitFor(() => row(TASK.title))

  const clear = await screen.findByRole("button", { name: "Show sessions for every goal" })
  expect(screen.getByTitle(GOAL.title)).toBeTruthy()
  await waitFor(() => expect(queriesTo("ariadne").at(-1)?.get("goal")).toBe(GOAL.id))
  // A goal filter leaves nothing an outside row could match, so its half is
  // never asked for.
  expect(queriesTo("outside")).toHaveLength(0)

  await user.click(clear)

  await waitFor(() => expect(seen.url).toBe("/sessions"))
})

it("comes back to the status and seat filters the screen was left with", async () => {
  const user = userEvent.setup()
  renderPage()
  await waitFor(() => row(TASK.title))

  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Failed" }))
  await waitFor(() => expect(queriesTo("ariadne").at(-1)?.get("status")).toBe("failed"))

  cleanup()
  daemonFetch.mockClear()
  stubDaemon()
  const seen = renderPage()

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed"))
})

it("calls an empty list empty when nothing is filtered", async () => {
  stubDaemon({ sessions: [], outside: [] })
  renderPage()

  expect(await screen.findByText("No sessions found.")).toBeTruthy()
})

it("names the scope by its short id while the app has no title for it", async () => {
  const gone = "01JGOAL0000000000000GONE01"
  renderPage(`/sessions?goal=${gone}`)

  expect(await screen.findByText(shortId(gone))).toBeTruthy()
})
