// @vitest-environment jsdom

/**
 * The turn console, driven from the daemon's side: the trajectory stream on
 * an `EventSource` stand-in, and the routes it reads and posts to on the
 * fetch stub.
 */

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { BanIcon, CircleCheckIcon, CircleXIcon, ClockIcon, LoaderCircleIcon } from "lucide-react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { QuestionDto, StepDto, TrajectorySnapshotDto, TurnDto } from "@/api"
import { drawnColours } from "@/test/colours"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"
import {
  aMessageStep,
  anErrorStep,
  anId,
  anUpdate,
  aPermissionStep,
  aQuestionStep,
  aSnapshot,
  aToolStep,
  aTurn,
  text,
} from "@/test/trajectory"

import { consoleViewKey, updateViewState } from "../console/console-view-state"
import { SessionDetailView } from "../session-detail-view"
import { TrajectoryConsole } from "./trajectory-console"

const SESSION_ID = "01JSESS0000000000000000001"

type Reply = Response | (() => Response | Promise<Response>)

/** Each request the console made, with its path, query and body. */
const requests: { method: string; path: string; query: URLSearchParams; body: string }[] = []
/** Replies by `METHOD path`, else the open questions, else an empty 204. */
let replies: Record<string, Reply[]> = {}
let listed: QuestionDto[] = []

beforeEach(() => {
  // The console retains its view per session: forget it between tests.
  for (const id of [SESSION_ID, "01JSESS0000000000000000002"]) {
    updateViewState(consoleViewKey("local", "http://127.0.0.1:7676", id), {
      draft: "",
      failed: [],
      scrollTop: 0,
      following: true,
      oldest: undefined,
      blockFolds: {},
    })
  }
  stubEventSource()
  requests.length = 0
  replies = {}
  listed = []
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    requests.push({
      method: request.method,
      path: url.pathname,
      query: url.searchParams,
      body: await request.text(),
    })
    const queue = replies[`${request.method} ${url.pathname}`]
    const reply = queue?.shift()
    if (reply) return typeof reply === "function" ? reply() : reply
    if (url.pathname.endsWith("/questions")) return Response.json(listed)
    return new Response(null, { status: 204 })
  })
})

afterEach(() => {
  vi.useRealTimers()
})

function reply(method: string, path: string, ...answers: Reply[]) {
  replies[`${method} /v1/sessions/${SESSION_ID}${path}`] = answers
}

function posts(path: string) {
  return requests.filter(
    (one) => one.method === "POST" && one.path === `/v1/sessions/${SESSION_ID}${path}`,
  )
}

function renderConsole(
  snapshot: TrajectorySnapshotDto,
  status: "running" | "idle" | "exited" = "idle",
) {
  const view = renderScreen(
    <TrajectoryConsole host="local" sessionId={SESSION_ID} status={status} />,
  )
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", snapshot)
  })
  return { ...view, source }
}

function transcript() {
  return screen.getByRole("log", { name: "Console transcript" })
}

function input() {
  return screen.getByRole("textbox", { name: "Message the agent" })
}

function activityLine(turn: TurnDto) {
  const article = transcript().querySelector<HTMLElement>(`[data-turn-id="${turn.id}"]`)
  if (!article) throw new Error(`turn ${turn.id} is not drawn`)
  // The activity line is the turn's first closed disclosure; the count of attention follows it.
  const line = within(article).getAllByRole("button", { expanded: false })[0]
  if (!line) throw new Error(`turn ${turn.id} has no closed activity line`)
  return line
}

it("shows the turn console in the session view, with no parameter", () => {
  const session = aSession({ id: SESSION_ID, status: "idle" })

  renderScreen(<SessionDetailView host="local" session={session} />, {
    route: `/sessions?session=${SESSION_ID}`,
  })

  expect(latestSource().url).toBe(
    `http://127.0.0.1:7676/v1/sessions/${SESSION_ID}/trajectory/stream`,
  )
})

it("keeps the draft and the refused prompts apart for each session", async () => {
  const other = "01JSESS0000000000000000002"
  const user = userEvent.setup()
  const view = renderScreen(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([aTurn()]))
  })
  await user.type(input(), "Only for session A")

  view.rerender(<TrajectoryConsole host="local" sessionId={other} status="idle" />)
  expect((input() as HTMLTextAreaElement).value).toBe("")

  view.rerender(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  expect((input() as HTMLTextAreaElement).value).toBe("Only for session A")
})

it("draws a closed turn as its prompt, one activity line and its answer", () => {
  const turn = aTurn()
  renderConsole(aSnapshot([turn]))

  const article = within(transcript()).getByRole("article")
  within(article).getByText("Fix the bug")
  within(article).getByText("Fixed it")
  const line = within(article).getByRole("button", { expanded: false })
  expect(line.textContent?.trim()).toBe("1m 04s·5 steps·2 files changed·1 failed")
  expect(within(article).getAllByRole("button")).toHaveLength(1)
})

