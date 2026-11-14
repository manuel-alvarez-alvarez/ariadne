// @vitest-environment jsdom

/**
 * The composer and the actions around it: how it grows, what sends, what a
 * refused message keeps, the plus menu, and what the console remembers of a
 * session across an unmount.
 */

import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { AgentEventDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"

import { consoleViewKey, readViewState, updateViewState } from "./console-view-state"
import { SessionConsole } from "./session-console"

const SESSION_ID = "01JSESS0000000000000000001"
const posts: { path: string; body: string }[] = []
/** The pending requests `GET .../agent-requests` lists. */
let pendingList: unknown[] = []

/** Forget what the console retained, for the sessions these tests open. */
function resetViewStates() {
  for (const id of [SESSION_ID, "other"]) {
    updateViewState(consoleViewKey("http://127.0.0.1:7676", id), {
      draft: "",
      scrollTop: 0,
      anchor: undefined,
      oldest: undefined,
      following: true,
      folded: true,
      blockFolds: {},
      shown: undefined,
      failed: [],
    })
  }
}

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

/** The daemon, answering the requests list apart from the routes under test. */
function answer(reply: (path: string) => Response = () => new Response(null, { status: 204 })) {
  posts.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const path = new URL(request.url).pathname
    if (request.method === "GET" && path.endsWith("/agent-requests"))
      return Response.json(pendingList)
    posts.push({ path, body: await request.text() })
    return reply(path)
  })
}

beforeEach(() => {
  pendingList = []
  resetViewStates()
  stubEventSource()
  answer()
})

afterEach(() => {
  vi.restoreAllMocks()
  useSettingsStore.getState().resetBaseUrl()
})

function mount(
  snapshot: AgentEventDto[] = [],
  status: "running" | "idle" | "exited" = "idle",
  extra: Partial<Parameters<typeof SessionConsole>[0]> = {},
) {
  const view = renderScreen(
    <>
      <SessionConsole sessionId={SESSION_ID} status={status} {...extra} />
      <Toaster />
    </>,
  )
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", snapshot)
  })
  return { ...view, source }
}

const input = () =>
  screen.getByRole("textbox", { name: "Message the agent" }) as HTMLTextAreaElement
const transcript = () => screen.getByRole("log", { name: "Console transcript" })

it("starts at one line, grows with its text to six lines, then scrolls inside", () => {
  let height = 20
  vi.spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get").mockImplementation(() => height)
  mount()
  expect(input().rows).toBe(1)
  expect(input().style.height).toBe("20px")

  height = 60
  fireEvent.change(input(), { target: { value: "a\nb\nc" } })
  expect(input().style.height).toBe("60px")
  expect(input().style.overflowY).toBe("hidden")

  height = 400
  fireEvent.change(input(), { target: { value: Array(30).fill("x").join("\n") } })
  expect(input().style.height).toBe("120px")
  expect(input().style.overflowY).toBe("auto")
})

