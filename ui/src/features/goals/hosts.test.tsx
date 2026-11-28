// @vitest-environment jsdom

/**
 * The goal screens across hosts: the board merges the selected hosts' goals,
 * a goal's panel opens on its host, the goal dialog reads the host it creates
 * on, and the attention strip answers a remote host's question on that host.
 */

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { AttentionItemDto } from "@/api"
import { DetailPanels } from "@/components/detail-panels"
import { EventStreamProvider } from "@/events/provider"
import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { aGoal, aSessionPage } from "@/test/fixtures"
import { errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { byContent, contentOf } from "@/test/text-content"
import { AttentionPopover } from "./attention-strip"
import { GoalsListPage } from "./goals-list-page"

const MINE = aGoal({ id: "01JLOCALGOAL0000000000000A", title: "Goal on local" })
const THEIRS = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })

type ToastOptions = { description: string; action: { label: string; onClick: () => void } }
const toasted = vi.hoisted(() => vi.fn<(title: string, options?: ToastOptions) => void>())
vi.mock("sonner", () => ({ toast: { success: toasted, error: vi.fn(), dismiss: vi.fn() } }))

const copied = vi.hoisted(() => vi.fn())
vi.mock("@/lib/clipboard", async (original) => ({
  ...(await original<typeof import("@/lib/clipboard")>()),
  copyText: copied,
}))

/** What every list the board and its strip read answers when it has nothing. */
const EMPTY = {
  "/v1/tasks": [],
  "/v1/sessions": aSessionPage([]),
  "/v1/attention": { items: [], complete: true },
}

beforeEach(() => {
  localStorage.clear()
  useSettingsStore.setState({ goalStatusFilter: "", hostFilter: "" })
})

afterEach(() => {
  forgetHosts()
  useSettingsStore.setState({ hostFilter: "" })
})

it("merges the goals of every host under All, each goal naming its host", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  const theirs = await screen.findByRole("link", { name: THEIRS.title })
  expect(screen.getByRole("link", { name: MINE.title })).toBeTruthy()
  expect(theirs.getAttribute("href")).toBe(`/goals?goal=${THEIRS.id}&host=box`)
  const named = screen.getAllByTitle("Host").map((badge) => badge.textContent)
  expect(named.sort()).toEqual(["box", "local"])
})

it("reads only the picked host, and names no host on its goals", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  expect(await screen.findByRole("link", { name: THEIRS.title })).toBeTruthy()
  expect(screen.queryByText(MINE.title)).toBeNull()
  expect(screen.queryAllByTitle("Host")).toHaveLength(0)
  expect(
    seen.some(
      (request) => request.origin.includes("127.0.0.1") && request.url.pathname === "/v1/goals",
    ),
  ).toBe(false)
})

it("names each goal's host under a saved pick the daemon no longer lists", async () => {
  useSettingsStore.setState({ hostFilter: "gone" })
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: THEIRS.title })
  await screen.findByRole("link", { name: MINE.title })
  expect(goalHosts()).toEqual(["box", "local"])
})

it("keeps the host of the goals that stay when a named host goes down", async () => {
  stubHosts({ local: { ...EMPTY, "/v1/goals": [MINE] } }, [
    LOCAL,
    { ...BOX, status: "down", error: "connection refused" },
  ])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: MINE.title })
  await waitFor(() => expect(goalHosts()).toEqual(["local"]))
})

it("names no host on the goals with only local listed", async () => {
  stubHosts({ local: { ...EMPTY, "/v1/goals": [MINE] } }, [LOCAL])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: MINE.title })
  expect(screen.queryAllByTitle("Host")).toHaveLength(0)
})

const STUCK: AttentionItemDto = {
  id: "recovery:resource:descriptor-limit",
  occurrence: "",
  producer: "recovery",
  reason: "resource",
  summary: "The daemon ran out of file descriptors.",
  required_action: "Free file descriptors, then retry the task.",
  since: "2026-11-17T10:00:00Z",
  affected: [{ kind: "task", id: "01OTHER", label: "another task" }],
  target: { kind: "task", task_id: "01OTHER" },
}

it("names local on a local attention row under All", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [], "/v1/attention": { items: [STUCK], complete: true } },
    box: { ...EMPTY, "/v1/goals": [] },
  })
  renderScreen(<AttentionPopover />, { route: "/goals" })

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const row = (await screen.findByText(STUCK.summary)).closest("li") as HTMLElement
  expect(within(row).getByTitle("Host").textContent).toBe("local")
})

it("lists the attention rows of every host under one picked host, each naming its host", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [], "/v1/attention": { items: [STUCK], complete: true } },
    box: { ...EMPTY, "/v1/goals": [] },
  })
  renderScreen(<AttentionPopover />, { route: "/goals" })

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const row = (await screen.findByText(STUCK.summary)).closest("li") as HTMLElement
  expect(within(row).getByTitle("Host").textContent).toBe("local")
})