it("opens the activity line to the step rows of the turn", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ earlier_steps: 2 })
  const tool = aToolStep(turn.id)
  const thought = {
    type: "thought" as const,
    id: "01THOUGHT00000000000000001",
    turn_id: turn.id,
    lane: "main",
    created_at: "2026-11-14T10:00:02Z",
    content: text("Look at the tests first"),
    streaming: false,
  }
  reply(
    "GET",
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [thought, tool], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([turn]))

  await user.click(activityLine(turn))

  const list = await within(transcript()).findByRole("list", { name: "Steps of the turn" })
  const rows = within(list).getAllByRole("button")
  expect(rows.map((row) => row.getAttribute("data-step-id"))).toEqual([thought.id, tool.id])
  within(list).getByRole("button", { name: "completed thought Look at the tests first" })
  within(list).getByRole("button", { name: "completed execute Run the tests npm test 12s" })
  expect(
    requests.some(
      (one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory/turns/${turn.id}/steps`,
    ),
  ).toBe(true)
})

it("shows the last steps of the running turn live, and streams its text", () => {
  const turn = aTurn({ state: "running", ended_at: null, answer: [] })
  const steps = [1, 2, 3, 4].map((n) => aToolStep(turn.id, { title: `Step ${n}`, subject: null }))
  const message = aMessageStep(turn.id, { content: text("Half") })
  const { source } = renderConsole(aSnapshot([{ ...turn, steps: [...steps, message] }]), "running")

  const live = within(transcript()).getByRole("list", { name: "Last steps of the turn" })
  expect(
    within(live)
      .getAllByRole("listitem")
      .map((row) => row.textContent?.trim()),
  ).toEqual(["Step 2 12s", "Step 3 12s", "Step 4 12s"])
  within(live).getByRole("button", { name: "completed execute Step 2 12s" })
  within(transcript()).getByText("Half")

  act(() =>
    source.emit(
      "update",
      anUpdate(2, [
        { op: "text_append", turn_id: turn.id, item_id: message.id, text: " done" },
        { op: "item_upsert", step: aToolStep(turn.id, { title: "Step 5", subject: null }) },
      ]),
    ),
  )

  within(transcript()).getByText("Half done")
  expect(within(live).getAllByRole("listitem").at(-1)?.textContent).toContain("Step 5")
})

it("draws a pending permission as a card above the composer, answered by its keys", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ state: "running", ended_at: null })
  const permission = aPermissionStep(turn.id)
  renderConsole(aSnapshot([{ ...turn, attention: [permission] }]), "running")

  const card = screen.getByRole("region", { name: "Pending permission" })
  within(card).getByText(/Permission · Bash/)
  within(card).getByText("rm -rf build")
  expect(within(transcript()).queryByText("rm -rf build")).toBeNull()
  const options = within(card).getAllByRole("option")
  expect(options.map((option) => option.textContent)).toEqual([
    "1.Allow once",
    "2.Allow always",
    "3.Reject",
  ])

  await user.click(input())
  await user.keyboard("{ArrowDown}")
  expect(within(card).getAllByRole("option")[1]?.getAttribute("aria-selected")).toBe("true")
  await user.keyboard("3")
  expect(within(card).getAllByRole("option")[2]?.getAttribute("aria-selected")).toBe("true")
  await user.keyboard("{ArrowUp}{Enter}")

  await waitFor(() => expect(posts("/console/input")).toHaveLength(1))
  expect(JSON.parse(posts("/console/input")[0]?.body ?? "")).toEqual({ text: "always" })
})

it("always shows an open question and each error, and counts the rest in one row", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ attention_more: 4 })
  const question = aQuestionStep(turn.id)
  const error = anErrorStep(turn.id)
  listed = [
    {
      id: question.request_id,
      session_id: SESSION_ID,
      created_at: question.created_at,
      message: "Which database?",
      schema: {
        type: "object",
        properties: { db: { type: "string", title: "Database", enum: ["pg", "sqlite"] } },
      } as never,
    },
  ]
  reply("GET", `/trajectory/turns/${turn.id}/steps`, Response.json({ steps: [], earlier_steps: 0 }))
  renderConsole(aSnapshot([{ ...turn, attention: [question, error] }]), "running")

  within(transcript()).getByText("The agent crashed")
  const card = await within(transcript()).findByRole("region", { name: "Agent question" })
  within(card).getByText("Which database?")
  await user.click(within(card).getByRole("button", { name: /sqlite/ }))
  await waitFor(() => expect(posts(`/questions/${question.request_id}/answer`)).toHaveLength(1))

  await user.click(within(transcript()).getByRole("button", { name: "4 more need attention" }))
  await waitFor(() =>
    expect(
      requests.some(
        (one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory/turns/${turn.id}/steps`,
      ),
    ).toBe(true),
  )
})

