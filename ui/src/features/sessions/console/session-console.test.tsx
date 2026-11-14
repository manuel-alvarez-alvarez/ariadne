// @vitest-environment jsdom

/**
 * The session console, driven from the daemon's side: the console stream on
 * an `EventSource` stand-in, and the input and cancel routes on the fetch stub.
 */

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { AgentEventDto } from "@/api"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"

import { SessionConsole } from "./session-console"

const SESSION_ID = "01JSESS0000000000000000001"
const posts: { path: string; body: string }[] = []

let next = 0
function event(kind: string, payload: unknown = {}): AgentEventDto {
  next += 1
  return {
    id: `01EVENT${String(next).padStart(19, "0")}`,
    kind,
    payload,
    summary: `${kind} summary`,
    created_at: "2026-11-14T10:00:00Z",
  }
}

/** The session's pending requests, as `GET .../agent-requests` lists them with each snapshot. */
let pendingList: unknown[] = []
/** How many reads of the pending requests fail before one succeeds. */
let pendingFailures = 0

/** The read of the pending requests every snapshot makes, answered apart from the routes under test. */
function pendingRead(request: Request): Response | undefined {
  if (request.method !== "GET" || !new URL(request.url).pathname.endsWith("/agent-requests"))
    return undefined
  if (pendingFailures > 0) {
    pendingFailures -= 1
    return errorResponse(503, "unavailable", "the daemon is busy")
  }
  return Response.json(pendingList)
}

function answer(reply: () => Response = () => new Response(null, { status: 204 })) {
  posts.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const listed = pendingRead(request)
    if (listed) return listed
    posts.push({ path: new URL(request.url).pathname, body: await request.text() })
    return reply()
  })
}

beforeEach(() => {
  pendingList = []
  pendingFailures = 0
  stubEventSource()
  answer()
})

afterEach(() => {
  vi.useRealTimers()
})

/** The console, connected, with the snapshot the daemon opened on. */
function renderConsole(
  snapshot: AgentEventDto[] = [],
  status: "running" | "idle" | "exited" = "idle",
) {
  const view = renderScreen(<SessionConsole sessionId={SESSION_ID} status={status} />)
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

it("reads the console stream and draws the snapshot as blocks", () => {
  renderConsole([
    event("user_prompt_submit", { text: "Fix the bug" }),
    event("agent_message", { text: "**Fixed** it" }),
  ])

  expect(latestSource().url).toBe(`http://127.0.0.1:7676/v1/sessions/${SESSION_ID}/console/stream`)
  within(transcript()).getByText("Fix the bug")
  expect(within(transcript()).getByText("Fixed").tagName).toBe("STRONG")
})

it("marks a prompt the daemon sent", () => {
  renderConsole([event("user_prompt_submit", { text: "Resume", source: "daemon" })])

  within(transcript()).getByText("daemon")
})

it("extends the transcript with each event and replaces it with a new snapshot", () => {
  const { source } = renderConsole([event("user_prompt_submit", { text: "one" })])

  act(() => source.emit("event", event("agent_message", { text: "two" })))
  within(transcript()).getByText("two")

  act(() => source.emit("snapshot", [event("user_prompt_submit", { text: "three" })]))
  expect(within(transcript()).queryByText("one")).toBeNull()
  within(transcript()).getByText("three")
})

it("says it reconnects after a resync and takes the fresh snapshot without duplicates", () => {
  const prompt = event("user_prompt_submit", { text: "only once" })
  const { source } = renderConsole([prompt])
  vi.useFakeTimers()

  act(() => source.emit("resync", { missed: 2 }))
  screen.getByText(/reconnecting/)

  act(() => vi.advanceTimersByTime(500))
  expect(FakeEventSource.instances).toHaveLength(2)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [prompt])
  })

  expect(within(transcript()).getAllByText("only once")).toHaveLength(1)
  screen.getByText("Live")
})

it("closes the stream when it leaves the screen", () => {
  const { source, rerender } = renderConsole()
  expect(source.closed).toBe(false)

  rerender(<span />)

  expect(source.closed).toBe(true)
})

it("sends a prompt on Enter, and Shift+Enter adds a line", async () => {
  const user = userEvent.setup()
  renderConsole()

  await user.type(input(), "first{Shift>}{Enter}{/Shift}second{Enter}")

  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]).toEqual({
    path: `/v1/sessions/${SESSION_ID}/console/input`,
    body: JSON.stringify({ text: "first\nsecond" }),
  })
  expect(input()).toHaveProperty("value", "")
})