it("names the goal panel's host, local included, while a named host is listed", async () => {
  stubEventSource()
  stubHosts({ local: { ...EMPTY, [`/v1/goals/${MINE.id}`]: MINE } })
  renderScreen(<DetailPanels />, { route: `/goals?goal=${MINE.id}` })

  await screen.findByRole("heading", { name: MINE.title })
  await waitFor(() =>
    expect(screen.getAllByTitle("Host").map((label) => label.textContent)).toEqual(["local"]),
  )
})

it("names no host in the goal panel with only local listed", async () => {
  stubEventSource()
  stubHosts({ local: { ...EMPTY, [`/v1/goals/${MINE.id}`]: MINE } }, [LOCAL])
  renderScreen(<DetailPanels />, { route: `/goals?goal=${MINE.id}` })

  await screen.findByRole("heading", { name: MINE.title })
  expect(screen.queryAllByTitle("Host")).toHaveLength(0)
})

it("copies the attach command of a goal of a named host with its host", async () => {
  const user = userEvent.setup()
  stubEventSource()
  stubHosts({ box: { ...EMPTY, [`/v1/goals/${THEIRS.id}`]: THEIRS } })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?goal=${THEIRS.id}&host=box` },
  )

  await screen.findByRole("heading", { name: THEIRS.title })
  await user.click(screen.getByRole("button", { name: "Copy goal id" }))
  await user.click(await screen.findByRole("menuitem", { name: "Copy attach command" }))

  await waitFor(() => expect(copied).toHaveBeenCalled())
  expect(copied.mock.calls[0]?.[0]).toBe(`ariadne attach ${THEIRS.id} --host box`)
})

it("opens a goal's panel on the host its link names", async () => {
  const seen = stubHosts({
    box: { ...EMPTY, [`/v1/goals/${THEIRS.id}`]: THEIRS },
  })
  stubEventSource()
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?goal=${THEIRS.id}&host=box` },
  )

  expect(await screen.findByRole("heading", { name: THEIRS.title })).toBeTruthy()
  expect(seen.find((request) => request.url.pathname === `/v1/goals/${THEIRS.id}`)?.origin).toBe(
    "http://box.test:7676",
  )
})

it("shows a question a remote host's stream delivers, and answers it on that host", async () => {
  stubEventSource()
  const question: AttentionItemDto = {
    id: "question:q1",
    occurrence: "q1",
    producer: "agent_request",
    reason: "unknown",
    summary: "Pick a database",
    required_action: "Answer the question",
    since: "2026-11-17T10:00:00Z",
    affected: [{ kind: "session", id: "s-box", label: "orchestrator" }],
    target: { kind: "console", session_id: "s-box" },
  }
  let asked: AttentionItemDto[] = []
  const seen = stubHosts({
    local: { ...EMPTY, "/v1/goals": [] },
    box: {
      ...EMPTY,
      "/v1/goals": [],
      "/v1/attention": () => jsonResponse({ items: asked, complete: true }),
      "/v1/sessions/s-box/questions": [
        {
          id: "q1",
          session_id: "s-box",
          message: "Which database?",
          schema: {
            type: "object",
            properties: {
              db: {
                type: "string",
                title: "Database",
                oneOf: [
                  { const: "pg", title: "Postgres" },
                  { const: "sqlite", title: "SQLite" },
                ],
              },
            },
          },
          created_at: "2026-11-17T10:00:00Z",
        },
      ],
      "/v1/sessions/s-box/questions/q1/answer": () => new Response(null, { status: 204 }),
    },
  })
  renderScreen(
    <EventStreamProvider>
      <AttentionPopover />
    </EventStreamProvider>,
    { route: "/goals" },
  )
  const box = await waitFor(() => {
    const source = FakeEventSource.instances.find((one) => one.url.startsWith("http://box.test"))
    if (!source) throw new Error("no stream on box yet")
    return source
  })

  asked = [question]
  act(() => {
    box.succeed()
    box.emit("attention_changed", {})
  })

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const card = await screen.findByRole("region", { name: "Agent question" })
  const row = card.closest("li") as HTMLElement
  expect(within(row).getByTitle("Host").textContent).toBe("box")
  fireEvent.click(within(card).getByRole("button", { name: "Postgres" }))

  await waitFor(() => expect(within(card).getByRole("status").textContent).toBe("Answer sent."))
  const answer = seen.find((request) => request.url.pathname.endsWith("/answer"))
  expect(answer?.origin).toBe("http://box.test:7676")
  expect(answer?.method).toBe("POST")
  expect(answer?.url.pathname).toBe("/v1/sessions/s-box/questions/q1/answer")
  expect(JSON.parse(answer?.body ?? "{}")).toEqual({ action: "accept", content: { db: "pg" } })
})

/** The host each goal names, sorted. */
function goalHosts(): string[] {
  return screen
    .getAllByTitle("Host")
    .filter((badge) => !badge.closest("p"))
    .map((badge) => badge.textContent ?? "")
    .sort()
}