it("sends a prompt as queued, and drops the queued block once a turn holds it", async () => {
  const user = userEvent.setup()
  const { source } = renderConsole(aSnapshot([aTurn()]))

  await user.type(input(), "Add a test{Enter}")

  expect(JSON.parse(posts("/console/input")[0]?.body ?? "")).toEqual({ text: "Add a test" })
  within(transcript()).getByText("queued")
  act(() =>
    source.emit(
      "update",
      anUpdate(2, [
        {
          op: "turn_upsert",
          turn: aTurn({
            state: "running",
            ended_at: null,
            prompt: { id: "p", content: text("Add a test"), source: "console" },
          }),
        },
      ]),
    ),
  )
  expect(within(transcript()).queryByText("queued")).toBeNull()
  expect(within(transcript()).getAllByText("Add a test")).toHaveLength(1)
})

it("keeps a refused prompt with Retry and Edit", async () => {
  const user = userEvent.setup()
  const refused = () => errorResponse(409, "busy", "the agent is busy")
  reply("POST", "/console/input", refused, refused)
  renderConsole(aSnapshot([]))

  await user.type(input(), "Try this{Enter}")
  const failed = await within(transcript()).findByRole("alert")
  within(failed).getByText(/the agent is busy/)
  expect(within(transcript()).queryByText("queued")).toBeNull()

  expect(within(failed).getByRole("button", { name: "Retry" }).textContent).toBe("")
  expect(within(failed).getByRole("button", { name: "Edit" }).textContent).toBe("")
  await user.click(within(failed).getByRole("button", { name: "Retry" }))
  await waitFor(() => expect(posts("/console/input")).toHaveLength(2))
  expect(JSON.parse(posts("/console/input")[1]?.body ?? "")).toEqual({ text: "Try this" })

  await user.click(await within(transcript()).findByRole("button", { name: "Edit" }))
  expect(input()).toHaveProperty("value", "Try this")
  expect(document.activeElement).toBe(input())
})

it("cancels a running turn with Stop and with Escape", async () => {
  const user = userEvent.setup()
  renderConsole(aSnapshot([aTurn({ state: "running", ended_at: null })]), "running")

  await user.click(screen.getByRole("button", { name: "Stop the turn" }))
  await user.click(input())
  await user.keyboard("{Escape}")

  await waitFor(() => expect(posts("/console/cancel")).toHaveLength(2))
})

it("lists the agent's slash commands from the session state", async () => {
  const user = userEvent.setup()
  const snapshot = aSnapshot([])
  snapshot.session.commands = [
    { name: "review", description: "Review the branch" },
    { name: "plan", description: "Write a plan", hint: "goal" },
  ]
  renderConsole(snapshot)

  await user.type(input(), "/re")
  const list = screen.getByRole("listbox", { name: "Slash commands" })
  expect(
    within(list)
      .getAllByRole("option")
      .map((one) => one.textContent),
  ).toEqual(["/reviewReview the branch"])
  await user.keyboard("{Enter}")
  expect(input()).toHaveProperty("value", "/review ")
})

/** The scroll box of the turns, sized and placed by the test. */
function sizeLog(log: HTMLElement, state: { height: () => number; top: number }) {
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: state.height })
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
  Object.defineProperty(log, "scrollTop", {
    configurable: true,
    get: () => state.top,
    set: (value: number) => {
      state.top = value
    },
  })
}

it("loads older turns at the top and keeps the reader's place", async () => {
  // Made oldest first: each id sorts after the ones made before it.
  const older = aTurn({ answer: text("older answer") })
  const newest = aTurn({ answer: text("newest answer") })
  reply("GET", "/trajectory", Response.json(aSnapshot([older], { rev: 0, has_older: false })))
  renderConsole(aSnapshot([newest], { has_older: true }))
  const state = {
    height: () => (screen.queryByText("older answer") ? 1400 : 1000),
    top: 300,
  }
  sizeLog(transcript(), state)

  state.top = 0
  fireEvent.scroll(transcript())

  await within(transcript()).findByText("older answer")
  const read = requests.find((one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory`)
  expect(read?.query.get("before")).toBe(newest.id)
  expect(state.top).toBe(400)
  const turns = within(transcript()).getAllByRole("article")
  expect(turns.map((one) => one.dataset.turnId)).toEqual([older.id, newest.id])
})

it("reads older turns from the history it holds, not from an old turn the stream brought", async () => {
  // Made oldest first: each id sorts after the ones made before it.
  const old = aTurn({ answer: text("old answer") })
  const between = aTurn({ answer: text("between answer") })
  const newest = aTurn({ answer: text("newest answer") })
  reply("GET", "/trajectory", Response.json(aSnapshot([between], { rev: 0, has_older: true })))
  const { source } = renderConsole(aSnapshot([newest], { has_older: true }))
  const state = { height: () => 1000, top: 300 }
  sizeLog(transcript(), state)

  act(() => source.emit("update", anUpdate(2, [{ op: "turn_upsert", turn: old }])))
  expect(within(transcript()).queryByText("old answer")).toBeNull()

  state.top = 0
  fireEvent.scroll(transcript())

  await within(transcript()).findByText("between answer")
  const read = requests.find((one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory`)
  expect(read?.query.get("before")).toBe(newest.id)
  expect(within(transcript()).queryByText("old answer")).toBeNull()
})

it("follows the newest output, and offers Jump to latest once the reader scrolls away", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ state: "running", ended_at: null })
  const { source } = renderConsole(aSnapshot([turn]), "running")
  const state = { height: () => 1000, top: 0 }
  sizeLog(transcript(), state)

  act(() => source.emit("update", anUpdate(2, [{ op: "item_upsert", step: aToolStep(turn.id) }])))
  expect(state.top).toBe(1000)

  state.top = 100
  fireEvent.scroll(transcript())
  act(() => source.emit("update", anUpdate(3, [{ op: "item_upsert", step: aToolStep(turn.id) }])))
  expect(state.top).toBe(100)
  screen.getByText("New output")
  expect(screen.getByRole("button", { name: /Jump to latest/ }).textContent).toBe("New output")

  await user.click(screen.getByRole("button", { name: /Jump to latest/ }))
  expect(state.top).toBe(1000)
  expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull()
})