it("adds an error block when the daemon refuses the prompt", async () => {
  answer(() => errorResponse(409, "session_not_live", "the session is not live"))
  renderConsole()

  fireEvent.change(input(), { target: { value: "hello" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await within(transcript()).findByText(/the session is not live/)
})

it("disables the input with a reason when the session is not live", () => {
  renderConsole([], "exited")

  expect(input()).toHaveProperty("disabled", true)
  screen.getByText("This session has ended. Resume it to talk to it.")
})

it("cancels a running turn from its control and from Escape inside the console", async () => {
  const user = userEvent.setup()
  renderConsole([event("user_prompt_submit", { text: "go" })], "running")

  await user.click(screen.getByRole("button", { name: "Cancel the turn" }))
  await waitFor(() => expect(posts).toHaveLength(1))
  fireEvent.keyDown(input(), { key: "Escape" })
  await waitFor(() => expect(posts).toHaveLength(2))

  for (const post of posts) {
    expect(post).toEqual({ path: `/v1/sessions/${SESSION_ID}/console/cancel`, body: "" })
  }
})

it("answers a permission from its picker with the keyboard, and shows who decided", async () => {
  const request = event("permission_request", {
    acp: { toolCallId: "call-1", title: "Bash", rawInput: { command: "rm -rf build" } },
    options: [
      { optionId: "allow", name: "Allow", kind: "allow_once" },
      { optionId: "always", name: "Allow always", kind: "allow_always" },
      { optionId: "reject", name: "Reject", kind: "reject_once" },
    ],
  })
  const { source } = renderConsole([request], "running")
  within(transcript()).getByText("rm -rf build")
  const picker = screen.getByRole("listbox", { name: "Answer the permission" })

  fireEvent.keyDown(picker, { key: "ArrowDown" })
  fireEvent.keyDown(picker, { key: "ArrowDown" })
  fireEvent.keyDown(picker, { key: "ArrowUp" })
  expect(within(picker).getByRole("option", { name: /Allow always/ })).toHaveProperty(
    "ariaSelected",
    "true",
  )
  fireEvent.keyDown(picker, { key: "3" })
  fireEvent.keyDown(picker, { key: "Enter" })

  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]).toEqual({
    path: `/v1/sessions/${SESSION_ID}/console/input`,
    body: JSON.stringify({ text: "reject" }),
  })

  act(() =>
    source.emit(
      "event",
      event("permission.replied", { option_id: "reject", decided_by: "console" }),
    ),
  )
  expect(screen.queryByRole("listbox", { name: "Answer the permission" })).toBeNull()
  within(transcript()).getByText("Reject · decided by console")
})

it.each([
  ["Shift+Enter", { shiftKey: true }],
  ["a composing Enter", { isComposing: true }],
] as const)("answers no permission on %s from an empty input", (_label, init) => {
  renderConsole([
    event("permission_request", {
      tool_name: "Edit",
      options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
    }),
  ])

  fireEvent.keyDown(input(), { key: "Enter", ...init })

  // An answer marks its option as being sent at once, before its POST.
  expect(screen.getByRole("option", { name: /Allow/ }).getAttribute("aria-disabled")).toBe("false")
  expect(posts).toHaveLength(0)
})

it("picks a permission option with a click", async () => {
  const user = userEvent.setup()
  renderConsole([
    event("permission_request", {
      tool_name: "Edit",
      options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
    }),
  ])

  await user.click(screen.getByRole("option", { name: /Allow/ }))

  await waitFor(() => expect(posts[0]?.body).toBe(JSON.stringify({ text: "allow" })))
})

it("expands into a near-fullscreen modal and collapses back on a fresh stream", async () => {
  const user = userEvent.setup()
  const { source } = renderConsole()

  await user.click(screen.getByRole("button", { name: "Expand the console" }))

  const dialog = await screen.findByRole("dialog", { name: "Session console" })
  expect(dialog.className).toContain("h-[calc(100dvh-2rem)]")
  expect(source.closed).toBe(true)
  expect(FakeEventSource.instances).toHaveLength(2)

  await user.click(screen.getByRole("button", { name: "Collapse the console back into the panel" }))

  expect(screen.queryByRole("dialog", { name: "Session console" })).toBeNull()
  expect(FakeEventSource.instances).toHaveLength(3)
})

it("keeps the modal open on Escape inside the console, and closes it on Escape outside", async () => {
  const user = userEvent.setup()
  renderConsole()
  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  const dialog = await screen.findByRole("dialog", { name: "Session console" })

  fireEvent.keyDown(input(), { key: "Escape" })
  expect(screen.getByRole("dialog", { name: "Session console" })).not.toBeNull()

  fireEvent.keyDown(dialog, { key: "Escape" })
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Session console" })).toBeNull())
})

it("moves focus to Collapse on ⌘Escape and cancels nothing", async () => {
  const user = userEvent.setup()
  renderConsole([event("user_prompt_submit", { text: "go" })], "running")
  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await screen.findByRole("dialog", { name: "Session console" })
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [event("user_prompt_submit", { text: "go" })])
  })

  fireEvent.keyDown(input(), { key: "Escape", metaKey: true })

  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Collapse the console back into the panel" }),
  )
  expect(posts).toHaveLength(0)
  expect(screen.getByRole("dialog", { name: "Session console" })).not.toBeNull()
})

