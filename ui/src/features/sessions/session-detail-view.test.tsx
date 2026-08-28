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
 * And two things a session says about itself that are neither: the model, null
 * on the wire for a session launched without one, which means the agent CLI
 * chose — a fact about the session, not a blank — and what it has spent, which
 * is zero rather than blank for an agent that has reported nothing yet.
 *
 * xterm needs a browser this environment only half is, so `matchMedia` and
 * `ResizeObserver` are stubbed for it, as in `session-terminal.test.tsx`.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useLocation } from "react-router-dom"
import { beforeEach, expect, it, vi } from "vitest"

import type { GoalDto, ProfileDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aProfile, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { SessionDetailView } from "./session-detail-view"

const GOAL: GoalDto = aGoal()

const TASK: TaskDto = aTask({
  title: "Wire the tabs",
  branch: "wire-the-tabs-000001",
  engineer_profile_id: "01JPROF0000000000000000ENG",
  goal_id: GOAL.id,
})

/** Pinned to a model of its own, which the session below did not launch with. */
const PROFILE: ProfileDto = aProfile({
  id: TASK.engineer_profile_id,
  name: "engineer-default",
  model: "claude_code:claude-sonnet-5",
})

const SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000001",
  model: "claude-opus-5",
  usage: { input_tokens: 12_345, cached_input_tokens: 10_000, output_tokens: 950 },
  last_activity_at: "2026-01-01T00:10:00Z",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: PROFILE.id,
  tmux_session: "ariadne-01JSESS0000000000000000001",
})

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
    return Promise.resolve(jsonResponse(body))
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

function renderView(session: SessionDto = SESSION, entry = "/goals?goal=g1") {
  renderScreen(
    <>
      <CurrentSearch />
      <SessionDetailView session={session} />
    </>,
    { route: entry },
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
  // The profile has `claude_code:claude-sonnet-5` pinned today; what this agent
  // runs is the snapshot taken when it started, not the profile as edited
  // since — and the badge spells that snapshot's two fields as one id.
  await waitFor(() => expect(detail("Profile")).toContain("engineer-default"))
  expect(detail("Profile")).toContain("claude_code:claude-opus-5")
})

it("names the agent CLI's own choice where no model was recorded", () => {
  renderView({ ...SESSION, model: null })

  expect(detail("Model")).toBe("default")
})

it("shows what the session's agent has spent, as the pair it is", () => {
  renderView()

  // What was sent and what came back, each behind its own arrow; the cached
  // share of the input and the exact counts are the hint behind them.
  expect(detail("Tokens")).toBe("12k in, 950 out")
})

it("says zero for a session that has reported nothing yet", () => {
  renderView({
    ...SESSION,
    usage: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
  })

  expect(detail("Tokens")).toBe("0 in, 0 out")
})
