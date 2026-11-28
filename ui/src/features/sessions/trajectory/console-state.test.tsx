// @vitest-environment jsdom

/**
 * The live state of a session in the turn console, outside the activity
 * fold: the plan, the status row, the notices, the marks of answered
 * permissions, the title and find. And the view the console keeps for each
 * session through expand, collapse and a change of tab.
 */

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { StepDto, TrajectorySnapshotDto, TurnDto } from "@/api"
import { latestSource, stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, renderScreen } from "@/test/harness"
import {
  anErrorStep,
  anId,
  anUpdate,
  aPermissionStep,
  aSnapshot,
  aSubagentStep,
  aToolStep,
  aTurn,
  text,
} from "@/test/trajectory"

import { consoleViewKey, updateViewState } from "../console/console-view-state"
import { SessionDetailView } from "../session-detail-view"
import { TrajectoryConsole } from "./trajectory-console"

const SESSION_ID = "01JSESS0000000000000000101"

type Plan = TrajectorySnapshotDto["plans"][number]
type NoteStep = Extract<StepDto, { type: "note" }>
type CompactionStep = Extract<StepDto, { type: "compaction" }>

/** Each request the console made, with its path and query. */
const requests: { method: string; path: string; query: URLSearchParams }[] = []
/** Replies by `GET path`, else the steps of no turn, else the open questions, else an empty 204. */
let replies: Record<string, (Response | (() => Promise<Response>))[]> = {}

beforeEach(() => {
  updateViewState(consoleViewKey("local", "http://127.0.0.1:7676", SESSION_ID), {
    draft: "",
    failed: [],
    scrollTop: 0,
    following: true,
    oldest: undefined,
    blockFolds: {},
    turnEarlier: {},
  })
  stubEventSource()
  requests.length = 0
  replies = {}
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    requests.push({ method: request.method, path: url.pathname, query: url.searchParams })
    const reply = replies[`${request.method} ${url.pathname}`]?.shift()
    if (reply) return typeof reply === "function" ? reply() : reply
    if (url.pathname.endsWith("/steps")) return Response.json({ steps: [], earlier_steps: 0 })
    if (url.pathname.endsWith("/questions")) return Response.json([])
    return new Response(null, { status: 204 })
  })
})

function reply(path: string, ...answers: (Response | (() => Promise<Response>))[]) {
  replies[`GET /v1/sessions/${SESSION_ID}${path}`] = answers
}

function renderConsole(snapshot: TrajectorySnapshotDto, status: "running" | "idle" = "idle") {
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
  return screen.getByRole("textbox", { name: "Message the agent" }) as HTMLTextAreaElement
}

function article(turn: TurnDto) {
  const found = transcript().querySelector<HTMLElement>(`[data-turn-id="${turn.id}"]`)
  if (!found) throw new Error(`turn ${turn.id} is not drawn`)
  return found
}

/** The activity line of a turn: its first disclosure. */
function activityLine(turn: TurnDto) {
  const line = within(article(turn)).getAllByRole("button")[0]
  if (!line) throw new Error(`turn ${turn.id} has no activity line`)
  return line
}

function sizeLog(log: HTMLElement, state: { height: () => number; top: number }) {
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: state.height })
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
  Object.defineProperty(log, "scrollTop", {
    configurable: true,
    get: () => state.top,
    // A browser clamps the offset to the scroll range.
    set: (value: number) => {
      state.top = Math.max(0, Math.min(value, state.height() - 200))
    },
  })
}

function aPlan(turnId: string, statuses: string[], overrides: Partial<Plan> = {}): Plan {
  return {
    plan_id: anId(),
    variant: "entries",
    entries: statuses.map((status, index) => ({
      content: ["Read the code", "Write the test", "Make it pass"][index] ?? `Entry ${index}`,
      status,
    })),
    turn_id: turnId,
    removed: false,
    updated_at: "2026-11-14T10:00:30Z",
    ...overrides,
  }
}

function aNote(turnId: string, severity: "info" | "warning", value: string): NoteStep {
  return {
    type: "note",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:10Z",
    origin: "notice",
    severity,
    text: value,
  }
}

