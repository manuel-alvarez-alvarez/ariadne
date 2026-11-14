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

function answer(reply: () => Response = () => new Response(null, { status: 204 })) {
  posts.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    posts.push({ path: new URL(request.url).pathname, body: await request.text() })
    return reply()
  })
}

beforeEach(() => {
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
  expect(daemonFetch).not.toHaveBeenCalled()
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
