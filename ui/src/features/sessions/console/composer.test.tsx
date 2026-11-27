// @vitest-environment jsdom

/**
 * The composer and the actions around it: how it grows, what sends, what a
 * refused message keeps, the plus menu, and what the console remembers of a
 * session across an unmount.
 */

import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { qk, rememberHosts, type TrajectorySnapshotDto } from "@/api"
import { Toaster } from "@/components/ui/sonner"
import { useSettingsStore } from "@/stores/settings"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aSession } from "@/test/fixtures"
import { daemonFetch, errorResponse, jsonResponse, renderScreen } from "@/test/harness"
import { BOX, forgetHosts, LOCAL } from "@/test/hosts"
import { aSnapshot, aTurn } from "@/test/trajectory"
import { TrajectoryConsole } from "../trajectory/trajectory-console"
import { consoleViewKey, readViewState, updateViewState } from "./console-view-state"

const SESSION_ID = "01JSESS0000000000000000001"
const posts: { path: string; body: string }[] = []

/** Forget what the console retained, for the sessions these tests open. */
function resetViewStates() {
  for (const id of [SESSION_ID, "other"]) {
    updateViewState(consoleViewKey("local", "http://127.0.0.1:7676", id), {
      draft: "",
      scrollTop: 0,
      anchor: undefined,
      oldest: undefined,
      following: true,
      blockFolds: {},
      shown: undefined,
      failed: [],
    })
  }
}

/** The daemon, answering the questions list apart from the routes under test. */
function answer(reply: (path: string) => Response = () => new Response(null, { status: 204 })) {
  posts.length = 0
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const path = new URL(request.url).pathname
    if (request.method === "GET" && path.endsWith("/questions")) return Response.json([])
    posts.push({ path, body: await request.text() })
    return reply(path)
  })
}

beforeEach(() => {
  resetViewStates()
  stubEventSource()
  answer()
})

afterEach(() => {
  vi.restoreAllMocks()
  useSettingsStore.getState().resetBaseUrl()
  forgetHosts()
})

function mount(
  snapshot: TrajectorySnapshotDto = aSnapshot([]),
  status: "running" | "idle" | "exited" = "idle",
  extra: Partial<Parameters<typeof TrajectoryConsole>[0]> = {},
) {
  const view = renderScreen(
    <>
      <TrajectoryConsole host="local" sessionId={SESSION_ID} status={status} {...extra} />
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
  mount(aSnapshot([aTurn({ state: "running", ended_at: null })]), "running")

  screen.getByRole("button", { name: "Stop the turn" })
  screen.getByRole("button", { name: "Send" })
})

it("draws Send and Stop as icons without a label, on a row that never wraps", () => {
  mount(aSnapshot([aTurn({ state: "running", ended_at: null })]), "running")

  const send = screen.getByRole("button", { name: "Send" })
  const stop = screen.getByRole("button", { name: "Stop the turn" })
  expect(send.textContent).toBe("")
  expect(stop.textContent).toBe("")
  expect(send.querySelector("svg")).not.toBeNull()
  expect(stop.querySelector("svg")).not.toBeNull()
  expect(send.parentElement?.className).not.toContain("flex-wrap")
  expect(send.parentElement?.lastElementChild).toBe(send)
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

async function openMenu(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Commands and session actions" }))
}

/** The agent lists its slash commands: a newer snapshot carries them in the session state. */
function commands(
  source: FakeEventSource,
  list = [
    { name: "init", description: "Set up" },
    { name: "review", description: "Review it" },
  ],
) {
  const snapshot = aSnapshot([])
  snapshot.rev = 2
  snapshot.session.commands = list
  act(() => source.emit("snapshot", snapshot))
}

it("cuts a long command name and the copy label in the menu, whole on hover, with no sideways scroll", async () => {
  const user = userEvent.setup()
  const long = `${"very-long-command-".repeat(6)}end`
  const { source } = mount()
  commands(source, [{ name: long, description: "Does a long thing" }])

  await openMenu(user)

  const command = screen.getByRole("button", { name: new RegExp(long) })
  expect(command.getAttribute("title")).toBe(`/${long} Does a long thing`)
  expect(command.querySelector("span")?.className).toContain("truncate")
  const copy = screen.getByRole("button", { name: "Copy loaded transcript (main)" })
  expect(copy.getAttribute("title")).toBe("Copy loaded transcript (main)")
  expect(copy.querySelector("span")?.className).toContain("truncate")
  expect(command.parentElement?.className).toContain("overflow-x-hidden")
})

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
  mount(aSnapshot([]), "exited", { onResumed })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Resume" }))

  await waitFor(() => expect(onResumed).toHaveBeenCalled())
  expect(posts[0]?.path).toBe(`/v1/sessions/${SESSION_ID}/resume`)
})