function aCompaction(turnId: string): CompactionStep {
  return {
    type: "compaction",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:12Z",
    compaction_id: "c1",
    status: "completed",
    summary: text("The summary"),
  }
}

it("shows one plan line above the composer while a plan has an open entry, and a finished plan in its turn", async () => {
  const user = userEvent.setup()
  const turn = aTurn()
  const plan = aPlan(turn.id, ["completed", "in_progress", "pending"])
  const { source } = renderConsole(aSnapshot([turn], { plans: [plan] }))

  const dock = screen.getByRole("region", { name: "Console dock" })
  const line = within(dock).getByRole("button", { expanded: false })
  expect(line.textContent).toBe("Plan 1/3·Write the test")
  expect(within(article(turn)).queryByRole("region", { name: "Plan" })).toBeNull()

  await user.click(line)
  const entries = within(dock).getByRole("list", { name: "Plan entries" })
  expect(
    within(entries)
      .getAllByRole("listitem")
      .map((one) => one.textContent),
  ).toEqual(["✓Read the code", "●Write the test", "○Make it pass"])

  act(() =>
    source.emit(
      "update",
      anUpdate(2, [
        {
          op: "plan_upsert",
          plan: aPlan(turn.id, ["completed", "completed", "completed"], {
            plan_id: plan.plan_id,
          }),
        },
      ]),
    ),
  )
  expect(screen.queryByRole("region", { name: "Console dock" })).toBeNull()
  const finished = within(article(turn)).getByRole("region", { name: "Plan" })
  expect(within(finished).getByRole("button").textContent).toBe("Plan 3/3·done")
})

it("shows the status, the tokens, the context use, the model, the mode, the options and the cost in rank order", () => {
  const snapshot = aSnapshot([aTurn()])
  snapshot.session = {
    ...snapshot.session,
    mode: {
      current_id: "code",
      available: [
        { id: "ask", name: "Ask" },
        { id: "code", name: "Code" },
      ],
    },
    config_options: [
      {
        id: "effort",
        name: "Effort",
        category: "thought_level",
        current_value: "high",
        values: [{ value: "high", name: "High" }],
      },
      {
        id: "mode",
        name: "Mode",
        category: "mode",
        current_value: "code",
        values: [{ value: "code", name: "Code" }],
      },
      {
        id: "model",
        name: "Model",
        category: "model",
        current_value: "opus",
        values: [
          { value: "opus", name: "Opus 5.5" },
          { value: "sonnet", name: "Sonnet 5.5" },
        ],
      },
    ],
    context: { used: 50_000, size: 200_000 },
    cost: { amount: 0.42, currency: "USD" },
    usage: { input_tokens: 1200, output_tokens: 340, cached_input_tokens: 0 },
  }
  renderConsole(snapshot)

  const row = screen.getByRole("region", { name: "Session status" })
  expect(row.textContent).toBe("idle↑1.2k ↓340context 25%Model Opus 5.5mode CodeEffort High$0.42")
  // jsdom lays nothing out: the classes keep the row to one line of whole facts,
  // and the browser proof of spec 015 rule 99b measures them.
  const line = row.firstElementChild as HTMLElement
  expect(line.className).toContain("whitespace-nowrap")
  expect(line.className).toContain("overflow-hidden")
  expect(line.className).toContain("flex-wrap")
  expect(line.className).toContain("h-4")
})

it("shows the mode one time when the mode state and a mode option both name it", () => {
  const snapshot = aSnapshot([aTurn()])
  snapshot.session = {
    ...snapshot.session,
    mode: { current_id: "code", available: [{ id: "code", name: "Code" }] },
    config_options: [
      {
        id: "mode",
        name: "Mode",
        category: "mode",
        current_value: "code",
        values: [{ value: "code", name: "Code" }],
      },
    ],
  }
  renderConsole(snapshot)

  const row = screen.getByRole("region", { name: "Session status" })
  expect(row.textContent?.match(/mode/gi)).toHaveLength(1)
})