it("replaces the state on a reconnect and shows no item two times", () => {
  const turn = aTurn({ answer: text("only once") })
  const { source } = renderConsole(aSnapshot([turn]))
  vi.useFakeTimers()

  act(() => source.emit("resync", { missed: 3 }))
  screen.getByText(/reconnecting/)
  act(() => vi.advanceTimersByTime(500))
  expect(FakeEventSource.instances).toHaveLength(2)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([turn], { rev: 9 }))
  })

  expect(within(transcript()).getAllByText("only once")).toHaveLength(1)
  expect(within(transcript()).getAllByRole("article")).toHaveLength(1)
  screen.getByText("Live")
})

it("shows the session's status, the running turn and the token totals in the status row", () => {
  const snapshot = aSnapshot([aTurn({ state: "running", ended_at: null })])
  snapshot.session.status = "running"
  snapshot.session.usage = { input_tokens: 1200, output_tokens: 340, cached_input_tokens: 0 }
  renderConsole(snapshot, "running")

  const row = screen.getByRole("region", { name: "Session status" })
  within(row).getByText("running")
  within(row).getByText("thinking")
  within(row).getByText(/↑1\.2k ↓340/)
})

it("takes the state word and the busy state from the trajectory once its snapshot came", () => {
  // The session row says running; the trajectory says idle and names no turn.
  const snapshot = aSnapshot([aTurn()])
  snapshot.session.status = "idle"
  renderConsole(snapshot, "running")

  const row = screen.getByRole("region", { name: "Session status" })
  expect(row.textContent).toBe("idle")
  expect(within(row).queryByText("thinking")).toBeNull()
  expect(screen.queryByRole("button", { name: "Stop the turn" })).toBeNull()
})

it("shows a turn activity and Stop while running_turn_id names a turn, whatever the session row says", () => {
  const snapshot = aSnapshot([aTurn({ state: "running", ended_at: null })])
  snapshot.session.status = "running"
  renderConsole(snapshot, "idle")

  within(screen.getByRole("region", { name: "Session status" })).getByText("thinking")
  screen.getByRole("button", { name: "Stop the turn" })
})

it("shows no waiting-for-you activity for an open question while no turn runs", () => {
  const turn = aTurn()
  const snapshot = aSnapshot([{ ...turn, attention: [aQuestionStep(turn.id)] }])
  renderConsole(snapshot, "running")

  const row = screen.getByRole("region", { name: "Session status" })
  expect(within(row).queryByText("waiting for you")).toBeNull()
  expect(screen.queryByRole("button", { name: "Stop the turn" })).toBeNull()
})

it("moves the keyboard to the pane's Close button on ⌘Escape, and closes nothing", async () => {
  const user = userEvent.setup()
  const heard: string[] = []
  renderScreen(
    // biome-ignore lint/a11y/noStaticElementInteractions: the stand-in pane only listens
    <div data-slot="docked-pane" onKeyDown={(event) => heard.push(event.key)}>
      <button type="button" data-slot="pane-close">
        Close
      </button>
      <TrajectoryConsole host="local" sessionId={SESSION_ID} status="running" />
    </div>,
  )
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([aTurn({ state: "running", ended_at: null })]))
  })

  await user.click(input())
  await user.keyboard("{Meta>}{Escape}{/Meta}")

  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }))
  expect(heard).not.toContain("Escape")
  expect(posts("/console/cancel")).toHaveLength(0)
})