it("gives the input the keyboard when asked to on arrival", () => {
  renderScreen(<SessionConsole sessionId={SESSION_ID} status="idle" autoFocus />)

  expect(document.activeElement).toBe(input())
})

it("follows the newest block while the reader is at the bottom", () => {
  const { source } = renderConsole([event("agent_message", { text: "one" })])
  const log = transcript()
  let top = 0
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => 1000 })
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
  Object.defineProperty(log, "scrollTop", {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = value
    },
  })

  act(() => source.emit("event", event("agent_message", { text: "two" })))
  expect(top).toBe(1000)

  // Scrolled up to read: a new block leaves the reader where they are.
  top = 100
  fireEvent.scroll(log)
  act(() => source.emit("event", event("agent_message", { text: "three" })))
  expect(top).toBe(100)
})

/** The scroll box of the transcript, sized and positioned by the test. */
function sizeLog(log: HTMLElement, state: { height: number; top: number }) {
  Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => state.height })
  Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
  Object.defineProperty(log, "scrollTop", {
    configurable: true,
    get: () => state.top,
    set: (value: number) => {
      state.top = value
    },
  })
}

const historyUrls: URL[] = []

/** Answer `GET /v1/events` with each page in turn, or with an error where one is an Error. */
function pages(...replies: (AgentEventDto[] | Error)[]) {
  historyUrls.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const listed = pendingRead(request)
    if (listed) return listed
    historyUrls.push(new URL(request.url))
    const reply = replies.shift() ?? []
    if (reply instanceof Error) return errorResponse(500, "internal", reply.message)
    return Response.json(reply)
  })
}

function scrollToTop(log: HTMLElement, state: { top: number }) {
  state.top = 0
  fireEvent.scroll(log)
}

it("loads one older page when scrolled to the top, and keeps the reader's place", async () => {
  const older = [event("user_prompt_submit", { text: "older prompt" })]
  const first = event("agent_message", { text: "newest" })
  renderConsole([first])
  const state = { height: 1000, top: 300 }
  sizeLog(transcript(), state)
  // The page makes the box 400px taller once it is drawn.
  Object.defineProperty(transcript(), "scrollHeight", {
    configurable: true,
    get: () => (screen.queryByText("older prompt") ? 1400 : 1000),
  })
  pages([...older].reverse())

  scrollToTop(transcript(), state)

  await within(transcript()).findByText("older prompt")
  expect(historyUrls).toHaveLength(1)
  const query = historyUrls[0]?.searchParams
  expect(historyUrls[0]?.pathname).toBe("/v1/events")
  expect(Object.fromEntries(query ?? [])).toEqual({
    session: SESSION_ID,
    order: "desc",
    before: first.id,
    limit: "200",
  })
  expect(state.top).toBe(400)
  expect(screen.queryByRole("log")?.textContent).toContain("newest")
})

it("shows a loading state, then an error with a retry that loads the page", async () => {
  const user = userEvent.setup()
  renderConsole([event("agent_message", { text: "newest" })])
  const state = { height: 1000, top: 0 }
  sizeLog(transcript(), state)
  pages(new Error("boom"), [event("user_prompt_submit", { text: "older prompt" })])

  scrollToTop(transcript(), state)
  screen.getByText("Loading older events…")
  await screen.findByText(/Could not load older events/)

  await user.click(screen.getByRole("button", { name: "Retry" }))
  await within(transcript()).findByText("older prompt")
  expect(historyUrls).toHaveLength(2)
})

it("stops fetching once an empty page marks the start, until a reconnect", async () => {
  const { source } = renderConsole([event("agent_message", { text: "newest" })])
  const state = { height: 1000, top: 0 }
  sizeLog(transcript(), state)
  pages([])

  scrollToTop(transcript(), state)
  await waitFor(() => expect(historyUrls).toHaveLength(1))
  await waitFor(() => expect(screen.queryByText("Loading older events…")).toBeNull())
  scrollToTop(transcript(), state)
  scrollToTop(transcript(), state)
  expect(historyUrls).toHaveLength(1)

  // A fresh snapshot replaces the transcript, and the pages load again.
  pages([event("user_prompt_submit", { text: "older prompt" })])
  act(() => source.emit("snapshot", [event("agent_message", { text: "fresh" })]))
  scrollToTop(transcript(), state)
  await within(transcript()).findByText("older prompt")
  expect(historyUrls).toHaveLength(1)
})