it("counts no running subagent in the status row, and draws no bar of views for one", () => {
  const turn = aTurn({ state: "running", ended_at: null, answer: [] })
  const child = aSubagentStep(turn.id)
  renderConsole(aSnapshot([{ ...turn, steps: [child] }], { subagents: [child] }), "running")

  const row = screen.getByRole("region", { name: "Session status" })
  expect(row.textContent).not.toMatch(/subagent/)
  expect(screen.queryByRole("tablist", { name: "Console views" })).toBeNull()
  expect(within(article(turn)).getByText("Explore the tests")).toBeTruthy()
})

it("always shows a warning, an error and a compaction in their turn, and an info notice in the fold alone", async () => {
  const user = userEvent.setup()
  const turn = aTurn({ state: "running", ended_at: null, answer: [] })
  const info = aNote(turn.id, "info", "Session started")
  const warning = aNote(turn.id, "warning", "Rate limited · retrying")
  const compaction = aCompaction(turn.id)
  const error = anErrorStep(turn.id)
  const steps = [info, warning, compaction]
  reply(`/trajectory/turns/${turn.id}/steps`, Response.json({ steps, earlier_steps: 0 }))
  renderConsole(
    aSnapshot([{ ...turn, steps, notices: [warning, compaction], attention: [error] }]),
    "running",
  )

  const drawn = article(turn)
  within(drawn).getByText("Rate limited · retrying")
  within(drawn).getByText("Compacted the context")
  within(drawn).getByText("The agent crashed")
  expect(within(drawn).queryByText("Session started")).toBeNull()

  await user.click(activityLine(turn))

  const rows = await within(drawn).findByRole("list", { name: "Steps of the turn" })
  await waitFor(() => within(rows).getByText("Session started"))
  expect(within(drawn).getAllByText("Rate limited · retrying")).toHaveLength(1)
  expect(within(drawn).getAllByText("Compacted the context")).toHaveLength(1)
})

it("marks an answered permission on the row of its call, with who decided", async () => {
  const user = userEvent.setup()
  const turn = aTurn()
  const allowed = aPermissionStep(turn.id, {
    state: "answered",
    answer: { option_id: "allow", label: "Allow once", decided_by: "ai" },
  })
  const rejected = aPermissionStep(turn.id, {
    state: "answered",
    answer: { option_id: "reject", label: "Reject", decided_by: "console" },
  })
  const first = aToolStep(turn.id, {
    title: "Remove the build",
    subject: null,
    tool_call_id: allowed.tool_call_id,
    permission: allowed.id,
  })
  const second = aToolStep(turn.id, {
    title: "Push the branch",
    subject: null,
    tool_call_id: rejected.tool_call_id,
  })
  reply(
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [allowed, first, rejected, second], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([turn]))

  await user.click(activityLine(turn))

  const rows = await within(article(turn)).findAllByRole("listitem")
  expect(rows.map((row) => row.textContent?.trim())).toEqual([
    "Remove the build ✓ Allow once · AI 12s",
    "Push the branch ✗ Reject · you 12s",
  ])
})

it("shows the title that the agent gives as the subtitle of the console", () => {
  const snapshot = aSnapshot([aTurn()])
  snapshot.session = { ...snapshot.session, title: "Fix the login bug" }
  const { source } = renderConsole(snapshot)

  expect(screen.getByLabelText("Session title").textContent).toBe("Fix the login bug")

  act(() =>
    source.emit(
      "update",
      anUpdate(2, [
        { op: "session_patch", session: { ...snapshot.session, title: "Fix the logout bug" } },
      ]),
    ),
  )
  expect(screen.getByLabelText("Session title").textContent).toBe("Fix the logout bug")
})