it("sends with Enter and the Send control, and keeps a newline, a composition and a paste", async () => {
  const user = userEvent.setup()
  mount()
  const send = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement
  expect(send.disabled).toBe(true)

  fireEvent.change(input(), { target: { value: "line one\nline two" } })
  fireEvent.keyDown(input(), { key: "Enter", shiftKey: true })
  fireEvent.keyDown(input(), { key: "Enter", isComposing: true })
  expect(posts).toHaveLength(0)
  expect(input().value).toBe("line one\nline two")

  await user.click(send)
  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]?.body).toBe(JSON.stringify({ text: "line one\nline two" }))
  expect(input().value).toBe("")
  expect(document.activeElement).toBe(input())

  fireEvent.change(input(), { target: { value: "again" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await waitFor(() => expect(posts).toHaveLength(2))
})

it("offers Stop beside Send while a turn runs", () => {
  mount([event("user_prompt_submit", { text: "go" })], "running")

  screen.getByRole("button", { name: "Stop the turn" })
  screen.getByRole("button", { name: "Send" })
})

it("keeps a refused message with Retry, and Retry sends the same text without touching the draft", async () => {
  const user = userEvent.setup()
  let refuse = true
  answer(() =>
    refuse
      ? errorResponse(409, "session_not_live", "not live")
      : new Response(null, { status: 204 }),
  )
  mount()
  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await within(transcript()).findByText(/not live/)
  fireEvent.change(input(), { target: { value: "newer draft" } })

  refuse = false
  await user.click(within(transcript()).getByRole("button", { name: "Retry" }))

  await waitFor(() => expect(posts).toHaveLength(2))
  expect(posts[1]?.body).toBe(JSON.stringify({ text: "ship it" }))
  expect(input().value).toBe("newer draft")
  expect(within(transcript()).queryByRole("button", { name: "Retry" })).toBeNull()
})

it("returns a refused message for editing in front of the newer draft", async () => {
  const user = userEvent.setup()
  answer(() => errorResponse(409, "session_not_live", "not live"))
  mount()
  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await within(transcript()).findByText(/not live/)
  fireEvent.change(input(), { target: { value: "newer" } })

  await user.click(within(transcript()).getByRole("button", { name: "Edit" }))

  expect(input().value).toBe("ship it\nnewer")
  expect(within(transcript()).queryByRole("button", { name: "Edit" })).toBeNull()
  expect(document.activeElement).toBe(input())
})

it("answers an open question from the composer on that request's own route", async () => {
  mount([
    event("agent_request", {
      request_id: "req-1",
      hint: "Pick one",
      question: { text: "Which one?", options: [{ id: "a", label: "A" }] },
    }),
  ])
  screen.getByRole("region", { name: "Question · Pick one" })

  fireEvent.change(input(), { target: { value: "my own words" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]).toEqual({
    path: `/v1/sessions/${SESSION_ID}/agent-requests/req-1/answer`,
    body: JSON.stringify({ text: "my own words" }),
  })
  screen.getByText("Answer sent")

  fireEvent.change(input(), { target: { value: "later" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await waitFor(() => expect(posts).toHaveLength(2))
  expect(posts[1]?.path).toBe(`/v1/sessions/${SESSION_ID}/console/input`)
})

async function openMenu(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Commands and session actions" }))
}

function commands(source: FakeEventSource) {
  act(() =>
    source.emit(
      "event",
      event("available_commands_update", {
        available_commands: [
          { name: "init", description: "Set up" },
          { name: "review", description: "Review it" },
        ],
      }),
    ),
  )
}

it("inserts a menu command in front of the draft for editing, and hands the composer focus", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)
  fireEvent.change(input(), { target: { value: "the auth module" } })

  await openMenu(user)
  await user.type(screen.getByRole("textbox", { name: "Search commands and actions" }), "rev")
  expect(screen.queryByRole("button", { name: /\/init/ })).toBeNull()
  await user.click(screen.getByRole("button", { name: /\/review/ }))

  await waitFor(() => expect(input().value).toBe("/review the auth module"))
  expect(posts).toHaveLength(0)
  await waitFor(() => expect(document.activeElement).toBe(input()))
  expect(input().selectionStart).toBe("/review ".length)
})

it("picks a menu entry with the keyboard", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)

  await openMenu(user)
  const search = screen.getByRole("textbox", { name: "Search commands and actions" })
  await waitFor(() => expect(document.activeElement).toBe(search))
  await user.keyboard("{ArrowDown}")
  await user.keyboard("{ArrowDown}")
  await user.keyboard("{Enter}")

  await waitFor(() => expect(input().value).toBe("/review "))
})

it("completes a suggestion on a click, keeping the composer's focus", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)
  input().focus()
  fireEvent.change(input(), { target: { value: "/re" } })

  await user.click(
    within(screen.getByRole("listbox", { name: "Slash commands" })).getByText("/review"),
  )

  expect(input().value).toBe("/review ")
  expect(document.activeElement).toBe(input())
})

it("offers Resume only once the session has ended, and resumes through the existing route", async () => {
  const user = userEvent.setup()
  answer(() => jsonResponse(aSession({ id: SESSION_ID, status: "running" })))
  const onResumed = vi.fn()
  mount([], "exited", { onResumed })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Resume" }))

  await waitFor(() => expect(onResumed).toHaveBeenCalled())
  expect(posts[0]?.path).toBe(`/v1/sessions/${SESSION_ID}/resume`)
})