it("drops the loaded pages when a fresh snapshot replaces the transcript", async () => {
  const { source } = renderConsole([event("agent_message", { text: "newest" })])
  const state = { height: 1000, top: 0 }
  sizeLog(transcript(), state)
  pages([event("user_prompt_submit", { text: "older prompt" })])
  scrollToTop(transcript(), state)
  await within(transcript()).findByText("older prompt")

  act(() => source.emit("snapshot", [event("agent_message", { text: "fresh" })]))

  expect(within(transcript()).queryByText("older prompt")).toBeNull()
  within(transcript()).getByText("fresh")
})

const EXPLORER = { id: "child-1", title: "Explorer" }
const TESTER = { id: "child-2", title: "Tester" }

/** A `subagent_update` as the daemon stores it (021). */
function update(child: { id: string; title: string }, state: string, cancel = false) {
  return event("subagent_update", {
    subagent: { ...child, description: `${child.title} task` },
    state,
    capabilities: { cancel },
  })
}

function bar() {
  return screen.getByRole("tablist", { name: "Console views" })
}

function tabs() {
  return within(bar())
    .getAllByRole("tab")
    .map((tab) => `${tab.textContent}${tab.getAttribute("aria-selected") === "true" ? " *" : ""}`)
}

it("shows the bar only with a child, listing main and each child with its mark", () => {
  const { source } = renderConsole([event("agent_message", { text: "hello" })])
  expect(screen.queryByRole("tablist", { name: "Console views" })).toBeNull()

  act(() => {
    source.emit("event", update(EXPLORER, "running"))
    source.emit("event", update(TESTER, "running"))
    source.emit("event", update(TESTER, "failed"))
    source.emit("event", event("subagent_update", { subagent: { id: "child-3", title: "Idle" } }))
    source.emit(
      "event",
      event("permission_request", {
        tool_name: "Bash",
        subagent: EXPLORER,
        options: [{ optionId: "allow", name: "Allow" }],
      }),
    )
  })

  expect(tabs()).toEqual(["main *", "● Explorer ?", "✗ Tester", "◇ Idle"])
})

it("marks a completed and a cancelled child", () => {
  renderConsole([update(EXPLORER, "completed"), update(TESTER, "cancelled")])

  expect(tabs()).toEqual(["main *", "✓ Explorer", "⊘ Tester"])
})

it("switches the view with Tab, Shift-Tab and a click, in bar order, wrapping around", async () => {
  const user = userEvent.setup()
  renderConsole([update(EXPLORER, "running"), update(TESTER, "running")])

  fireEvent.keyDown(input(), { key: "Tab" })
  expect(tabs()).toEqual(["main", "● Explorer *", "● Tester"])
  await waitFor(() => expect(document.activeElement).not.toBe(document.body))
  fireEvent.keyDown(document.activeElement as Element, { key: "Tab" })
  expect(tabs()).toEqual(["main", "● Explorer", "● Tester *"])
  fireEvent.keyDown(document.activeElement as Element, { key: "Tab" })
  expect(tabs()).toEqual(["main *", "● Explorer", "● Tester"])
  fireEvent.keyDown(input(), { key: "Tab", shiftKey: true })
  expect(tabs()).toEqual(["main", "● Explorer", "● Tester *"])

  await user.click(screen.getByRole("tab", { name: /Explorer/ }))
  expect(tabs()).toEqual(["main", "● Explorer *", "● Tester"])
})

it("leaves Tab alone in a session with no child", () => {
  renderConsole()

  expect(fireEvent.keyDown(input(), { key: "Tab" })).toBe(true)
})

