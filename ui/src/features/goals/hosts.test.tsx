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
import { aGoal, aModel, aRepository, aSessionPage } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { forgetHosts, stubHosts } from "@/test/hosts"
import { AttentionPopover } from "./attention-strip"
import { CreateGoalDialog } from "./create-goal-dialog"
import { GoalsListPage } from "./goals-list-page"

const MINE = aGoal({ id: "01JLOCALGOAL0000000000000A", title: "Goal on local" })
const THEIRS = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })

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

it("merges the goals of every host under All, each lane naming its host", async () => {
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  const theirs = await screen.findByRole("link", { name: THEIRS.title })
  expect(screen.getByRole("link", { name: MINE.title })).toBeTruthy()
  expect(theirs.getAttribute("href")).toBe(`/goals?goal=${THEIRS.id}&host=box`)
  const lanes = screen.getAllByTitle("Host").map((badge) => badge.textContent)
  expect(lanes.sort()).toEqual(["box", "local"])
})

it("reads only the picked host, and names no host on its lanes", async () => {
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

it("defaults the goal dialog to the picked host, and reloads its lists when the host changes", async () => {
  const user = userEvent.setup()
  useSettingsStore.setState({ hostFilter: "box" })
  const seen = stubHosts({
    local: {
      "/v1/repositories": [aRepository({ id: "r-local", path: "/src/local-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
    },
    box: {
      "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
    },
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const host = await screen.findByRole("combobox", { name: "Host" })
  await waitFor(() => expect(host.textContent).toContain("box"))
  await waitFor(() =>
    expect(reads(seen, "box")).toEqual(["/v1/models", "/v1/repositories", "/v1/workflows"]),
  )
  expect(reads(seen, "local")).toEqual([])

  await user.click(host)
  await user.click(await screen.findByRole("option", { name: "local" }))

  await waitFor(() =>
    expect(reads(seen, "local")).toEqual(["/v1/models", "/v1/repositories", "/v1/workflows"]),
  )
})

it("opens the goal dialog on local under All", async () => {
  stubHosts({ box: {} })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const host = await screen.findByRole("combobox", { name: "Host" })
  await waitFor(() => expect(host.textContent).toContain("local"))
})

it("shows a question a remote host's stream delivers, and answers it on that host", async () => {
  stubEventSource()
  const question: AttentionItemDto = {
    id: "agent_request:r1",
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
      "/v1/sessions/s-box/agent-requests": [
        {
          id: "r1",
          created_at: "2026-11-17T10:00:00Z",
          summary: "Pick a database",
          question: {
            text: "Which database?",
            options: [
              { id: "pg", label: "Postgres" },
              { id: "sqlite", label: "SQLite" },
            ],
          },
        },
      ],
      "/v1/sessions/s-box/agent-requests/r1/answer": () => new Response(null, { status: 204 }),
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
  const card = await screen.findByRole("region", { name: "Question · Pick a database" })
  const row = card.closest("li") as HTMLElement
  expect(within(row).getByTitle("Host").textContent).toBe("box")
  fireEvent.click(within(card).getByRole("button", { name: "Postgres" }))

  await waitFor(() => expect(within(card).getByRole("status").textContent).toBe("Answer sent."))
  const answer = seen.find((request) => request.url.pathname.endsWith("/answer"))
  expect(answer?.origin).toBe("http://box.test:7676")
  expect(answer?.method).toBe("POST")
  expect(answer?.url.pathname).toBe("/v1/sessions/s-box/agent-requests/r1/answer")
  expect(JSON.parse(answer?.body ?? "{}")).toEqual({ option_id: "pg" })
})

/** The paths a host was asked to read, each once, sorted. */
function reads(seen: ReturnType<typeof stubHosts>, host: "local" | "box"): string[] {
  const origin = host === "box" ? "http://box.test:7676" : "http://127.0.0.1:7676"
  return [
    ...new Set(
      seen
        .filter((request) => request.origin === origin && request.url.pathname !== "/v1/hosts")
        .filter((request) => request.url.pathname !== "/v1/acp-agents")
        .map((request) => request.url.pathname),
    ),
  ].sort()
}
