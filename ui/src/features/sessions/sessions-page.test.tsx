// @vitest-environment jsdom

/**
 * The sessions screen: the one place the list is mounted with no goal and no
 * task around it.
 *
 * What is worth pinning is exactly what that unscoped mounting buys, and none
 * of it shows without rendering the screen: the Context column saying which
 * work each row belongs to (a task's title, a planner session's goal), a pick
 * turning into `?session=` over the screen rather than a navigation away from
 * it, and the two filters — one of which the daemon answers (`?status=failed`)
 * and one of which it cannot (`live` is three statuses, so it is narrowed
 * here).
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"
import { formatAbsolute } from "@/lib/time"

import { SessionsPage } from "./sessions-page"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is a stub the daemon client never sees.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const GOAL: GoalDto = {
  id: "01JGOAL0000000000000000001",
  title: "Ship the board",
  description: "",
  planner_profile_id: "01JPROF00000000000000PLAN",
  repos: [],
  required_approvals: 1,
  status: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: GOAL.id,
  title: "Wire the sessions screen",
  description: "",
  status: "in_progress",
  branch: "ariadne/task-01JTASK0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  stalled: false,
  engineer_profile_id: "01JPROF00000000000000ENGI",
  reviewer_profile_ids: [],
  depends_on: [],
  review_round: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const PROFILE: ProfileDto = {
  id: "01JPROF00000000000000ENGI",
  name: "Engineer",
  role: "engineer",
  agent_kind: "claude_code",
  system_prompt: "",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function session(overrides: Partial<SessionDto> & Pick<SessionDto, "id">): SessionDto {
  return {
    goal_id: GOAL.id,
    task_id: TASK.id,
    role: "engineer",
    profile_id: PROFILE.id,
    agent_kind: "claude_code",
    model: null,
    internal_session_id: null,
    tmux_session: `ariadne-${overrides.id}`,
    worktree_path: null,
    review_round: null,
    status: "running",
    attention_reason: null,
    attention_since: null,
    last_activity_at: "2026-01-01T00:00:00Z",
    created_at: "2026-01-01T00:00:00Z",
    ended_at: null,
    ...overrides,
  }
}

/** An engineer at work, and the planner that has no task of its own. */
const ENGINEER = session({ id: "01JSESS0000000000000000ENG" })
const PLANNER = session({
  id: "01JSESS0000000000000000PLA",
  task_id: null,
  role: "planner",
  status: "failed",
})

/** The daemon, answering the four lists the screen reads. */
function stubDaemon(sessions: SessionDto[] = [ENGINEER, PLANNER]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    const status = url.searchParams.get("status")
    const body =
      url.pathname === "/v1/sessions"
        ? sessions.filter((one) => !status || one.status === status)
        : url.pathname === "/v1/goals"
          ? [GOAL]
          : url.pathname === "/v1/tasks"
            ? [TASK]
            : url.pathname === "/v1/profiles"
              ? [PROFILE]
              : []
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
  })
}

/** What `GET /v1/sessions` was asked to filter by, oldest request first. */
function sessionRequests(): (string | null)[] {
  return daemonFetch.mock.calls
    .map(([input]) => new URL(typeof input === "string" ? input : (input as Request).url))
    .filter((url) => url.pathname === "/v1/sessions")
    .map((url) => url.searchParams.get("status"))
}

function renderScreen(entry = "/sessions") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  const seen = { url: entry }
  function Probe() {
    const location = useLocation()
    seen.url = `${location.pathname}${location.search}`
    return null
  }
  render(
    <MemoryRouter initialEntries={[entry]}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delay={0}>
          <SessionsPage />
          <Probe />
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
  return seen
}