it("hides Resume while the session is live, and Switch where it cannot switch", async () => {
  const user = userEvent.setup()
  mount([], "idle", { session: aSession({ id: SESSION_ID }), canSwitch: false })

  await openMenu(user)

  expect(screen.queryByRole("button", { name: "Resume" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Switch agent/model" })).toBeNull()
  screen.getByRole("button", { name: "Expand all" })
})

it("opens the switch dialog from the menu", async () => {
  const user = userEvent.setup()
  mount([], "idle", { session: aSession({ id: SESSION_ID }) })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Switch agent/model" }))

  await screen.findByText("Switch session")
})

it("copies only the loaded transcript of the view shown, and names that scope", async () => {
  const user = userEvent.setup()
  const written: string[] = []
  vi.stubGlobal("navigator", {
    ...navigator,
    clipboard: { writeText: async (text: string) => void written.push(text) },
  })
  mount([
    event("user_prompt_submit", { text: "Fix the bug" }),
    event("agent_message", { text: "Fixed it" }),
  ])

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Copy loaded transcript (main)" }))

  await waitFor(() => expect(written).toEqual(["> Fix the bug\n\nFixed it"]))
  vi.unstubAllGlobals()
})

it("expands and collapses everything from the menu", async () => {
  const user = userEvent.setup()
  mount()
  screen.getByRole("button", { name: "Unfold" })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Expand all" }))

  screen.getByRole("button", { name: "Fold" })
  expect(readViewState(consoleViewKey("http://127.0.0.1:7676", SESSION_ID)).folded).toBe(false)
})

it("brings a session's draft and fold back after the console is unmounted", () => {
  mount()
  fireEvent.change(input(), { target: { value: "half a thought" } })
  fireEvent.click(screen.getByRole("button", { name: "Unfold" }))
  cleanup()
  stubEventSource()

  mount()

  expect(input().value).toBe("half a thought")
  screen.getByRole("button", { name: "Fold" })
})

it("keeps each daemon's and each session's state apart", () => {
  updateViewState(consoleViewKey("http://127.0.0.1:7676", SESSION_ID), { draft: "mine" })

  expect(readViewState(consoleViewKey("http://127.0.0.1:7676", "other")).draft).toBe("")
  expect(readViewState(consoleViewKey("http://10.0.0.2:7676", SESSION_ID)).draft).toBe("")

  useSettingsStore.getState().setBaseUrl("http://10.0.0.2:7676")
  mount()
  expect(input().value).toBe("")
})

it("restores the reader's place, not the bottom, when the console comes back", () => {
  const key = consoleViewKey("http://127.0.0.1:7676", SESSION_ID)
  updateViewState(key, { following: false, scrollTop: 37 })
  mount([event("agent_message", { text: "one" })])

  expect(transcript().scrollTop).toBe(37)
  expect(readViewState(key).following).toBe(false)
})

it("holds one open stream for the console shown, and none once it is hidden", async () => {
  const user = userEvent.setup()
  mount()
  const open = () => FakeEventSource.instances.filter((source) => !source.closed)
  expect(open()).toHaveLength(1)

  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  expect(open()).toHaveLength(1)

  await user.click(screen.getByRole("button", { name: "Collapse the console back into the panel" }))
  expect(open()).toHaveLength(1)

  cleanup()
  expect(open()).toHaveLength(0)
})

it("completes a suggestion by keyboard or click and keeps the text after the command", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)
  input().focus()
  fireEvent.change(input(), { target: { value: "/re the auth module" } })
  input().setSelectionRange(2, 2)
  fireEvent.select(input())

  fireEvent.keyDown(input(), { key: "Enter" })
  expect(input().value).toBe("/review the auth module")
  expect(posts).toHaveLength(0)

  fireEvent.change(input(), { target: { value: "/in\nsecond line" } })
  input().setSelectionRange(3, 3)
  fireEvent.select(input())
  await user.click(
    within(screen.getByRole("listbox", { name: "Slash commands" })).getByText("/init"),
  )
  expect(input().value).toBe("/init\nsecond line")
})

it("keeps a refused message, with Retry and Edit, when the console is unmounted and mounted again", async () => {
  answer(() => errorResponse(409, "session_not_live", "not live"))
  mount()
  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })
  await within(transcript()).findByText(/not live/)
  cleanup()
  stubEventSource()

  mount()

  within(transcript()).getByText("ship it")
  within(transcript()).getByRole("button", { name: "Retry" })
  within(transcript()).getByRole("button", { name: "Edit" })
})