it("shows a child's own events read-only, and its questions and lines in main", async () => {
  const user = userEvent.setup()
  renderConsole([
    event("agent_message", { text: "parent says" }),
    update(EXPLORER, "running"),
    event("agent_message", { text: "child says", subagent: EXPLORER }),
    event("permission_request", {
      tool_name: "Bash",
      subagent: EXPLORER,
      options: [{ optionId: "allow", name: "Allow" }],
    }),
  ])

  within(transcript()).getByText("◇ subagent Explorer · Explorer task")
  within(transcript()).getByText("Permission · Bash · asked by subagent Explorer")
  expect(within(transcript()).queryByText("child says")).toBeNull()
  await user.click(screen.getByRole("option", { name: /Allow/ }))
  await waitFor(() => expect(posts[0]?.body).toBe(JSON.stringify({ text: "allow" })))

  await user.click(screen.getByRole("tab", { name: /Explorer/ }))

  within(transcript()).getByText("child says")
  expect(within(transcript()).queryByText("parent says")).toBeNull()
  expect(within(transcript()).queryByText(/asked by subagent/)).toBeNull()
  expect(screen.queryByRole("textbox", { name: "Message the agent" })).toBeNull()
  screen.getByText("read-only · tab next view · esc back")
})

it("goes back to main on Escape from a child that may not be cancelled", async () => {
  const user = userEvent.setup()
  renderConsole([update(EXPLORER, "running")], "running")
  await user.click(screen.getByRole("tab", { name: /Explorer/ }))

  fireEvent.keyDown(screen.getByRole("tab", { name: /Explorer/ }), { key: "Escape" })

  expect(tabs()).toEqual(["main *", "● Explorer"])
  expect(posts).toHaveLength(0)
})

it("cancels a running child that may be cancelled on Escape, and shows a refusal in its view", async () => {
  const user = userEvent.setup()
  answer(() => errorResponse(409, "subagent_not_cancellable", "the subagent has ended"))
  renderConsole([update(EXPLORER, "running", true)], "running")
  await user.click(screen.getByRole("tab", { name: /Explorer/ }))
  screen.getByText("read-only · tab next view · esc cancel")

  fireEvent.keyDown(screen.getByRole("tab", { name: /Explorer/ }), { key: "Escape" })

  await within(transcript()).findByText(/the subagent has ended/)
  expect(posts).toEqual([
    {
      path: `/v1/sessions/${SESSION_ID}/console/cancel`,
      body: JSON.stringify({ subagent_id: "child-1" }),
    },
  ])
  expect(tabs()).toEqual(["main", "● Explorer *"])
})

it("says how many subagents run", () => {
  const { source } = renderConsole([update(EXPLORER, "running"), update(TESTER, "running")])
  screen.getByText("running 2 subagents")

  act(() => source.emit("event", update(TESTER, "completed")))
  screen.getByText("running 1 subagent")

  act(() => source.emit("event", update(EXPLORER, "completed")))
  expect(screen.queryByText(/running \d subagent/)).toBeNull()
})

it("rebuilds the bar and the views from the snapshot of a reconnect", async () => {
  const user = userEvent.setup()
  const { source } = renderConsole([
    update(EXPLORER, "running"),
    event("agent_message", { text: "old child", subagent: EXPLORER }),
  ])
  await user.click(screen.getByRole("tab", { name: /Explorer/ }))

  act(() =>
    source.emit("snapshot", [
      update(TESTER, "running"),
      event("agent_message", { text: "new child", subagent: TESTER }),
    ]),
  )

  expect(tabs()).toEqual(["main *", "● Tester"])
  await user.click(screen.getByRole("tab", { name: /Tester/ }))
  within(transcript()).getByText("new child")
})

const lines = (count: number, label = "line") =>
  Array.from({ length: count }, (_, index) => `${label} ${index + 1}`).join("\n")

it("folds output, diffs and daemon prompts together, starting folded, from the header and Ctrl-O", async () => {
  const user = userEvent.setup()
  renderConsole([
    event("user_prompt_submit", { text: lines(10, "prompt"), source: "daemon" }),
    event("tool_call_update", {
      acp: {
        toolCallId: "call-1",
        title: "Bash",
        status: "completed",
        content: [{ type: "content", content: { text: lines(10, "out") } }],
      },
    }),
    event("tool_call_update", {
      acp: {
        toolCallId: "call-2",
        title: "Edit",
        content: [{ type: "diff", path: "a.ts", patch: { text: `@@\n${lines(30, "+add")}` } }],
      },
    }),
  ])

  const log = within(transcript())
  expect(log.getByText(/prompt 6/)).not.toBeNull()
  expect(log.queryByText(/prompt 7/)).toBeNull()
  expect(log.getByText(/out 10/)).not.toBeNull()
  expect(log.queryByText(/out 6\b/)).toBeNull()
  expect(log.getByText(/out 7/)).not.toBeNull()
  expect(log.queryByText("+add 25")).toBeNull()

  await user.click(screen.getByRole("button", { name: "Unfold" }))
  expect(log.getByText(/prompt 10/)).not.toBeNull()
  expect(log.getByText(/out 1\b/)).not.toBeNull()
  expect(log.getByText("+add 30")).not.toBeNull()

  fireEvent.keyDown(input(), { key: "o", ctrlKey: true })
  screen.getByRole("button", { name: "Unfold" })
  expect(log.queryByText(/out 6\b/)).toBeNull()
})