it("keeps the steps of a subagent out of its parent's last rows", () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const { source } = renderConsole(
    aSnapshot([{ ...turn, steps: [aToolStep(turn.id, { title: "Parent call" })] }]),
    "running",
  )

  act(() =>
    source.emit(
      "update",
      anUpdate(2, [
        { op: "item_upsert", step: aToolStep(turn.id, { lane: "01CHILD", title: "Child call" }) },
      ]),
    ),
  )

  const live = within(transcript()).getByRole("list", { name: "Last steps of the turn" })
  within(live).getByText("Parent call")
  expect(within(transcript()).queryByText("Child call")).toBeNull()
})

it("draws a pending permission only as its card, in the last rows and in the open fold", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ state: "running", ended_at: null })
  const permission = aPermissionStep(turn.id, { tool: { title: "Delete the build" } })
  reply(
    "GET",
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [permission], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([{ ...turn, steps: [permission], attention: [permission] }]), "running")

  screen.getByRole("region", { name: "Pending permission" })
  expect(within(transcript()).queryByText("Delete the build")).toBeNull()

  await user.click(activityLine(turn))
  await waitFor(() => expect(within(transcript()).queryByText("Loading the steps…")).toBeNull())
  expect(within(transcript()).queryByText("Delete the build")).toBeNull()
})

it("keeps the attention count as a disclosure once the fold is open", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ attention_more: 2 })
  reply("GET", `/trajectory/turns/${turn.id}/steps`, Response.json({ steps: [], earlier_steps: 0 }))
  renderConsole(aSnapshot([turn]))

  const more = within(transcript()).getByRole("button", { name: "2 more need attention" })
  expect(more.getAttribute("aria-expanded")).toBe("false")
  await user.click(more)

  const open = within(transcript()).getByRole("button", { name: "2 more need attention" })
  expect(open.getAttribute("aria-expanded")).toBe("true")
})

it("says a failed read of the questions, and reads them again on Retry", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ state: "running", ended_at: null })
  const question = aQuestionStep(turn.id)
  // The snapshot reads the questions, and so does the open question it brings.
  const busy = () => errorResponse(500, "internal", "store is busy")
  reply("GET", "/questions", busy, busy)
  listed = [
    {
      id: question.request_id,
      session_id: SESSION_ID,
      created_at: question.created_at,
      message: "Which database?",
      schema: {
        type: "object",
        properties: { db: { type: "string", title: "Database", enum: ["pg", "sqlite"] } },
      } as never,
    },
  ]
  renderConsole(aSnapshot([{ ...turn, attention: [question] }]), "running")

  const alert = await within(transcript()).findByText(/Could not read the open questions/)
  await user.click(within(alert).getByRole("button", { name: "Retry" }))

  const card = await within(transcript()).findByRole("region", { name: "Agent question" })
  within(card).getByRole("button", { name: /sqlite/ })
  expect(within(transcript()).queryByText(/Could not read the open questions/)).toBeNull()
})