it("answers a question restored from the requests list from the composer", async () => {
  pendingList = [
    {
      id: "01RQ",
      summary: "Pick a base",
      created_at: "2026-11-14T09:00:00Z",
      question: { text: "Which base?", options: [{ id: "main", label: "Main" }] },
    },
  ]
  mount()
  await screen.findByRole("region", { name: "Question · Pick a base" })

  fireEvent.change(input(), { target: { value: "my own base" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]).toEqual({
    path: `/v1/sessions/${SESSION_ID}/agent-requests/01RQ/answer`,
    body: JSON.stringify({ text: "my own base" }),
  })
})

it("shows in the replacement pane a send that fails after the console was expanded", async () => {
  const user = userEvent.setup()
  let refuse: () => void = () => {}
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    if (request.method === "GET") return Response.json([])
    await new Promise<void>((resolve) => {
      refuse = resolve
    })
    return errorResponse(409, "session_not_live", "not live")
  })
  mount()
  fireEvent.change(input(), { target: { value: "ship it" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await act(async () => refuse())

  const dialog = await screen.findByRole("dialog")
  await within(dialog).findByText(/not live/)
  within(dialog).getByRole("button", { name: "Retry" })
  within(dialog).getByRole("button", { name: "Edit" })
})

it("answers the newest of a restored and a loaded question", async () => {
  pendingList = [
    {
      id: "01OLD",
      summary: "Old one",
      created_at: "2026-11-14T08:00:00Z",
      question: { text: "Older?", options: [] },
    },
  ]
  mount([
    event("agent_request", {
      request_id: "01NEW",
      hint: "New one",
      question: { text: "Newer?", options: [] },
    }),
  ])
  await screen.findByRole("region", { name: "Question · Old one" })

  fireEvent.change(input(), { target: { value: "reply" } })
  fireEvent.keyDown(input(), { key: "Enter" })

  await waitFor(() => expect(posts).toHaveLength(1))
  expect(posts[0]?.path).toBe(`/v1/sessions/${SESSION_ID}/agent-requests/01NEW/answer`)
})

it("keeps a draft of only spaces and newlines when a menu command is inserted", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)
  fireEvent.change(input(), { target: { value: " \n " } })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: /\/review/ }))

  await waitFor(() => expect(input().value).toBe("/review  \n "))
})

it("brings back the older page the reader had loaded, and their place in it, after the console expands", async () => {
  const user = userEvent.setup()
  const older = event("user_prompt_submit", { text: "older prompt" })
  const newest = event("agent_message", { text: "newest" })
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const path = new URL(request.url).pathname
    if (path.endsWith("/agent-requests")) return Response.json([])
    if (path === "/v1/events") {
      const before = new URL(request.url).searchParams.get("before")
      return Response.json(before === newest.id ? [older] : [])
    }
    return new Response(null, { status: 204 })
  })
  mount([newest])
  const size = (log: HTMLElement, state: { top: number }) => {
    Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => 1400 })
    Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
    Object.defineProperty(log, "scrollTop", {
      configurable: true,
      get: () => state.top,
      set: (value: number) => {
        state.top = value
      },
    })
  }
  const first = { top: 0 }
  size(transcript(), first)
  fireEvent.scroll(transcript())
  await within(transcript()).findByText("older prompt")
  first.top = 500
  fireEvent.scroll(transcript())

  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  const second = { top: 0 }
  const log = await screen.findByRole("log", { name: "Console transcript" })
  size(log, second)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [newest])
  })

  await within(log).findByText("older prompt")
  await waitFor(() => expect(second.top).toBe(500))
})

it("puts the block the reader was on back in place when new events move the page boundary", async () => {
  const user = userEvent.setup()
  const older2 = event("user_prompt_submit", { text: "oldest prompt" })
  const older = event("user_prompt_submit", { text: "older prompt" })
  const newest = event("agent_message", { text: "newest" })
  const extra = event("agent_message", { text: "newer still" })
  let fetches = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const path = new URL(request.url).pathname
    if (path.endsWith("/agent-requests")) return Response.json([])
    if (path === "/v1/events") {
      fetches += 1
      // The first read gets one event; the reload gets a page that goes past it.
      return Response.json(fetches === 1 ? [older] : fetches === 2 ? [older, older2] : [])
    }
    return new Response(null, { status: 204 })
  })
  // Every block is 100px tall, stacked in order below the scroll box's top.
  const state = { top: 0 }
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
    this: HTMLElement,
  ) {
    const index = this.dataset.blockId
      ? Array.from(this.parentElement?.querySelectorAll("[data-block-id]") ?? []).indexOf(this)
      : 0
    const top = this.dataset.blockId ? index * 100 - state.top : 0
    return { top, bottom: top + (this.dataset.blockId ? 100 : 0) } as DOMRect
  })
  const size = (log: HTMLElement) => {
    Object.defineProperty(log, "scrollHeight", { configurable: true, get: () => 1400 })
    Object.defineProperty(log, "clientHeight", { configurable: true, get: () => 200 })
    Object.defineProperty(log, "scrollTop", {
      configurable: true,
      get: () => state.top,
      set: (value: number) => {
        state.top = value
      },
    })
  }
  mount([newest])
  size(transcript())
  fireEvent.scroll(transcript())
  await within(transcript()).findByText("older prompt")
  // Blocks: older, newest. The reader has newest's top 50px above the box.
  state.top = 150
  fireEvent.scroll(transcript())

  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  state.top = 0
  const log = await screen.findByRole("log", { name: "Console transcript" })
  size(log)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [newest, extra])
  })

  await within(log).findByText("oldest prompt")
  // Blocks: oldest, older, newest, extra. newest is third, so its top is 200 - top.
  await waitFor(() => expect(state.top).toBe(250))
})