it("finds the text of the loaded turns and the loaded steps", async () => {
  const user = userEvent.setup()
  const older = aTurn({ answer: text("Fixed it first") })
  const turn = aTurn()
  const tool = aToolStep(turn.id, { subject: "npm test -- login" })
  let release: () => void = () => {}
  const late = new Promise<void>((resolve) => {
    release = resolve
  })
  reply(`/trajectory/turns/${turn.id}/steps`, () =>
    late.then(() => Response.json({ steps: [tool], earlier_steps: 0 })),
  )
  renderConsole(aSnapshot([older, turn], { has_older: true }))

  await user.click(screen.getByRole("button", { name: "Find in transcript" }))
  const field = screen.getByRole("textbox", { name: "Find in transcript" })
  const find = field.closest("search") as HTMLElement
  await user.type(field, "fixed")
  expect(within(find).getByRole("status").textContent).toBe("1 of 2")
  within(find).getByText("Older history remains unloaded.", { exact: false })

  // A step is found once the console loaded it, and not before: an open fold loads it.
  fireEvent.change(field, { target: { value: "-- login" } })
  expect(within(find).getByRole("status").textContent).toBe("No matches in loaded messages")
  fireEvent.click(activityLine(turn))
  expect(within(find).getByRole("status").textContent).toBe("No matches in loaded messages")
  await act(async () => release())
  await waitFor(() => expect(within(find).getByRole("status").textContent).toBe("1 of 1"))
})

/**
 * A session view with the turn console, a draft, an open fold and a place
 * away from the bottom, and the check that a new pane keeps all three.
 */
async function keptView(mount: "console" | "session view") {
  const turn = aTurn()
  const snapshot = { ...aSnapshot([turn]), rev: 0 }
  if (mount === "console")
    renderScreen(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  else
    renderScreen(
      <SessionDetailView host="local" session={aSession({ id: SESSION_ID, status: "idle" })} />,
      { route: `/sessions?session=${SESSION_ID}` },
    )
  /** Open the stream of the pane on screen, with a box of its own. */
  const show = () => {
    const state = { height: () => 1000, top: 0 }
    sizeLog(transcript(), state)
    act(() => {
      latestSource().succeed()
      latestSource().emit("snapshot", snapshot)
    })
    return state
  }
  show()
  fireEvent.change(input(), { target: { value: "Half a thought", selectionStart: 14 } })
  fireEvent.click(activityLine(turn))
  const place = { height: () => 1000, top: 300 }
  sizeLog(transcript(), place)
  fireEvent.scroll(transcript())

  return async () => {
    const state = show()
    expect(input().value).toBe("Half a thought")
    expect(activityLine(turn).getAttribute("aria-expanded")).toBe("true")
    await waitFor(() => expect(state.top).toBe(300))
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).not.toBeNull()
  }
}

it("keeps the draft, the scroll place and the open folds through expand and collapse", async () => {
  const user = userEvent.setup()
  const kept = await keptView("console")

  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await screen.findByRole("dialog", { name: "Session console" })
  await kept()

  await user.click(screen.getByRole("button", { name: "Collapse the console back into the panel" }))
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  await kept()
})

it("keeps the draft, the scroll place and the open folds through a change of tab", async () => {
  const user = userEvent.setup()
  const kept = await keptView("session view")

  await user.click(screen.getByRole("tab", { name: "Agent activity" }))
  await user.click(screen.getByRole("tab", { name: "Console" }))
  await kept()
})

it("puts the place back once the open folds read their rows, not before", async () => {
  const turn = aTurn()
  const tool = aToolStep(turn.id)
  const older = aToolStep(turn.id, { title: "Read the older rows" })
  const page = () => Response.json({ steps: [tool], earlier_steps: 1 })
  const earlier = () => Response.json({ steps: [older], earlier_steps: 0 })
  let release: () => void = () => {}
  const late = new Promise<void>((resolve) => {
    release = resolve
  })
  reply(`/trajectory/turns/${turn.id}/steps`, page(), earlier(), page(), () => late.then(earlier))
  const view = renderConsole(aSnapshot([turn]))
  fireEvent.click(activityLine(turn))
  await within(article(turn)).findByText("Run the tests")
  fireEvent.click(within(article(turn)).getByRole("button", { name: "Show 1 earlier" }))
  await within(article(turn)).findByText("Read the older rows")
  // The open rows make the box tall enough for the place.
  const rows = () => within(transcript()).queryByText("Read the older rows") !== null
  const first = { height: () => (rows() ? 1000 : 300), top: 600 }
  sizeLog(transcript(), first)
  fireEvent.scroll(transcript())

  view.rerender(<div />)
  view.rerender(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  const again = { height: () => (rows() ? 1000 : 300), top: 0 }
  sizeLog(transcript(), again)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([turn]))
  })
  await within(article(turn)).findByText("Run the tests")
  expect(again.top).toBe(0)

  await act(async () => release())
  await within(article(turn)).findByText("Read the older rows")
  await waitFor(() => expect(again.top).toBe(600))
})

