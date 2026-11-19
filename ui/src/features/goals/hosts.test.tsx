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
import { aGoal, aModel, anEffort, aRepository, aSessionPage } from "@/test/fixtures"
import { jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"
import { AttentionPopover } from "./attention-strip"
import { CreateGoalDialog } from "./create-goal-dialog"
import { GoalsListPage } from "./goals-list-page"

const MINE = aGoal({ id: "01JLOCALGOAL0000000000000A", title: "Goal on local" })
const THEIRS = aGoal({ id: "01JBOXGOAL000000000000000B", title: "Goal on box" })

const toasts = vi.hoisted(() => ({
  success: vi.fn<(title: string, options: ToastOptions) => void>(),
}))
type ToastOptions = { description: string; action: { label: string; onClick: () => void } }
vi.mock("sonner", async (original) => ({
  ...(await original<typeof import("sonner")>()),
  toast: Object.assign(vi.fn(), (await original<typeof import("sonner")>()).toast, toasts),
}))

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

it("names each lane's host under a saved pick the daemon no longer lists", async () => {
  useSettingsStore.setState({ hostFilter: "gone" })
  stubHosts({
    local: { ...EMPTY, "/v1/goals": [MINE] },
    box: { ...EMPTY, "/v1/goals": [THEIRS] },
  })
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: THEIRS.title })
  await screen.findByRole("link", { name: MINE.title })
  expect(lanesHosts()).toEqual(["box", "local"])
})

it("keeps the host of the lanes that stay when a named host goes down", async () => {
  stubHosts({ local: { ...EMPTY, "/v1/goals": [MINE] } }, [
    LOCAL,
    { ...BOX, status: "down", error: "connection refused" },
  ])
  renderScreen(<GoalsListPage />, { route: "/goals" })

  await screen.findByRole("link", { name: MINE.title })
  await waitFor(() => expect(lanesHosts()).toEqual(["local"]))
})

it("names no host on the lanes with only local listed", async () => {
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

it("names no host on an attention row under one picked host", async () => {
  useSettingsStore.setState({ hostFilter: "box" })
  stubHosts({
    box: { ...EMPTY, "/v1/goals": [], "/v1/attention": { items: [STUCK], complete: true } },
  })
  renderScreen(<AttentionPopover />, { route: "/goals" })

  fireEvent.click(await screen.findByRole("button", { name: /Needs attention, 1 items/ }))
  const row = (await screen.findByText(STUCK.summary)).closest("li") as HTMLElement
  expect(within(row).queryByTitle("Host")).toBeNull()
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

/** Pick `model` in the open goal dialog's pin picker. */
async function pickModel(user: ReturnType<typeof userEvent.setup>, id: string) {
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await user.click(within(dialog).getByRole("button", { name: "Orchestrator runs on" }))
  const models = await screen.findByRole("listbox", { name: "Models" })
  await user.click(within(models).getByText(id))
  await user.keyboard("{Escape}")
  return dialog
}

/** A model that can be run at `low` or `high`. */
const EFFORT_MODEL = aModel({ efforts: [anEffort({ id: "low" }), anEffort({ id: "high" })] })

it("clears the model and the effort when the goal dialog's host changes", async () => {
  const user = userEvent.setup()
  const answers = {
    "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
    "/v1/models": [EFFORT_MODEL],
    "/v1/workflows": [],
    "/v1/goals": (_url: URL, request: Request) =>
      request.method === "POST" ? jsonResponse(aGoal({ title: "On box" }), 201) : undefined,
  }
  const seen = stubHosts({ local: answers, box: answers })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  const pin = within(dialog).getByRole("button", { name: "Orchestrator runs on" })

  // A model and an effort of it, on local.
  await user.click(pin)
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(EFFORT_MODEL.id),
  )
  await user.click(
    within(await screen.findByRole("listbox", { name: "Efforts" })).getByText("high", {
      selector: "[data-slot=option-label]",
    }),
  )
  await waitFor(() => expect(pin.textContent).toContain("high"))
  if (screen.queryByRole("listbox", { name: "Efforts" })) await user.keyboard("{Escape}")

  await user.click(await within(dialog).findByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "box" }))

  await waitFor(() => expect(pin.textContent).not.toContain("claude-sonnet-5"))
  expect(pin.textContent).not.toContain("high")
  expect(within(dialog).getByRole("button", { name: "Create goal" }).hasAttribute("disabled")).toBe(
    true,
  )

  // The same model again, with no effort picked: none is sent.
  await user.click(pin)
  await user.click(
    within(await screen.findByRole("listbox", { name: "Models" })).getByText(EFFORT_MODEL.id),
  )
  await user.keyboard("{Escape}")
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "On box")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))
  await waitFor(() => expect(seen.some((one) => one.method === "POST")).toBe(true))
  const body = JSON.parse(seen.find((one) => one.method === "POST")?.body ?? "{}")
  expect(body).toMatchObject({ model: EFFORT_MODEL.id })
  expect(body).not.toHaveProperty("effort")
})

