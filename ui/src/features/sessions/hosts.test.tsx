// @vitest-environment jsdom

/**
 * The sessions screens across hosts: the list merges the selected hosts, and
 * a remote session's panel and console talk to that session's host.
 */

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it } from "vitest"
import type { PullRequestDto } from "@/api"
import { DetailPanels } from "@/components/detail-panels"
import { EventStreamProvider } from "@/events/provider"
import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
import { errorResponse, renderScreen } from "@/test/harness"
import {
  BOX as BOX_HOST,
  forgetHosts,
  LOCAL as LOCAL_HOST_ROW,
  LOCAL_ORIGIN,
  stubHosts,
} from "@/test/hosts"
import { byContent } from "@/test/text-content"
import { SessionsPage } from "./sessions-page"

const BOX = "http://box.test:7676"
const GOAL = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })
const TASK = aTask({ id: "01JBOXTASK000000000000000T", goal_id: GOAL.id, title: "Task on box" })
const REMOTE = aSession({ id: "01JBOXSESS00000000000000S1", goal_id: GOAL.id, task_id: TASK.id })
const LOCAL = aSession({ id: "01JLOCSESS00000000000000S2", goal_id: "g-local", task_id: "t-local" })

beforeEach(() => {
  stubEventSource()
  localStorage.clear()
  useSettingsStore.setState({ hostFilter: "" })
})
afterEach(() => forgetHosts())