it("shows the warnings and the compaction of a closed turn from its notices, and reads no steps for them", async () => {
  const turn = aTurn()
  const warning = aNote(turn.id, "warning", "Rate limited · retrying")
  const compaction = aCompaction(turn.id)
  // A closed turn of a snapshot carries no steps (040, rule 13), only its notices.
  renderConsole(aSnapshot([{ ...turn, steps: [], notices: [warning, compaction] }]))

  const drawn = article(turn)
  within(drawn).getByText("Rate limited · retrying")
  within(drawn).getByText("Compacted the context")
  expect(activityLine(turn).getAttribute("aria-expanded")).toBe("false")
  expect(requests.filter((one) => one.path.endsWith(`/turns/${turn.id}/steps`))).toEqual([])

  fireEvent.click(screen.getByRole("button", { name: "Find in transcript" }))
  const field = screen.getByRole("textbox", { name: "Find in transcript" })
  const find = within(field.closest("search") as HTMLElement)
  fireEvent.change(field, { target: { value: "Rate limited" } })
  expect(find.getByRole("status").textContent).toBe("1 of 1")
})

it("counts the notices of a turn that its notices do not hold in one row, which opens the steps of the turn", async () => {
  const user = userEvent.setup()
  const turn = aTurn()
  const warning = aNote(turn.id, "warning", "Rate limited · retrying")
  const older = aNote(turn.id, "warning", "Pin fell back to sonnet")
  reply(
    `/trajectory/turns/${turn.id}/steps`,
    Response.json({ steps: [older, warning], earlier_steps: 0 }),
  )
  renderConsole(aSnapshot([{ ...turn, notices: [warning], notices_more: 2 }]))

  const drawn = article(turn)
  expect(requests.filter((one) => one.path.endsWith(`/turns/${turn.id}/steps`))).toEqual([])
  await user.click(within(drawn).getByRole("button", { name: "2 more notices" }))

  expect(activityLine(turn).getAttribute("aria-expanded")).toBe("true")
  await within(drawn).findByRole("list", { name: "Steps of the turn" })
  // The notice the turn's own list left out shows once its steps are read, once.
  await within(drawn).findByText("Pin fell back to sonnet")
  expect(within(drawn).getAllByText("Rate limited · retrying")).toHaveLength(1)
  expect(requests.filter((one) => one.path.endsWith(`/turns/${turn.id}/steps`))).toHaveLength(1)
})

it("loads back to the oldest turn it had loaded before it puts the reader's place back", async () => {
  const older = aTurn({ answer: text("older answer") })
  const newest = aTurn({ answer: text("newest answer") })
  const page = aSnapshot([older], { rev: 0, has_older: false })
  reply("/trajectory", Response.json(page), Response.json(page))
  const view = renderConsole(aSnapshot([newest], { has_older: true }))
  const first = {
    height: () => (screen.queryByText("older answer") ? 1400 : 1000),
    top: 300,
  }
  sizeLog(transcript(), first)
  first.top = 0
  fireEvent.scroll(transcript())
  await within(transcript()).findByText("older answer")
  first.top = 250
  fireEvent.scroll(transcript())
  // A change of tab unmounts the pane, and coming back mounts a new one.
  view.rerender(<div />)
  view.rerender(<TrajectoryConsole host="local" sessionId={SESSION_ID} status="idle" />)
  const again = {
    height: () => (screen.queryByText("older answer") ? 1400 : 1000),
    top: 0,
  }
  sizeLog(transcript(), again)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", aSnapshot([newest], { has_older: true }))
  })

  await within(transcript()).findByText("older answer")
  const reads = requests.filter((one) => one.path === `/v1/sessions/${SESSION_ID}/trajectory`)
  expect(reads.map((one) => one.query.get("before"))).toEqual([newest.id, newest.id])
  await waitFor(() => expect(again.top).toBe(250))
})
