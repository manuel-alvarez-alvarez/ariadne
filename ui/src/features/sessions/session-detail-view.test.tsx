// @vitest-environment jsdom

/**
 * The session view as every panel mounts it: the facts on top, then the two
 * tabs over the one space.
 *
 * Three things are worth pinning down here. The tabs are the layout the whole
 * view now hangs off, and the console is the one that must be open without
 * being asked for — a session is opened to watch its agent. Leaving that tab
 * unmounts the console, so coming back has to open a *new* socket rather
 * than leave the view holding a dead one; that is the trade the tabs take, and
 * it is only correct as long as the reconnect actually happens.
 *
 * Which tab that is comes from `?tab=`, so a link can point at what an agent
 * reported and a reload comes back on it. The param is shared with the panels
 * this view is drilled into, which is why a value that is not one of these two
 * has to read as the console rather than as nothing.
 *
 * And two things a session says about itself that are neither: what it runs
 * on, which is the tail of the Agent fact and no longer a Model row saying
 * the same thing again — and what it has spent, which is zero rather than
 * blank for an agent that has reported nothing yet.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useLocation } from "react-router-dom"
import { beforeEach, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { FakeWebSocket, stubWebSocket } from "@/test/web-socket"
import { SessionDetailView } from "./session-detail-view"

const GOAL: GoalDto = aGoal()

const TASK: TaskDto = aTask({
  title: "Wire the tabs",
  branch: "wire-the-tabs-000001",
  goal_id: GOAL.id,
})

const SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000001",
  model: "claude-agent-acp:claude-opus-5",
  effort: "xhigh",
  usage: { input_tokens: 12_345, cached_input_tokens: 10_000, output_tokens: 950 },
  last_activity_at: "2026-01-01T00:10:00Z",
  goal_id: GOAL.id,
  task_id: TASK.id,
  task_agent_id: "01JAGENT0000000000000AUTH",
})

beforeEach(() => {
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
        : []
    return Promise.resolve(jsonResponse(body))
  })
  stubWebSocket()
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

/** The terminal sockets opened so far, in order. */
function sockets(): FakeWebSocket[] {
  return FakeWebSocket.instances
}

it("opens on the console, with the activity feed a tab away", async () => {
  const user = userEvent.setup()
  renderView()

  expect(screen.getByRole("tab", { name: "Console" }).getAttribute("data-active")).not.toBeNull()
  // The console is live: one socket, opened by the terminal that is mounted.
  await waitFor(() => expect(sockets()).toHaveLength(1))
  expect(sockets()[0]?.readyState).not.toBe(FakeWebSocket.CLOSED)

  await user.click(screen.getByRole("tab", { name: "Agent activity" }))

  // The console is gone rather than hidden, and its socket went with it —
  // which is the whole reason the two halves can share the space.
  expect(sockets()[0]?.readyState).toBe(FakeWebSocket.CLOSED)
  await screen.findByText("No agent events yet")

  await user.click(screen.getByRole("tab", { name: "Console" }))

  // Back on a socket of its own: every one draws the transcript afresh, so
  // the console is as functional as it was before the detour.
  await waitFor(() => expect(sockets()).toHaveLength(2))
  expect(sockets()[1]?.readyState).not.toBe(FakeWebSocket.CLOSED)
})

it("takes its tab from the URL, and puts a switch back into it", async () => {
  const user = userEvent.setup()
  renderView(SESSION, "/sessions?session=s1&tab=activity")

  // The link opened on the feed, so the console was never mounted at all.
  await screen.findByText("No agent events yet")
  expect(sockets()).toHaveLength(0)

  await user.click(screen.getByRole("tab", { name: "Console" }))

  await waitFor(() => expect(currentSearch().get("tab")).toBe("terminal"))
  await waitFor(() => expect(sockets()).toHaveLength(1))
})

it("falls back to the console for a tab that is not one of its own", () => {
  // `?tab=sessions` is what a goal's or a task's panel leaves on the URL while
  // it is drilled into a session — this strip is not the one it names.
  renderView(SESSION, "/goals?goal=g1&tab=sessions&session=s1")

  expect(screen.getByRole("tab", { name: "Console" }).getAttribute("data-active")).not.toBeNull()
})

it("shows the model the session was launched with, once", async () => {
  renderView()

  // The profile has `claude-agent-acp:claude-sonnet-5` pinned today; what this
  // agent runs is the snapshot taken when it started, not the profile as
  // edited since.
  await waitFor(() => expect(detail("Agent")).toContain("Author"))
  expect(detail("Agent")).toContain("claude-agent-acp:claude-opus-5 @ xhigh")
  // And it says it once: a Model row under this one carried the same tail with
  // the agent taken off it.
  expect(screen.queryByText("Model")).toBeNull()
})

it("shows what the session's agent has spent, as the pair it is", () => {
  renderView()

  // What was sent and what came back, each behind its own arrow, with the
  // share of the input the cache served on the half it is a property of; the
  // exact counts are the hint behind them.
  expect(detail("Tokens")).toBe("12k in, 81% cached, 950 out")
})

it("says zero for a session that has reported nothing yet", () => {
  renderView({
    ...SESSION,
    usage: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
  })

  expect(detail("Tokens")).toBe("0 in, 0% cached, 0 out")
})