function failing(message: string) {
  return () => errorResponse(500, "internal", message)
}

it("says which listed host is not shown and why", async () => {
  stubHosts({ local: { ...EMPTY, "/v1/goals": [MINE] } }, [
    LOCAL,
    { ...BOX, status: "down", error: "connection refused" },
  ])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  const line = (await screen.findByText(/^Not shown:/)) as HTMLElement
  expect(within(line).getByTitle("Host").textContent).toBe("box")
  expect(line.textContent).toBe("Not shown:box(connection refused)")
})

it("names the host that still loads while another has answered", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { "/v1/tasks": [], "/v1/sessions": aSessionPage([]) },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: MINE.title })
  const loading = screen.getByText(/^Still loading:/) as HTMLElement
  expect(within(loading).getByTitle("Host").textContent).toBe("box")
})

it("says the tasks of a host are not loaded in its goals, and names the host, local too", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE], "/v1/tasks": failing("tasks broke on local") },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  const warning = await screen.findByText(byContent("Could not load tasks on local"))
  const alert = warning.closest("[role=alert]") as HTMLElement
  expect(within(alert).getByText(/tasks broke on local/)).toBeTruthy()
  expect(within(alert).queryByText(/may be outdated/)).toBeNull()
  const goal = screen.getByRole("group", { name: `${MINE.title}: Tasks` })
  expect(within(goal).getByText("Tasks are not loaded")).toBeTruthy()
  const other = screen.getByRole("group", { name: `${THEIRS.title}: Tasks` })
  expect(within(other).getByText(/No tasks/)).toBeTruthy()
})

it("says the last read may be outdated only while one is on screen", async () => {
  let broken = false
  stubHosts(
    {
      local: {
        ...EMPTY,
        "/v1/goals": [MINE],
        "/v1/tasks": () => (broken ? failing("gone")() : jsonResponse([])),
      },
    },
    [LOCAL],
  )
  const { queryClient } = renderScreen(<GoalsListPage />, { route: "/goals" })
  await screen.findByRole("link", { name: MINE.title })
  broken = true
  await act(() => queryClient.invalidateQueries({ queryKey: ["local", "tasks"] }))

  expect(await screen.findByText(byContent("Could not refresh tasks on local"))).toBeTruthy()
  expect(screen.getByText(/may be outdated/)).toBeTruthy()
})

it("keeps the goals and says tasks are not loaded when every host's task read fails", async () => {
  stubHosts({ local: { ...EMPTY, "/v1/goals": [MINE], "/v1/tasks": failing("tasks broke") } }, [
    LOCAL,
  ])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  expect(await screen.findByText(byContent("Could not load tasks on local"))).toBeTruthy()
  const goal = screen.getByRole("group", { name: `${MINE.title}: Tasks` })
  expect(within(goal).getByText("Tasks are not loaded")).toBeTruthy()
  expect(within(goal).queryByText(/No tasks/)).toBeNull()
})

it("shows the goals as not loaded while every task read is pending", async () => {
  stubHosts({ local: { "/v1/goals": [MINE], "/v1/sessions": aSessionPage([]) } }, [LOCAL])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  const goal = await screen.findByRole("group", { name: `${MINE.title}: Tasks` })
  expect(within(goal).getByText("Tasks are not loaded")).toBeTruthy()
  expect(within(goal).queryByText(/No tasks/)).toBeNull()
})

it("names the host in the title of a goal that failed to load there", async () => {
  stubEventSource()
  stubHosts({ box: { [`/v1/goals/${THEIRS.id}`]: () => errorResponse(500, "internal", "unwell") } })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?goal=${THEIRS.id}&host=box` },
  )

  expect(await screen.findByText(byContent("Could not load goal on box"))).toBeTruthy()
})

it("names the host in the toast of a goal action on a named host", async () => {
  const user = userEvent.setup()
  stubEventSource()
  stubHosts({
    box: {
      ...EMPTY,
      [`/v1/goals/${THEIRS.id}`]: THEIRS,
      [`/v1/goals/${THEIRS.id}/cancel`]: { ...THEIRS, status: "cancelled" },
    },
  })
  renderScreen(
    <EventStreamProvider>
      <DetailPanels />
    </EventStreamProvider>,
    { route: `/goals?goal=${THEIRS.id}&host=box` },
  )

  await user.click(await screen.findByRole("button", { name: "Cancel goal" }))
  const dialog = await screen.findByRole("dialog")
  await user.click(within(dialog).getByRole("button", { name: "Cancel goal" }))

  await waitFor(() => expect(toasted).toHaveBeenCalled())
  expect(contentOf(toasted.mock.calls[0]?.[0])).toBe("Goal cancelled on box")
})

it("keeps skeleton rows below answered goals until every host answers", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { "/v1/tasks": [], "/v1/sessions": aSessionPage([]) },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })
  await screen.findByRole("link", { name: MINE.title })
  expect(screen.getByLabelText("Loading goals")).toBeTruthy()
})
