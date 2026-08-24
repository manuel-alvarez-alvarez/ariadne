// @vitest-environment jsdom

/**
 * The session view as every panel mounts it: the facts on top, then the two
 * tabs over the one space.
 *
 * Three things are worth pinning down here. The tabs are the layout the whole
 * view now hangs off, and the terminal is the one that must be open without
 * being asked for — a session is opened to watch a pane. Leaving that tab
 * unmounts the emulator, so coming back has to attach a *new* stream rather
 * than leave the view holding a dead one; that is the trade the tabs take, and
 * it is only correct as long as the reconnect actually happens.
 *
 * Which tab that is comes from `?tab=`, so a link can point at what an agent
 * reported and a reload comes back on it. The param is shared with the panels
 * this view is drilled into, which is why a value that is not one of these two
 * has to read as the terminal rather than as nothing.
 *
 * And the model: it is null on the wire for a session launched without one,
 * which means the agent CLI chose — a fact about the session, not a blank.
 *
 * xterm needs a browser this environment only half is, so `matchMedia` and
 * `ResizeObserver` are stubbed for it, as in `session-terminal.test.tsx`.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"

import { SessionDetailView } from "./session-detail-view"

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
  repo_id: "01JREPO0000000000000000001",
  title: "Wire the tabs",
  description: "",
  status: "in_progress",
  branch: "wire-the-tabs-000001",
  depends_on: [],
  engineer_profile_id: "01JPROF0000000000000000ENG",
  reviewers: [],
  review_round: 0,
  stalled: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

/** Pinned to a model of its own, which the session below did not launch with. */
const PROFILE: ProfileDto = {
  id: TASK.engineer_profile_id,
  name: "engineer-default",
  role: "engineer",
  agent_kind: "claude_code",
  model: "claude-sonnet-5",
  system_prompt: "",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const SESSION: SessionDto = {
  id: "01JSESS0000000000000000001",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: PROFILE.id,
  role: "engineer",
  agent_kind: "claude_code",
  model: "claude-opus-5",
  status: "running",
  tmux_session: "ariadne-01JSESS0000000000000000001",
  created_at: "2026-01-01T00:00:00Z",
  last_activity_at: "2026-01-01T00:10:00Z",
}

/** One log-stream connection, and whether it is still open. */
interface Connection {
  url: string
  closed: boolean
}

let connections: Connection[]

/** Enough of `EventSource` for the terminal's stream to open and be closed. */
class StubEventSource {
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  readonly #connection: Connection

  constructor(url: string) {
    this.#connection = { url, closed: false }
    connections.push(this.#connection)
  }

  addEventListener(): void {}

  close(): void {
    this.#connection.closed = true
  }
}

beforeEach(() => {
  connections = []
  daemonFetch.mockReset()
  // Whatever the view reads to turn its ids into names — the goal, the task,
  // the profiles — plus the empty activity feed. Nothing here is about a
  // request failing.
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body = url.pathname.startsWith("/v1/goals")
      ? GOAL
      : url.pathname.startsWith("/v1/tasks")
        ? TASK
        : url.pathname.startsWith("/v1/profiles")
          ? [PROFILE]
          : []
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
  })
  vi.stubGlobal("EventSource", StubEventSource)
  // xterm measures the device pixel ratio and watches the frame; neither
  // exists here, and neither is what these tests are about.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }))
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    },
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

function renderView(session: SessionDto = SESSION, entry = "/goals?goal=g1") {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  render(
    <MemoryRouter initialEntries={[entry]}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delay={0}>
          <CurrentSearch />
          <SessionDetailView session={session} />
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
}

/** The search string the view has left behind, for the tab it keeps there. */
function CurrentSearch() {
  return <output data-testid="search">{useLocation().search}</output>
}

function currentSearch(): URLSearchParams {
  return new URLSearchParams(screen.getByTestId("search").textContent ?? "")
}

/** The value of a summary row, by the label above it. */
function detail(label: string): string {
  const term = screen.getByText(label)
  const value = term.nextElementSibling
  if (!value) throw new Error(`no value under "${label}"`)
  return value.textContent ?? ""
}

it("opens on the terminal, with the activity feed a tab away", async () => {
  const user = userEvent.setup()
  renderView()

  expect(screen.getByRole("tab", { name: "Terminal" }).getAttribute("data-active")).not.toBeNull()
  // The pane is live: one stream, opened by the emulator that is mounted.
  expect(connections).toHaveLength(1)
  expect(connections[0]?.closed).toBe(false)

  await user.click(screen.getByRole("tab", { name: "Agent activity" }))

  // The terminal is gone rather than hidden, and its stream went with it —
  // which is the whole reason the two halves can share the space.
  expect(connections[0]?.closed).toBe(true)
  await screen.findByText("No agent events yet")

  await user.click(screen.getByRole("tab", { name: "Terminal" }))

  // Back on a connection of its own: every one replays the pane from a
  // snapshot, so the terminal is as functional as it was before the detour.
  expect(connections).toHaveLength(2)
  expect(connections[1]?.closed).toBe(false)
})

it("takes its tab from the URL, and puts a switch back into it", async () => {
  const user = userEvent.setup()
  renderView(SESSION, "/sessions?session=s1&tab=activity")

  // The link opened on the feed, so the emulator was never mounted at all.
  await screen.findByText("No agent events yet")
  expect(connections).toHaveLength(0)

  await user.click(screen.getByRole("tab", { name: "Terminal" }))

  await waitFor(() => expect(currentSearch().get("tab")).toBe("terminal"))
  expect(connections).toHaveLength(1)
})

it("falls back to the terminal for a tab that is not one of its own", () => {
  // `?tab=sessions` is what a goal's or a task's panel leaves on the URL while
  // it is drilled into a session — this strip is not the one it names.
  renderView(SESSION, "/goals?goal=g1&tab=sessions&session=s1")

  expect(screen.getByRole("tab", { name: "Terminal" }).getAttribute("data-active")).not.toBeNull()
})

it("shows the model the session was launched with", async () => {
  renderView()

  expect(detail("Model")).toBe("claude-opus-5")
  // The profile has `claude-sonnet-5` pinned today; what this agent runs is
  // the snapshot taken when it started, not the profile as edited since.
  await waitFor(() => expect(detail("Profile")).toContain("engineer-default"))
  expect(detail("Profile")).toContain("Claude Code · claude-opus-5")
})

it("names the agent CLI's own choice where no model was recorded", () => {
  renderView({ ...SESSION, model: null })

  expect(detail("Model")).toBe("default")
})