function listCommands(source: FakeEventSource, commands: object[]) {
  act(() =>
    source.emit("event", event("available_commands_update", { available_commands: commands })),
  )
}

it("suggests the agent's commands while the first word is typed, and completes the pick", () => {
  const { source } = renderConsole()
  listCommands(source, [
    { name: "init", description: "Set up", input: { hint: "the path" } },
    { name: "review", description: "Review it" },
    { name: "compact", description: "Shrink" },
  ])

  fireEvent.change(input(), { target: { value: "/i" } })
  const list = screen.getByRole("listbox", { name: "Slash commands" })
  expect(within(list).getAllByRole("option")).toHaveLength(2)
  within(list).getByText("the path")
  within(list).getByText("Set up")

  fireEvent.keyDown(input(), { key: "ArrowDown" })
  expect(within(list).getByRole("option", { name: /review/ })).toHaveProperty(
    "ariaSelected",
    "true",
  )
  fireEvent.keyDown(input(), { key: "Tab" })
  expect(input()).toHaveProperty("value", "/review ")
  expect(screen.queryByRole("listbox", { name: "Slash commands" })).toBeNull()
  expect(posts).toHaveLength(0)
})

it("says no command matches, closes the list on Escape without cancelling, and sends a command as text", async () => {
  const { source } = renderConsole([event("user_prompt_submit", { text: "go" })], "running")
  listCommands(source, [{ name: "init", description: "Set up" }])

  fireEvent.change(input(), { target: { value: "/zzz" } })
  screen.getByText("no matching command")

  fireEvent.change(input(), { target: { value: "/ini" } })
  fireEvent.keyDown(input(), { key: "Escape" })
  expect(screen.queryByRole("listbox", { name: "Slash commands" })).toBeNull()
  expect(posts).toHaveLength(0)

  fireEvent.change(input(), { target: { value: "/ini" } })
  fireEvent.change(input(), { target: { value: "" } })
  fireEvent.change(input(), { target: { value: "/ini" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  expect(input()).toHaveProperty("value", "/init ")
  fireEvent.keyDown(input(), { key: "Enter" })
  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]).toEqual({
    path: `/v1/sessions/${SESSION_ID}/console/input`,
    body: JSON.stringify({ text: "/init " }),
  })
})

it("shows at most eight suggestions", () => {
  const { source } = renderConsole()
  listCommands(
    source,
    Array.from({ length: 12 }, (_, index) => ({ name: `cmd${index}`, description: "" })),
  )

  fireEvent.change(input(), { target: { value: "/cmd" } })

  expect(
    within(screen.getByRole("listbox", { name: "Slash commands" })).getAllByRole("option"),
  ).toHaveLength(8)
})

it("shows the status, the turn with its clock, and the token totals", () => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date("2026-11-14T10:01:05Z"))
  const stop = event("stop", {
    stop_reason: "end_turn",
    ariadne_usage: { source: "a", input_tokens: 12_400, output_tokens: 3_100 },
  })
  const { source } = renderConsole([stop], "idle")
  const row = screen.getByLabelText("Session status")
  within(row).getByText("idle")
  within(row).getByText("↑12k ↓3.1k")
  expect(within(row).queryByText("thinking")).toBeNull()

  act(() => source.emit("event", event("user_prompt_submit", { text: "go" })))
  within(row).getByText("thinking")
  within(row).getByText("1m 05s")
  act(() => vi.advanceTimersByTime(3000))
  within(row).getByText("1m 08s")

  act(() => source.emit("event", event("pre_tool_use", { tool_name: "Bash" })))
  within(row).getByText("running Bash")
})

it("shows a sent prompt as queued until the console echo replaces it", async () => {
  const { source } = renderConsole()

  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  within(transcript()).getByText("ship it")
  within(transcript()).getByText("queued")

  act(() =>
    source.emit("event", event("user_prompt_submit", { text: "ship it", source: "console" })),
  )
  expect(within(transcript()).getAllByText("ship it")).toHaveLength(1)
  expect(within(transcript()).queryByText("queued")).toBeNull()
})

it("removes the queued prompt and adds an error block when the send fails", async () => {
  answer(() => errorResponse(409, "session_not_live", "the session is not live"))
  renderConsole()

  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await within(transcript()).findByText(/the session is not live/)
  expect(within(transcript()).queryByText("ship it")).toBeNull()
  expect(within(transcript()).queryByText("queued")).toBeNull()
})