it("loads older turns while the turns fit the box and no scroll can reach the top", async () => {
  // Made oldest first: each id sorts after the ones made before it.
  const older = aTurn({ answer: text("older answer") })
  const newest = aTurn({ answer: text("newest answer") })
  reply("GET", "/trajectory", Response.json(aSnapshot([older], { rev: 0, has_older: false })))
  const client = vi
    .spyOn(HTMLElement.prototype, "clientHeight", "get")
    .mockImplementation(function (this: HTMLElement) {
      return this.getAttribute("role") === "log" ? 200 : 0
    })
  try {
    renderConsole(aSnapshot([newest], { has_older: true }))

    await within(transcript()).findByText("older answer")
    const read = requests.find((one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory`)
    expect(read?.query.get("before")).toBe(newest.id)
  } finally {
    client.mockRestore()
  }
})

it("draws every part with the app's tokens, shared cards included, in both themes", async () => {
  const turn = aTurn({ state: "running", ended_at: null, attention_more: 1 })
  const question = aQuestionStep(turn.id)
  const permission = aPermissionStep(turn.id)
  listed = [
    {
      id: question.request_id,
      session_id: SESSION_ID,
      created_at: question.created_at,
      message: "Which database?",
      schema: {
        type: "object",
        properties: { db: { type: "string", title: "Database", enum: ["pg", "sqlite"] } },
      } as never,
    },
  ]
  reply("POST", "/console/input", errorResponse(409, "busy", "the agent is busy"))
  renderConsole(
    aSnapshot([{ ...turn, attention: [question, permission, anErrorStep(turn.id)] }]),
    "running",
  )
  await within(transcript()).findByRole("region", { name: "Agent question" })
  // The parts are drawn by hand: what the claim covers is their colours. The
  // rows of an open fold are this folder's own, which the source scan reads.
  fireEvent.change(input(), { target: { value: "x", selectionStart: 1 } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await within(transcript()).findByText(/the agent is busy/)

  const root = document.documentElement
  try {
    for (const dark of [false, true]) {
      root.classList.toggle("dark", dark)
      expect(drawnColours(document.body), dark ? "dark" : "light").toEqual([])
    }
  } finally {
    root.classList.remove("dark")
  }
})

/** Open the fold of a closed turn on the steps the steps route gives. */
async function openFold(turn: TurnDto, steps: StepDto[]) {
  const user = userEvent.setup()
  reply("GET", `/trajectory/turns/${turn.id}/steps`, Response.json({ steps, earlier_steps: 0 }))
  renderConsole(aSnapshot([turn]))
  await user.click(activityLine(turn))
  const list = await within(transcript()).findByRole("list", { name: "Steps of the turn" })
  await within(list).findAllByRole("listitem")
  return { user, list }
}

/** The shape of an icon, as its drawn paths. */
function shape(root: Element): string {
  return [...root.querySelectorAll("path, circle, rect, line, polyline")]
    .map((node) => node.outerHTML)
    .join("")
}

/** The rows of a list, its own items alone. */
function rowsOf(list: HTMLElement): HTMLElement[] {
  return within(list)
    .getAllByRole("listitem")
    .filter((item) => item.parentElement === list)
}

it("shows the state and the kind of a step row as icons, with their words as names and tooltips", async () => {
  const turn = aTurn()
  const tool = aToolStep(turn.id, { kind: "execute", status: "failed" })
  const thought = {
    type: "thought" as const,
    id: anId(),
    turn_id: turn.id,
    lane: "main",
    created_at: "2026-11-14T10:00:02Z",
    content: text("Look at the tests first"),
    streaming: false,
  }
  const { user, list } = await openFold(turn, [thought, tool])

  const [thinking, call] = rowsOf(list)
  // The words leave the row, and stay in its accessible name.
  expect(thinking?.textContent?.trim()).toBe("Look at the tests first")
  expect(call?.textContent?.trim()).toBe("Run the tests npm test 12s")
  within(list).getByRole("button", { name: "completed thought Look at the tests first" })
  const row = within(list).getByRole("button", {
    name: "failed execute Run the tests npm test 12s",
  })
  const kind = within(row).getByRole("img", { name: "execute" })
  within(row).getByRole("img", { name: "failed" })

  await user.hover(kind)
  expect(
    (await screen.findByText("execute", { selector: "[data-slot=tooltip-content]" })).textContent,
  ).toBe("execute")
})

it("shows a step state with the icon and the colour the board gives the same state", async () => {
  const turn = aTurn({ state: "running", ended_at: null })
  const states = ["running", "completed", "failed", "cancelled", "pending"] as const
  const steps = states.map((status) => aToolStep(turn.id, { title: status, status }))
  const user = userEvent.setup()
  reply("GET", `/trajectory/turns/${turn.id}/steps`, Response.json({ steps, earlier_steps: 0 }))
  renderConsole(aSnapshot([{ ...turn, steps }]), "running")
  await user.click(activityLine(turn))
  const list = await within(transcript()).findByRole("list", { name: "Steps of the turn" })
  await within(list).findAllByRole("listitem")

  // The board's icons and colours of in progress, finished, failed, cancelled and pending.
  const board = [
    [LoaderCircleIcon, "text-status-active"],
    [CircleCheckIcon, "text-status-done"],
    [CircleXIcon, "text-status-danger"],
    [BanIcon, "text-muted-foreground"],
    [ClockIcon, "text-status-pending"],
  ] as const
  for (const [at, status] of states.entries()) {
    const [Icon, tone] = board[at] ?? board[0]
    const icon = within(rowsOf(list)[at] as HTMLElement).getByRole("img", { name: status })
    expect(icon.className).toContain(tone)
    const { container } = render(<Icon />)
    expect(shape(icon)).toBe(shape(container))
  }
})

it("folds consecutive read and search steps into one group row of their subjects", async () => {
  const turn = aTurn()
  const read = aToolStep(turn.id, { kind: "read", title: "Read", subject: "src/a.ts" })
  const search = aToolStep(turn.id, { kind: "search", title: "Search", subject: "TODO" })
  const other = aToolStep(turn.id, { kind: "read", title: "Read", subject: "src/b.ts" })
  const edit = aToolStep(turn.id, { kind: "edit", title: "Edit", subject: "src/c.ts" })
  const alone = aToolStep(turn.id, { kind: "search", title: "Search", subject: "FIXME" })
  const { list } = await openFold(turn, [read, search, other, edit, alone])

  const rows = rowsOf(list)
  // A step of another kind ends the group, and a group of one step is a step row.
  expect(rows.map((row) => row.textContent?.trim())).toEqual([
    "src/a.ts, TODO, src/b.ts",
    "Edit src/c.ts 12s",
    "Search FIXME 12s",
  ])
  const group = within(rows[0] as HTMLElement).getByRole("button", { expanded: false })
  // A group of both kinds names itself; its subjects are cut to the row.
  within(group).getByRole("img", { name: "read and search" })
  expect(group.querySelector("[data-group-subjects]")?.className).toContain("truncate")
  expect(
    within(rows[2] as HTMLElement)
      .getByRole("button")
      .getAttribute("data-step-id"),
  ).toBe(alone.id)
})

it("gives a group of one kind the icon of that kind", async () => {
  const turn = aTurn()
  const steps = ["src/a.ts", "src/b.ts"].map((subject) =>
    aToolStep(turn.id, { kind: "read", title: "Read", subject }),
  )
  const { list } = await openFold(turn, steps)

  const group = within(list).getByRole("button", { expanded: false })
  within(group).getByRole("img", { name: "read" })
})

it("shows a group as running while a step runs, failed if a step failed, and completed otherwise", async () => {
  const turn = aTurn()
  const group = (statuses: ("running" | "failed" | "completed")[]) =>
    statuses.map((status) => aToolStep(turn.id, { kind: "read", subject: status, status }))
  const breaker = () => aToolStep(turn.id, { kind: "execute" })
  const { list } = await openFold(turn, [
    ...group(["failed", "running", "completed"]),
    breaker(),
    ...group(["completed", "failed"]),
    breaker(),
    ...group(["completed", "completed"]),
  ])

  const groups = [...list.querySelectorAll<HTMLElement>("[data-step-group]")]
  expect(groups).toHaveLength(3)
  for (const [at, state] of ["running", "failed", "completed"].entries())
    within(groups[at] as HTMLElement).getByRole("img", { name: state })
})

it("opens a group row to its steps, and each opens its detail", async () => {
  const turn = aTurn()
  const steps = ["src/a.ts", "src/b.ts"].map((subject) =>
    aToolStep(turn.id, { kind: "read", title: "Read", subject }),
  )
  const { user, list } = await openFold(turn, steps)

  const group = within(list).getByRole("button", { expanded: false })
  await user.click(group)
  expect(group.getAttribute("aria-expanded")).toBe("true")
  const inner = within(list).getByRole("list", { name: "Steps of the group" })
  const rows = within(inner).getAllByRole("button")
  expect(rows.map((row) => row.getAttribute("data-step-id"))).toEqual(steps.map((one) => one.id))

  await user.click(rows[0] as HTMLElement)
  expect(rows[0]?.getAttribute("aria-expanded")).toBe("true")
  expect(transcript().querySelector(`[data-step-detail="${steps[0]?.id}"]`)).not.toBeNull()
})

it("shows the subject of an edit row and the lines it added and removed", async () => {
  const turn = aTurn()
  const edit = aToolStep(turn.id, {
    kind: "edit",
    title: "Edit",
    subject: "src/main.rs",
    diff_stat: { files: 1, added: 12, removed: 3 },
  })
  const plain = aToolStep(turn.id, { kind: "edit", title: "Edit", subject: "src/lib.rs" })
  const { list } = await openFold(turn, [edit, plain])

  const [stat, none] = rowsOf(list)
  expect(stat?.textContent?.trim()).toBe("Edit src/main.rs +12 -3 12s")
  expect(within(stat as HTMLElement).getByText("+12").className).toContain("text-diff-add-fg")
  expect(within(stat as HTMLElement).getByText("-3").className).toContain("text-diff-remove-fg")
  expect(none?.textContent?.trim()).toBe("Edit src/lib.rs 12s")
})

it("shows the state of a turn line as an icon, with the time and the counts", () => {
  const done = aTurn()
  const failed = aTurn({ state: "failed" })
  renderConsole(aSnapshot([done, failed]))

  const line = activityLine(failed)
  // The state word leaves the line, and stays in its accessible name.
  expect(line.textContent?.trim()).toBe("1m 04s·5 steps·2 files changed·1 failed")
  within(line).getByRole("img", { name: "failed" })
  within(activityLine(done)).getByRole("img", { name: "completed" })
})

it("shows the author of each turn part as an icon, its word as the name", () => {
  const turn = aTurn()
  const daemon = aTurn({ prompt: { id: anId(), content: text("Resume"), source: "daemon" } })
  renderConsole(aSnapshot([turn, daemon]))

  for (const role of ["You", "Developer"]) {
    within(transcript()).getByRole("img", { name: role })
    expect(within(transcript()).queryByText(role)).toBeNull()
  }
  expect(within(transcript()).getAllByRole("img", { name: "Assistant" })).toHaveLength(2)
  expect(within(transcript()).queryByText("Assistant")).toBeNull()
})

const BRIEF_LINES = ["# Resume", "line two", "line three", "line four", "line five", "line six"]

function aDaemonTurn(body: string) {
  return aTurn({
    prompt: {
      id: anId(),
      content: text(`Seat brief\n\n${body}`),
      text: body,
      source: "daemon",
    },
  })
}

it("shows the duration, then the steps, then each count above zero", () => {
  const busy = aTurn({
    activity: {
      steps: 7,
      files_changed: 1,
      failed: 0,
      cancelled: 2,
      subagents: 1,
      thoughts: 0,
      tool_calls: 7,
    },
  })
  const quiet = aTurn({
    activity: {
      steps: 0,
      files_changed: 0,
      failed: 0,
      cancelled: 0,
      subagents: 0,
      thoughts: 0,
      tool_calls: 0,
    },
  })
  const plain = aTurn({
    activity: {
      steps: 3,
      files_changed: 0,
      failed: 0,
      cancelled: 0,
      subagents: 0,
      thoughts: 0,
      tool_calls: 3,
    },
  })
  renderConsole(aSnapshot([busy, quiet, plain]))

  expect(activityLine(busy).textContent?.trim()).toBe(
    "1m 04s·7 steps·1 file changed·2 cancelled·1 subagent",
  )
  expect(activityLine(quiet).textContent?.trim()).toBe("1m 04s")
  expect(activityLine(plain).textContent?.trim()).toBe("1m 04s·3 steps")
})

it("shows a daemon prompt without its seat brief and its untrusted marks", () => {
  const turn = aDaemonTurn("Fix <<<untrusted>>>the bug<<<end untrusted>>> now")
  renderConsole(aSnapshot([turn]))

  within(transcript()).getByText("Fix the bug now")
  expect(transcript().textContent).not.toContain("Seat brief")
  expect(transcript().textContent).not.toContain("untrusted")
})

it("folds a daemon prompt of more than 4 lines, and keeps its fold off the screen", async () => {
  const user = userEvent.setup()
  const turn = aDaemonTurn(BRIEF_LINES.join("\n"))
  const first = renderConsole(aSnapshot([turn]))

  expect(transcript().textContent).toContain("line four")
  expect(transcript().textContent).not.toContain("line five")
  await user.click(within(transcript()).getByRole("button", { name: "Show more" }))
  expect(transcript().querySelector("h1")?.textContent).toBe("Resume")
  expect(transcript().textContent).toContain("line six")

  // A change of tab unmounts the pane, and coming back mounts a new one.
  first.rerender(<div />)
  first.rerender(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([turn]))
  })
  expect(transcript().textContent).toContain("line six")
  await user.click(within(transcript()).getByRole("button", { name: "Show less" }))
  expect(transcript().textContent).not.toContain("line five")
})

it("shows a typed prompt whole, and a daemon prompt of 4 lines whole", () => {
  const typed = aTurn({
    prompt: { id: anId(), content: text("1\n2\n3\n4\n5\n6"), source: "console" },
  })
  const short = aDaemonTurn("a\nb\nc\nd")
  renderConsole(aSnapshot([typed, short]))

  expect(transcript().textContent).toContain("6")
  expect(within(transcript()).queryByRole("button", { name: "Show more" })).toBeNull()
})

it("shows the first line of a title, and a subject only when the title lacks it", async () => {
  const user = userEvent.setup()
  const turn = aTurn()
  const multi = aToolStep(turn.id, { title: "Run the tests\nwith more words", subject: "npm test" })
  const same = aToolStep(turn.id, { title: "Read src/main.rs", subject: "src/main.rs" })
  const later = aToolStep(turn.id, {
    title: "Run it\nnpm test -- login",
    subject: "npm test -- login",
  })
  // An answered permission whose call shows no row is a row of its own.
  const asked = aPermissionStep(turn.id, {
    state: "answered",
    answer: { option_id: "allow", label: "Allow once", decided_by: "console" },
    tool: { title: "Bash\nmore", subject: "Bash", kind: "execute" },
  })
  reply(
    "GET",
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [multi, same, later, asked], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([turn]))

  await user.click(activityLine(turn))

  const list = await within(transcript()).findByRole("list", { name: "Steps of the turn" })
  const rows = within(list).getAllByRole("listitem")
  expect(rows.map((row) => row.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "Run the tests npm test 12s",
    "Read src/main.rs 12s",
    "Run it 12s",
    "Bash",
  ])
})

it("does not show the message step that is the answer of an open turn", async () => {
  const user = userEvent.setup()
  const turn = aTurn()
  const message = aMessageStep(turn.id, { streaming: false, content: text("Fixed it") })
  const tool = aToolStep(turn.id)
  reply(
    "GET",
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [tool, message], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([{ ...turn, answer_step_id: message.id }]))

  await user.click(activityLine(turn))

  const list = await within(transcript()).findByRole("list", { name: "Steps of the turn" })
  expect(list.querySelector(`[data-step-id="${message.id}"]`)).toBeNull()
  expect(list.querySelector(`[data-step-id="${tool.id}"]`)).not.toBeNull()
})