it("hides Resume while the session is live, and Switch where it cannot switch", async () => {
  const user = userEvent.setup()
  mount(aSnapshot([]), "idle", { session: aSession({ id: SESSION_ID }), canSwitch: false })

  await openMenu(user)

  expect(screen.queryByRole("button", { name: "Resume" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Switch agent/model" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Expand all" })).toBeNull()
})

it("opens the switch dialog from the menu", async () => {
  const user = userEvent.setup()
  mount(aSnapshot([]), "idle", { session: aSession({ id: SESSION_ID }) })

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
  mount(aSnapshot([aTurn()]))

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: "Copy loaded transcript (main)" }))

  await waitFor(() => expect(written).toEqual(["> Fix the bug\n\nFixed it"]))
  vi.unstubAllGlobals()
})

it("omits global fold actions from the menu", async () => {
  const user = userEvent.setup()
  mount()
  await openMenu(user)
  expect(screen.queryByRole("button", { name: /^(Expand all|Collapse all)$/ })).toBeNull()
})

it("brings a session's draft back after the console is unmounted", () => {
  mount()
  fireEvent.change(input(), { target: { value: "half a thought" } })
  cleanup()
  stubEventSource()

  mount()

  expect(input().value).toBe("half a thought")
  expect(screen.queryByRole("button", { name: "Fold" })).toBeNull()
})

it("keeps each daemon's and each session's state apart", () => {
  updateViewState(consoleViewKey("local", "http://127.0.0.1:7676", SESSION_ID), { draft: "mine" })

  expect(readViewState(consoleViewKey("local", "http://127.0.0.1:7676", "other")).draft).toBe("")
  expect(readViewState(consoleViewKey("local", "http://10.0.0.2:7676", SESSION_ID)).draft).toBe("")

  useSettingsStore.getState().setBaseUrl("http://10.0.0.2:7676")
  mount()
  expect(input().value).toBe("")
})

it("keeps a named host's state when its address changes, and gives the host's reason while it is down", async () => {
  const at = (rows: (typeof BOX)[]) => {
    rememberHosts([LOCAL, ...rows])
    return [LOCAL, ...rows]
  }
  at([BOX])
  const view = renderScreen(<TrajectoryConsole host="box" sessionId={SESSION_ID} status="idle" />)
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", [])
  })
  fireEvent.change(input(), { target: { value: "half a thought" } })
  const key = consoleViewKey("box", "http://box.test:7676", SESSION_ID)
  updateViewState(key, {
    blockFolds: { b1: true },
    failed: [{ id: "f", text: "t", message: "no" }],
  })

  // The host drops: the stream closes, the status line says why, the state stays.
  const down = { ...BOX, status: "down" as const, endpoint: null, error: "ssh closed" }
  act(() => view.queryClient.setQueryData(qk.hosts.list(), at([down])))
  expect(await screen.findByText("(ssh closed)")).toBeTruthy()
  expect(input().value).toBe("half a thought")

  // It returns on another port: a new stream, and the same draft, folds and failed send.
  act(() =>
    view.queryClient.setQueryData(
      qk.hosts.list(),
      at([{ ...BOX, endpoint: "http://box.test:9000" }]),
    ),
  )
  await waitFor(() => expect(latestSource().url).toContain("http://box.test:9000"))
  expect(input().value).toBe("half a thought")
  expect(readViewState(consoleViewKey("box", "http://box.test:9000", SESSION_ID))).toBe(
    readViewState(key),
  )
  expect(readViewState(key).blockFolds).toEqual({ b1: true })
  expect(readViewState(key).failed).toHaveLength(1)
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

it("keeps a draft of only spaces and newlines when a menu command is inserted", async () => {
  const user = userEvent.setup()
  const { source } = mount()
  commands(source)
  fireEvent.change(input(), { target: { value: " \n " } })

  await openMenu(user)
  await user.click(screen.getByRole("button", { name: /\/review/ }))

  await waitFor(() => expect(input().value).toBe("/review  \n "))
})