it("names the keys of the state in the footer", () => {
  const { source } = renderConsole()
  screen.getByText(/enter send/)
  expect(screen.queryByText(/esc cancel/)).toBeNull()

  act(() => source.emit("event", event("user_prompt_submit", { text: "go" })))
  screen.getByText(/esc cancel/)

  act(() =>
    source.emit(
      "event",
      event("permission_request", {
        tool_name: "Edit",
        options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
      }),
    ),
  )
  screen.getByText(/up\/down or 1-9 choose · enter answer/)
  expect(screen.queryByText(/esc cancel/)).toBeNull()
})

it("folds on Ctrl-O from a header control, not only from the input", () => {
  renderConsole()
  const expand = screen.getByRole("button", { name: "Expand the console" })

  fireEvent.keyDown(expand, { key: "o", ctrlKey: true })
  screen.getByRole("button", { name: "Fold" })

  fireEvent.keyDown(screen.getByRole("button", { name: "Fold" }), { key: "o", ctrlKey: true })
  screen.getByRole("button", { name: "Unfold" })
})

it("reopens the suggestions on the next edit after Escape closed them", () => {
  const { source } = renderConsole()
  listCommands(source, [{ name: "init", description: "Set up" }])
  fireEvent.change(input(), { target: { value: "/ini" } })
  fireEvent.keyDown(input(), { key: "Escape" })
  expect(screen.queryByRole("listbox", { name: "Slash commands" })).toBeNull()

  fireEvent.change(input(), { target: { value: "/init" } })

  screen.getByRole("listbox", { name: "Slash commands" })
})

it("takes back only the failed send when the same prompt is sent twice at once", async () => {
  let calls = 0
  answer(() => {
    calls += 1
    return calls === 1
      ? errorResponse(409, "session_not_live", "the first send was refused")
      : new Response(null, { status: 204 })
  })
  const { source } = renderConsole()

  for (let send = 0; send < 2; send += 1) {
    fireEvent.change(input(), { target: { value: "again" } })
    fireEvent.keyDown(input(), { key: "Enter" })
  }

  await within(transcript()).findByText(/the first send was refused/)
  expect(within(transcript()).getAllByText("queued")).toHaveLength(1)

  act(() => source.emit("event", event("user_prompt_submit", { text: "again", source: "console" })))
  expect(within(transcript()).queryByText("queued")).toBeNull()
  expect(within(transcript()).getAllByText("again")).toHaveLength(1)
})

it("leaves nothing queued when the echo of the second send comes before the first send's failure", async () => {
  let refuse: () => void = () => {}
  const refused = new Promise<void>((resolve) => {
    refuse = resolve
  })
  let calls = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const listed = pendingRead(input instanceof Request ? input : new Request(input, init))
    if (listed) return listed
    calls += 1
    if (calls > 1) return new Response(null, { status: 204 })
    await refused
    return errorResponse(409, "session_not_live", "the first send was refused")
  })
  const { source } = renderConsole()

  for (let send = 0; send < 2; send += 1) {
    fireEvent.change(input(), { target: { value: "again" } })
    fireEvent.keyDown(input(), { key: "Enter" })
  }
  act(() => source.emit("event", event("user_prompt_submit", { text: "again", source: "console" })))
  act(() => refuse())

  await within(transcript()).findByText(/the first send was refused/)
  expect(within(transcript()).queryByText("queued")).toBeNull()
  expect(within(transcript()).getAllByText("again")).toHaveLength(1)
})

const QUESTION_EVENT = () =>
  event("agent_request", {
    request_id: "01RQ",
    hint: "Pick a base",
    question: {
      text: "Which base branch should the task land on?",
      options: [
        { id: "main", label: "Main", description: "Land on the trunk." },
        { id: "next", label: "Next" },
      ],
      recommended_option_id: "main",
    },
  })

const ANSWER_PATH = `/v1/sessions/${SESSION_ID}/agent-requests/01RQ/answer`

function questionCard() {
  return screen.getByRole("region", { name: "Question · Pick a base" })
}

it("draws a question as a card and submits a clicked choice at once to its request", async () => {
  renderConsole([QUESTION_EVENT()])
  const card = questionCard()
  within(card).getByText("Which base branch should the task land on?")
  within(card).getByText("Recommended: Main")
  within(card).getByText("Land on the trunk.")

  await userEvent.click(within(card).getByRole("button", { name: /^Next/ }))

  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: '{"option_id":"next"}' }]))
  await within(card).findByText("Answer sent")
  expect(within(card).getByRole("button", { name: /^Main/ })).toHaveProperty("disabled", true)
})