/** The row a session is on, found by the role button that opens it. */
async function row(name: string): Promise<HTMLElement> {
  const button = await screen.findByRole("button", { name })
  const found = button.closest("tr")
  if (!found) throw new Error(`no row around ${name}`)
  return found
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  daemonFetch.mockReset()
  stubDaemon()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

it("says which work each session belongs to, and links to it", async () => {
  renderScreen()

  // A session on a task is named by the task; the planner, which has none, by
  // its goal — and the goal panel only opens on the board.
  const engineer = within(await row("Open Engineer session")).getByRole("link", {
    name: TASK.title,
  })
  expect(engineer.getAttribute("href")).toBe(`/sessions?task=${TASK.id}`)

  const planner = within(await row("Open Planner session")).getByRole("link", { name: GOAL.title })
  expect(planner.getAttribute("href")).toBe(`/goals?goal=${GOAL.id}`)
})

it("opens the picked session as a panel over the screen", async () => {
  const user = userEvent.setup()
  const seen = renderScreen()

  await user.click(await screen.findByRole("button", { name: "Open Engineer session" }))

  await waitFor(() => expect(seen.url).toBe(`/sessions?session=${ENGINEER.id}`))
})

it("asks the daemon for one status and keeps it in the URL", async () => {
  const user = userEvent.setup()
  const seen = renderScreen()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))
  expect(sessionRequests()).toEqual([null])

  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Failed" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed"))
  await waitFor(() => expect(sessionRequests()).toContain("failed"))
  expect(await screen.findByRole("button", { name: "Open Planner session" })).toBeTruthy()
  expect(screen.queryByRole("button", { name: "Open Engineer session" })).toBeNull()
})

it("narrows live sessions itself, without asking for a status", async () => {
  const user = userEvent.setup()
  const seen = renderScreen()
  await waitFor(() => expect(sessionRequests().length).toBeGreaterThan(0))

  await user.click(screen.getByRole("button", { name: "Filter by status" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Live" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=live"))
  expect(await screen.findByRole("button", { name: "Open Engineer session" })).toBeTruthy()
  // The failed planner is gone, and the daemon was never asked for a status:
  // "live" is three of them, so the same unfiltered response was reused.
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Open Planner session" })).toBeNull(),
  )
  expect(sessionRequests().every((status) => status === null)).toBe(true)
})

it("filters by role, and blames the filters when nothing is left", async () => {
  const user = userEvent.setup()
  const seen = renderScreen("/sessions?status=failed")
  await waitFor(() => expect(sessionRequests()).toContain("failed"))

  await user.click(screen.getByRole("button", { name: "Filter by role" }))
  await user.click(await screen.findByRole("menuitemradio", { name: "Engineer" }))

  await waitFor(() => expect(seen.url).toBe("/sessions?status=failed&role=engineer"))
  expect(await screen.findByText("No sessions match these filters")).toBeTruthy()
})

it("calls an empty list empty when nothing is filtered", async () => {
  stubDaemon([])
  renderScreen()

  expect(await screen.findByText("No sessions yet")).toBeTruthy()
})

/**
 * The table is the one surface that keeps the compact age as its text — the
 * heading says what it is the age of, and a column of "N minutes ago" is a
 * column of repeated words. Everything the column has no room for is the hint
 * behind it, and the hint opens on focus: both of these were a `title=`, which
 * a keyboard never reaches.
 */
it("puts the stamps behind the table's columns in reach of a keyboard", async () => {
  const user = userEvent.setup()
  renderScreen()
  const engineer = await row("Open Engineer session")

  // The Context cell's link is the trigger, so the pair it names costs no
  // focus stop of its own.
  within(engineer).getByRole("link", { name: TASK.title }).focus()
  expect(await screen.findByText(`Goal: ${GOAL.title}`)).not.toBeNull()

  // The age column is further along the row; Tab walks to it.
  for (let stop = 0; stop < 8; stop++) {
    await user.tab()
    if (screen.queryByText(`last activity ${formatAbsolute(ENGINEER.last_activity_at)}`)) {
      // The two columns the table dropped ride along in the same hint.
      expect(screen.getByText(`started ${formatAbsolute(ENGINEER.created_at)}`)).not.toBeNull()
      return
    }
  }
  throw new Error("the last-activity stamp is not reachable by keyboard")
})