async function warnsAndCreates(state: "failed" | "pending", error: string | null) {
  const user = userEvent.setup()
  useSettingsStore.setState({ hostFilter: "box" })
  const behind = { ...BOX, sync: { ...BOX.sync, state, error } }
  let created = false
  stubHosts(
    {
      local: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
      box: {
        "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
        "/v1/models": [aModel()],
        "/v1/workflows": [],
        "/v1/goals": (_url: URL, request: Request) => {
          if (request.method !== "POST") return undefined
          created = true
          return jsonResponse(aGoal({ title: "On box" }), 201)
        },
      },
    },
    [LOCAL, behind],
  )
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)

  const alert = await screen.findByRole("alert")
  expect(alert.textContent).toContain(state)
  if (error) expect(alert.textContent).toContain(error)

  const dialog = await pickModel(user, aModel().id)
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "On box")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))
  await waitFor(() => expect(created).toBe(true))
}

it("warns in the goal dialog of a host whose sync is failed, and still creates", () =>
  warnsAndCreates("failed", "push refused: disk full"))

it("warns in the goal dialog of a host whose sync is pending, and still creates", () =>
  warnsAndCreates("pending", null))

it("names the host in the toast of a goal created on a named host, and offers all hosts when the pick leaves it out", async () => {
  const user = userEvent.setup()
  toasts.success.mockClear()
  useSettingsStore.setState({ hostFilter: "local" })
  stubHosts({
    local: { "/v1/repositories": [], "/v1/models": [], "/v1/workflows": [] },
    box: {
      "/v1/repositories": [aRepository({ id: "r-box", path: "/src/box-repo" })],
      "/v1/models": [aModel()],
      "/v1/workflows": [],
      "/v1/goals": (_url: URL, request: Request) =>
        request.method === "POST" ? jsonResponse(aGoal({ title: "On box" }), 201) : undefined,
    },
  })
  renderScreen(<CreateGoalDialog open onOpenChange={vi.fn()} />)
  const dialog = await screen.findByRole("dialog", { name: "New goal" })
  await user.click(await within(dialog).findByRole("combobox", { name: "Host" }))
  await user.click(await screen.findByRole("option", { name: "box" }))
  await user.click(await within(dialog).findByRole("combobox", { name: "Repositories" }))
  await user.click(await screen.findByText(/box-repo/))
  await user.keyboard("{Escape}")
  await pickModel(user, aModel().id)
  await user.type(within(dialog).getByRole("textbox", { name: "Title" }), "On box")
  await user.click(within(dialog).getByRole("button", { name: "Create goal" }))

  await waitFor(() => expect(toasts.success).toHaveBeenCalled())
  const [title, options] = toasts.success.mock.calls[0] ?? []
  expect(title).toBe("Goal created")
  expect(options?.description).toContain("box")
  expect(options?.action.label).toBe("Show all hosts")
  act(() => options?.action.onClick())
  expect(useSettingsStore.getState().hostFilter).toBe("")
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

/** The host each lane names, sorted. */
function lanesHosts(): string[] {
  return screen
    .getAllByTitle("Host")
    .map((badge) => badge.textContent ?? "")
    .sort()
}

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