it("shows a failed answer on the card and keeps it answerable for another attempt", async () => {
  answer(() => errorResponse(409, "conflict", "no ACP agent is running"))
  renderConsole([QUESTION_EVENT()])
  const card = questionCard()

  await userEvent.click(within(card).getByRole("button", { name: /^Main/ }))
  await within(card).findByRole("alert")
  expect(within(card).getByRole("alert").textContent).toContain("no ACP agent is running")

  answer()
  await userEvent.click(within(card).getByRole("button", { name: /^Main/ }))
  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: '{"option_id":"main"}' }]))
  await waitFor(() => expect(within(card).queryByRole("alert")).toBeNull())
})

it("submits a custom answer as text", async () => {
  renderConsole([QUESTION_EVENT()])
  const card = questionCard()

  fireEvent.change(within(card).getByRole("textbox", { name: "Your own answer" }), {
    target: { value: "release" },
  })
  await userEvent.click(within(card).getByRole("button", { name: "Answer" }))

  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: '{"text":"release"}' }]))
})

it("draws an answered question closed, with no choice to submit again", () => {
  renderConsole([
    QUESTION_EVENT(),
    event("agent_request.resolved", {
      request_id: "01RQ",
      resolution: "answered",
      answer: { option_id: "next", text: "Next" },
    }),
  ])
  const card = questionCard()

  within(card).getByText("Answered · Next")
  expect(within(card).getByRole("button", { name: /^Main/ })).toHaveProperty("disabled", true)
  expect(within(card).queryByRole("textbox", { name: "Your own answer" })).toBeNull()
})

const PENDING = {
  id: "01RQ",
  summary: "Pick a base",
  created_at: "2026-11-14T09:00:00Z",
  question: {
    text: "Which base branch should the task land on?",
    options: [
      { id: "main", label: "Main", description: "Land on the trunk." },
      { id: "next", label: "Next" },
    ],
    recommended_option_id: "main",
  },
}

/** A page of later events that draw nothing: turns that ended the way turns end. */
function quietPage(count: number) {
  return Array.from({ length: count }, () => event("stop", { stop_reason: "end_turn" }))
}

it("restores a pending question older than the snapshot page on a load and a reconnect", async () => {
  pendingList = [PENDING]
  const later = quietPage(205)
  const { source } = renderConsole(later)
  await screen.findByRole("region", { name: "Question · Pick a base" })

  vi.useFakeTimers()
  act(() => source.emit("resync", { missed: 1 }))
  act(() => vi.advanceTimersByTime(500))
  vi.useRealTimers()
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", later)
  })
  const card = await screen.findByRole("region", { name: "Question · Pick a base" })
  expect(screen.getAllByRole("region", { name: "Question · Pick a base" })).toHaveLength(1)

  await userEvent.click(within(card).getByRole("button", { name: /^Next/ }))
  await waitFor(() => expect(posts).toEqual([{ path: ANSWER_PATH, body: '{"option_id":"next"}' }]))

  act(() =>
    latestSource().emit(
      "event",
      event("agent_request.resolved", {
        request_id: "01RQ",
        resolution: "answered",
        answer: { option_id: "next", text: "Next" },
      }),
    ),
  )
  await waitFor(() =>
    expect(screen.queryByRole("region", { name: "Question · Pick a base" })).toBeNull(),
  )
})

it("draws a pending question once where the snapshot already holds it, and closed where it closed", async () => {
  pendingList = [PENDING]
  renderConsole([
    QUESTION_EVENT(),
    event("agent_request.resolved", { request_id: "01RQ", resolution: "withdrawn" }),
  ])
  await waitFor(() => expect(daemonFetch).toHaveBeenCalled())
  // Let the pending list's answer land before the cards are counted.
  await act(async () => {})

  const cards = screen.getAllByRole("region", { name: "Question · Pick a base" })
  expect(cards).toHaveLength(1)
  within(cards[0] as HTMLElement).getByText("Withdrawn by the agent")
})

it("shows a failed read of the pending questions, and its Retry restores them", async () => {
  pendingList = [PENDING]
  pendingFailures = 1
  renderConsole(quietPage(205))

  const alert = await screen.findByRole("alert")
  expect(alert.textContent).toContain("Could not read the pending questions: the daemon is busy")
  expect(screen.queryByRole("region", { name: "Question · Pick a base" })).toBeNull()

  await userEvent.click(within(alert).getByRole("button", { name: "Retry" }))
  await screen.findByRole("region", { name: "Question · Pick a base" })
  expect(screen.queryByText(/Could not read the pending questions/)).toBeNull()
})