it("merges the sessions of every host, each row naming its host, and opens one on its host", async () => {
  stubHosts({
    local: {
      "/v1/sessions": aSessionPage([LOCAL]),
      "/v1/goals": [],
      "/v1/tasks": [],
    },
    box: {
      "/v1/sessions": aSessionPage([REMOTE]),
      "/v1/goals": [GOAL],
      "/v1/tasks": [TASK],
    },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const title = await screen.findByRole("row", { name: "Task on box" })
  expect(within(title).getByTitle("Host").textContent).toBe("box")
  expect(
    screen
      .getAllByTitle("Host")
      .map((badge) => badge.textContent)
      .sort(),
  ).toEqual(["box", "local"])
})

it("applies the picked host to the list and names no host", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({
    local: { "/v1/sessions": aSessionPage([LOCAL]), "/v1/goals": [], "/v1/tasks": [] },
    box: {
      "/v1/sessions": aSessionPage([REMOTE]),
      "/v1/goals": [GOAL],
      "/v1/tasks": [TASK],
    },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  await screen.findByRole("row", { name: "Task on box" })
  expect(screen.queryAllByTitle("Host")).toHaveLength(0)
  expect(
    seen.some(
      (request) => request.origin === LOCAL_ORIGIN && request.url.pathname === "/v1/sessions",
    ),
  ).toBe(false)
})

it("names each row's host under a saved pick the daemon no longer lists", async () => {
  useSettingsStore.setState({ hostFilter: "gone" })
  stubHosts({
    local: { "/v1/sessions": aSessionPage([LOCAL]), "/v1/goals": [], "/v1/tasks": [] },
    box: { "/v1/sessions": aSessionPage([REMOTE]), "/v1/goals": [GOAL], "/v1/tasks": [TASK] },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const title = await screen.findByRole("row", { name: "Task on box" })
  expect(within(title).getByTitle("Host").textContent).toBe("box")
})

const BOTH = {
  local: { "/v1/sessions": aSessionPage([LOCAL]), "/v1/goals": [], "/v1/tasks": [] },
  box: { "/v1/sessions": aSessionPage([REMOTE]), "/v1/goals": [GOAL], "/v1/tasks": [TASK] },
}

it("says which listed host is not shown and why, with host badges on the rows", async () => {
  stubHosts(BOTH, [LOCAL_HOST_ROW, { ...BOX_HOST, status: "down", error: "connection refused" }])
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const line = (await screen.findByText(/^Not shown:/)) as HTMLElement
  expect(within(line).getByTitle("Host").textContent).toBe("box")
  expect(line.textContent).toBe("Not shown:box(connection refused)")
  await waitFor(() =>
    expect(
      screen
        .getAllByTitle("Host")
        .filter((badge) => !badge.closest("p"))
        .map((badge) => badge.textContent),
    ).toEqual(["local"]),
  )
})

it("names the host that still loads while another has answered", async () => {
  stubHosts({ ...BOTH, box: { "/v1/goals": [], "/v1/tasks": [] } })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const loading = (await screen.findByText(/^Still loading:/)) as HTMLElement
  expect(within(loading).getByTitle("Host").textContent).toBe("box")
  expect(await screen.findByRole("row", { name: /local/ })).toBeTruthy()
})

it("keeps the host of a goal chip and applies it only while that host is selected", async () => {
  useSettingsStore.setState({ hostFilter: "local" })
  const seen = stubHosts(BOTH)
  renderScreen(<SessionsPage />, { route: `/sessions?goal=${GOAL.id}&goal_host=box` })

  await screen.findByRole("row", { name: /local/ })
  expect(screen.queryByRole("button", { name: "Show sessions for every goal" })).toBeNull()
  const asked = seen.filter((one) => one.url.pathname === "/v1/sessions")
  expect(asked.every((one) => one.url.searchParams.get("goal") === null)).toBe(true)
  // The chip is kept for when its host is selected again.
  expect(useSettingsStore.getState().sessionGoalHost).toBe("box")

  act(() => useSettingsStore.setState({ hostFilter: "" }))
  expect(await screen.findByRole("button", { name: "Show sessions for every goal" })).toBeTruthy()
  await waitFor(() =>
    expect(
      seen.some(
        (one) =>
          one.origin === BOX &&
          one.url.pathname === "/v1/sessions" &&
          one.url.searchParams.get("goal") === GOAL.id,
      ),
    ).toBe(true),
  )
})

it("takes the host of a goal picked in a row, and Clear filters removes both chips", async () => {
  const user = userEvent.setup()
  stubHosts(BOTH)
  renderScreen(<SessionsPage />, { route: "/sessions?status=failed" })

  await user.click(
    await screen.findByRole("button", { name: "Show the sessions of the goal Goal on box" }),
  )
  await user.click(
    await screen.findByRole("button", { name: "Show the sessions of the task Task on box" }),
  )
  expect(useSettingsStore.getState().sessionGoalHost).toBe("box")
  expect(useSettingsStore.getState().sessionTaskHost).toBe("box")
  expect(screen.getByRole("button", { name: "Show sessions for every goal" })).toBeTruthy()
  expect(screen.getByRole("button", { name: "Show sessions for every task" })).toBeTruthy()

  await user.click(screen.getByRole("button", { name: "Clear filters" }))
  expect(screen.queryByRole("button", { name: "Show sessions for every goal" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Show sessions for every task" })).toBeNull()
  expect(useSettingsStore.getState().sessionGoalFilter).toBe("")
  expect(useSettingsStore.getState().sessionTaskFilter).toBe("")
})

it("names the host of the session panel while a named host is listed", async () => {
  boxSession()

  await screen.findByRole("heading", { name: /./ })
  await waitFor(() =>
    expect(screen.getAllByTitle("Host").map((label) => label.textContent)).toEqual(["box"]),
  )
})

function boxSession() {
  const seen = stubHosts({
    box: {
      [`/v1/sessions/${REMOTE.id}`]: REMOTE,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/sessions/${REMOTE.id}/agent-requests`]: [],
      [`/v1/sessions/${REMOTE.id}/console/input`]: () => new Response(null, { status: 204 }),
      [`/v1/sessions/${REMOTE.id}/console/cancel`]: () => new Response(null, { status: 204 }),
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/sessions?session=${REMOTE.id}&host=box` },
  )
  return seen
}

async function consoleStream(): Promise<FakeEventSource> {
  return waitFor(() => {
    const stream = FakeEventSource.instances.find((one) => one.url.includes("/console/stream"))
    if (!stream) throw new Error("no console stream yet")
    return stream
  })
}

it("opens the console stream of a remote session on its host and sends input there", async () => {
  const user = userEvent.setup()
  const seen = boxSession()
  const stream = await consoleStream()
  expect(stream.url).toBe(`${BOX}/v1/sessions/${REMOTE.id}/console/stream`)

  act(() => {
    stream.succeed()
    stream.emit("snapshot", [
      {
        id: "01EVENT0000000000000000001",
        kind: "agent_message",
        payload: { text: "hello from box" },
        summary: "agent_message",
        created_at: "2026-11-17T10:00:00Z",
      },
    ])
  })
  expect(await screen.findByText("hello from box")).toBeTruthy()

  await user.type(screen.getByRole("textbox", { name: "Message the agent" }), "ping{Enter}")

  await waitFor(() =>
    expect(
      seen.filter(
        (request) => request.method === "POST" && request.url.pathname.endsWith("/console/input"),
      ),
    ).toHaveLength(1),
  )
  const sent = seen.find((request) => request.url.pathname.endsWith("/console/input"))
  expect(sent?.origin).toBe(BOX)
  expect(JSON.parse(sent?.body ?? "{}")).toEqual({ text: "ping" })
})

it("cancels a turn of a remote session on its host and shows the host's stream result", async () => {
  const seen = boxSession()
  const stream = await consoleStream()
  act(() => {
    stream.succeed()
    stream.emit("snapshot", [
      {
        id: "01EVENT0000000000000000002",
        kind: "user_prompt_submit",
        payload: { text: "go" },
        summary: "user_prompt_submit",
        created_at: "2026-11-17T10:00:00Z",
      },
    ])
  })

  fireEvent.click(await screen.findByRole("button", { name: "Stop the turn" }))

  await waitFor(() =>
    expect(seen.some((request) => request.url.pathname.endsWith("/console/cancel"))).toBe(true),
  )
  const cancel = seen.find((request) => request.url.pathname.endsWith("/console/cancel"))
  expect(cancel?.origin).toBe(BOX)
  expect(cancel?.method).toBe("POST")

  act(() =>
    stream.emit("event", {
      id: "01EVENT0000000000000000003",
      kind: "agent_message",
      payload: { text: "stopped on box" },
      summary: "agent_message",
      created_at: "2026-11-17T10:00:01Z",
    }),
  )
  expect(await screen.findByText(byContent("stopped on box"))).toBeTruthy()
})

it("titles each row from its own host when two hosts share a goal and task id", async () => {
  const sameGoal = aGoal({ id: "01JSAMEGOAL000000000000000", title: "Goal of local" })
  const sameTask = aTask({
    id: "01JSAMETASK000000000000000",
    goal_id: sameGoal.id,
    title: "Task of local",
  })
  const here = aSession({
    id: "01JSAME00000000000000000L1",
    goal_id: sameGoal.id,
    task_id: sameTask.id,
  })
  const there = aSession({
    id: "01JSAME00000000000000000B1",
    goal_id: sameGoal.id,
    task_id: sameTask.id,
  })
  stubHosts({
    local: {
      "/v1/sessions": aSessionPage([here]),
      "/v1/goals": [sameGoal],
      "/v1/tasks": [sameTask],
    },
    box: {
      "/v1/sessions": aSessionPage([there]),
      "/v1/goals": [{ ...sameGoal, title: "Goal of box" }],
      "/v1/tasks": [{ ...sameTask, title: "Task of box" }],
    },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  expect(await screen.findByRole("row", { name: "Task of box" })).toBeTruthy()
  expect(screen.getByRole("row", { name: "Task of local" })).toBeTruthy()
  expect(screen.getByText("Goal of box")).toBeTruthy()
})

it("names the host in the titles of a session that failed to load there", async () => {
  stubHosts({
    box: { [`/v1/sessions/${REMOTE.id}`]: () => errorResponse(500, "internal", "box is unwell") },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/sessions?session=${REMOTE.id}&host=box` },
  )

  expect(await screen.findByText(byContent(/^Could not load session .* on box$/))).toBeTruthy()
})

it("names the host in the title of a sessions list that failed on a named host", async () => {
  stubHosts({
    box: {
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      "/v1/sessions": () => errorResponse(500, "internal", "box is unwell"),
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?task=${TASK.id}&host=box&tab=sessions` },
  )

  expect(await screen.findByText(byContent("Could not load sessions on box"))).toBeTruthy()
})

it("names the host in the title of an activity tab that failed on a named host", async () => {
  const user = userEvent.setup()
  stubHosts({
    box: {
      [`/v1/sessions/${REMOTE.id}`]: REMOTE,
      [`/v1/goals/${GOAL.id}`]: GOAL,
      [`/v1/tasks/${TASK.id}`]: TASK,
      [`/v1/sessions/${REMOTE.id}/agent-requests`]: [],
      "/v1/events": () => errorResponse(500, "internal", "box is unwell"),
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/sessions?session=${REMOTE.id}&host=box` },
  )

  await user.click(await screen.findByRole("tab", { name: "Agent activity" }))
  expect(await screen.findByText(byContent("Could not load agent events on box"))).toBeTruthy()
})

it("reads the request link of a pull request session on the session's host", async () => {
  const session = aSession({ ...REMOTE, pull_request_id: "01JBOXPULL000000000000000P" })
  const pull = {
    id: session.pull_request_id,
    title: "Fix it on box",
    number: 7,
    url: "https://forge.test/acme/app/pull/7",
  } as PullRequestDto
  stubHosts({
    box: {
      [`/v1/sessions/${session.id}`]: session,
      [`/v1/pull-requests/${session.pull_request_id}`]: pull,
      [`/v1/sessions/${session.id}/agent-requests`]: [],
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/sessions?session=${session.id}&host=box` },
  )

  const link = await screen.findByRole("link", { name: /Fix it on box|#7/ })
  expect(link.getAttribute("href")).toBe(pull.url)
})

it("reads the request link of a pull request row of the sessions table on its host", async () => {
  const session = aSession({
    ...REMOTE,
    goal_id: null,
    task_id: null,
    pull_request_id: "01JBOXPULL000000000000000P",
  })
  const pull = {
    id: session.pull_request_id,
    title: "Fix it on box",
    number: 7,
    url: "https://forge.test/acme/app/pull/7",
  } as PullRequestDto
  stubHosts({
    local: { "/v1/sessions": aSessionPage([]), "/v1/goals": [], "/v1/tasks": [] },
    box: {
      "/v1/sessions": aSessionPage([session]),
      "/v1/goals": [],
      "/v1/tasks": [],
      [`/v1/pull-requests/${session.pull_request_id}`]: pull,
    },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })

  const link = await screen.findByRole("link", { name: /Fix it on box|#7/ })
  expect(link.getAttribute("href")).toBe(pull.url)
})

it("links a worker session to its source goal without reading a worker goal", async () => {
  const task = aTask({
    ...TASK,
    execution_context: {
      source_goal: {
        daemon_id: "source-id",
        goal_id: GOAL.id,
        title: "Saved source",
        description: "",
      },
      execution_owner: "worker-id",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  const seen = stubHosts({
    local: { "/v1/remote-tasks/protocol": { protocol: "remote-tasks-v1", daemon_id: "source-id" } },
    box: {
      [`/v1/sessions/${REMOTE.id}`]: REMOTE,
      [`/v1/tasks/${TASK.id}`]: task,
      "/v1/remote-tasks/protocol": { protocol: "remote-tasks-v1", daemon_id: "worker-id" },
    },
  })
  renderScreen(<DetailPanels />, { route: `/sessions?session=${REMOTE.id}&host=box` })
  const link = await screen.findByRole("link", { name: "Saved source" })
  expect(link.getAttribute("href")).toContain(`goal=${GOAL.id}`)
  expect(link.getAttribute("href")).not.toContain("host=box")
  expect(
    seen.some(
      (request) => request.origin === BOX && request.url.pathname === `/v1/goals/${GOAL.id}`,
    ),
  ).toBe(false)
})

it("names a worker session by the task's saved workflow", async () => {
  const task = aTask({
    ...TASK,
    agents: [{ id: "worker-agent", step: "build", skills: [], model: "codex:o3" }],
    execution_context: {
      source_goal: { daemon_id: "source", goal_id: GOAL.id, title: "Source", description: "" },
      execution_owner: "worker",
      workflow: {
        name: "saved",
        steps: [{ id: "build", title: "Saved column", description: "", skills: [], ranks: [] }],
      },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  const session = aSession({ ...REMOTE, seat: "agent", task_agent_id: "worker-agent" })
  stubHosts({
    local: { "/v1/sessions": aSessionPage([]), "/v1/goals": [], "/v1/tasks": [] },
    box: { "/v1/sessions": aSessionPage([session]), "/v1/goals": [], "/v1/tasks": [task] },
  })
  renderScreen(<SessionsPage />, { route: "/sessions" })
  expect(await screen.findByText("Saved column")).toBeTruthy()
  expect(screen.getByText("Source")).toBeTruthy()
})
