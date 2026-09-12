// @vitest-environment jsdom

/**
 * The console every session gets: pinning down that a stream turns into a
 * readable transcript — text streamed in chunks and settled by the stored
 * message, a thought folded away, a tool call that opens to its output and
 * its diff, a plan that updates in place — that a typed message reaches the
 * console's input endpoint and shows as pending until the daemon confirms
 * it, that a permission question is answered inline or by its number key,
 * that a running turn can be stopped, and that the pane follows new output
 * until the reader scrolls up.
 *
 * `AcpConsole` is exercised rather than `ConsoleView` directly: the portal it
 * wraps around the view is not the point of any of these, and testing through
 * it is what proves it renders the view at all.
 *
 * A stream frame delivered through `FakeEventSource.emit` lands outside any
 * `act()` React is told about, the same as `daemon-logs-drawer.test.tsx`'s
 * does — so the assertion right after one always goes through `findBy*`,
 * which waits for it, rather than the synchronous `getBy*`.
 */

import { fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import {
  aMessageChunk,
  anAgentEvent,
  aPlan,
  aStop,
  aThoughtChunk,
  aToolCall,
  aToolEvent,
} from "@/test/fixtures"
import { daemonFetch, renderScreen } from "@/test/harness"

import { AcpConsole } from "./acp-console"

const SESSION = "01JSESS0000000000000000001"

beforeEach(() => {
  stubEventSource()
})

afterEach(() => {
  vi.unstubAllGlobals()
})

/** Renders the console and opens its one stream connection. */
function open(status: "running" | "exited" = "running"): FakeEventSource {
  renderScreen(<AcpConsole sessionId={SESSION} status={status} />)
  const source = latestSource()
  source.succeed()
  return source
}

function requestBody(callIndex = 0): Promise<unknown> {
  const request = daemonFetch.mock.calls[callIndex]?.[0] as Request
  return request.text().then((body) => JSON.parse(body))
}

function requestUrl(callIndex = 0): string {
  const request = daemonFetch.mock.calls[callIndex]?.[0] as Request | undefined
  return request?.url ?? ""
}

function aPrompt(text: string, id: string, source = "console") {
  return anAgentEvent({ id, kind: "user_prompt_submit", payload: { text, source } })
}

const transcript = () => screen.getByRole("log", { name: "Console transcript" })

it("renders the transcript the stream delivers: a snapshot, then a delta", async () => {
  const source = open()

  source.emit("snapshot", [
    anAgentEvent({ id: "01E1", kind: "session_start", payload: { session_id: SESSION } }),
    anAgentEvent({ id: "01E2", kind: "agent_message", payload: { text: "All done here." } }),
  ])

  expect(await screen.findByText("Session started")).not.toBeNull()
  expect(screen.getByText("All done here.")).not.toBeNull()

  // A later turn's tool call, delivered as a delta rather than a fresh
  // connection — the whole point of a stream over a snapshot.
  source.emit(
    "event",
    aToolEvent(
      "pre_tool_use",
      aToolCall({ title: "Read", rawInput: { file_path: "AGENTS.md" } }),
      "01E3",
    ),
  )

  expect(await screen.findByText("Read")).not.toBeNull()
  expect(screen.getByText("AGENTS.md")).not.toBeNull()
})

it("streams message chunks into one item, which the stored message then replaces", async () => {
  const source = open()
  source.emit("snapshot", [aPrompt("plan it", "01P1")])
  expect(await screen.findByText("plan it")).not.toBeNull()

  source.emit("event", aMessageChunk("# Plan\n\n", "01C1"))
  source.emit("event", aMessageChunk("```sh\nls\n```", "01C2"))

  // One item, read as markdown: the heading and the code block are elements,
  // not the `#` and the fences.
  expect(await screen.findByRole("heading", { name: "Plan" })).not.toBeNull()
  expect(screen.getAllByRole("heading")).toHaveLength(1)
  expect(transcript().querySelector("pre code")?.textContent).toBe("ls\n")

  source.emit(
    "event",
    anAgentEvent({
      id: "01M1",
      kind: "agent_message",
      payload: { text: "# Plan\n\n```sh\nls\n```" },
    }),
  )
  source.emit("event", aStop("end_turn", "01S1"))

  // Still one heading: the whole replaced the chunks rather than joining them.
  await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull())
  expect(screen.getAllByRole("heading", { name: "Plan" })).toHaveLength(1)
})

it("renders a thought dimmed and folded, and the toggle opens it", async () => {
  const user = userEvent.setup()
  const source = open()
  source.emit("snapshot", [aPrompt("plan it", "01P1")])
  source.emit("event", aThoughtChunk("Think ", "01T1"))
  source.emit("event", aThoughtChunk("first.", "01T2"))

  const toggle = await screen.findByRole("button", { name: "Thought" })
  expect(screen.getByText("Think first.")).not.toBeNull()
  expect(toggle.getAttribute("aria-expanded")).toBe("false")
  expect(screen.getByText("Think first.").className).toContain("line-clamp-2")

  await user.click(toggle)
  expect(toggle.getAttribute("aria-expanded")).toBe("true")
  expect(screen.getByText("Think first.").className).not.toContain("line-clamp-2")

  // The stored thought replaces the streamed one, still one row.
  source.emit(
    "event",
    anAgentEvent({ id: "01T3", kind: "agent_thought", payload: { text: "Think first." } }),
  )
  await waitFor(() => expect(screen.getAllByText("Think first.")).toHaveLength(1))
})

it("shows a tool call as one row with its status, and opens it to the output", async () => {
  const user = userEvent.setup()
  const source = open()
  const call = aToolCall({ toolCallId: "call-2", title: "Bash", rawInput: { command: "ls" } })

  source.emit("snapshot", [aToolEvent("pre_tool_use", call, "01E1")])
  const row = await screen.findByRole("button", { name: /Bash/ })
  expect(within(row).getByText("Running")).not.toBeNull()
  expect(within(row).getByText("ls")).not.toBeNull()

  // Progress folds into the same row rather than adding one.
  source.emit("event", aToolEvent("tool_call_update", { ...call, status: "in_progress" }, "01E2"))
  source.emit(
    "event",
    aToolEvent(
      "post_tool_use",
      {
        ...call,
        status: "completed",
        content: [{ type: "content", content: { type: "text", text: "README.md" } }],
        rawOutput: { stdout: "README.md\n" },
      },
      "01E3",
    ),
  )

  await waitFor(() => expect(within(row).getByText("Done")).not.toBeNull())
  expect(screen.getAllByRole("button", { name: /Bash/ })).toHaveLength(1)
  expect(screen.queryByText("README.md")).toBeNull()

  await user.click(row)
  expect(screen.getByText("README.md")).not.toBeNull()
})

it("marks a failed tool call as failed", async () => {
  const source = open()
  source.emit("snapshot", [
    aToolEvent("post_tool_use", aToolCall({ status: "failed", rawOutput: "no such file" }), "01E1"),
  ])

  const row = await screen.findByRole("button", { name: /Bash/ })
  expect(within(row).getByText("Failed")).not.toBeNull()
})

it("opens a diff content entry to a diff view", async () => {
  const user = userEvent.setup()
  const source = open()
  source.emit("snapshot", [
    aToolEvent(
      "post_tool_use",
      aToolCall({
        title: "Edit",
        status: "completed",
        rawInput: { file_path: "a.txt" },
        content: [
          { type: "diff", path: "a.txt", oldText: "keep\ngone\n", newText: "keep\nadded\n" },
        ],
      }),
      "01E1",
    ),
  ])

  await user.click(await screen.findByRole("button", { name: /Edit/ }))

  const diff = screen.getByRole("region", { name: "Diff of a.txt" })
  expect(diff.querySelector(".cm-deletedChunk")?.textContent).toContain("gone")
  expect(diff.querySelector(".cm-content")?.textContent).toContain("added")
})

it("renders a plan as a checklist and updates it in place", async () => {
  const source = open()
  source.emit("snapshot", [
    aPrompt("go", "01P1"),
    aPlan(
      [
        { content: "List the files", priority: "high", status: "pending" },
        { content: "Read them", priority: "medium", status: "pending" },
      ],
      "01L1",
    ),
  ])

  const plan = await screen.findByRole("list", { name: "Plan" })
  expect(within(plan).getAllByRole("listitem")).toHaveLength(2)
  expect(
    within(within(plan).getAllByRole("listitem")[0] as HTMLElement).getByText("Pending"),
  ).not.toBeNull()

  source.emit(
    "event",
    aPlan(
      [
        { content: "List the files", priority: "high", status: "completed" },
        { content: "Read them", priority: "medium", status: "in_progress" },
      ],
      "01L2",
    ),
  )

  await waitFor(() => expect(within(plan).getByText("Done")).not.toBeNull())
  expect(screen.getAllByRole("list", { name: "Plan" })).toHaveLength(1)
  expect(within(plan).getByText("In progress")).not.toBeNull()
  expect(screen.getAllByText("List the files")).toHaveLength(1)
})

it("sends typed text to the console's input endpoint, and shows it pending until confirmed", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  const source = open()
  source.emit("snapshot", [])
  await waitFor(() =>
    expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("disabled", false),
  )

  await user.type(screen.getByPlaceholderText("Message the agent…"), "keep going")
  await user.keyboard("{Enter}")

  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(1))
  expect(requestUrl()).toContain(`/v1/sessions/${SESSION}/console/input`)
  expect(await requestBody()).toEqual({ text: "keep going" })

  // Shown at once as a prompt line, marked pending until the daemon has
  // said anything back.
  expect(screen.getByText("Sending…")).not.toBeNull()
  expect(screen.getByText("keep going").closest("[aria-busy='true']")).not.toBeNull()

  // The daemon turns it into a turn, which reaches the console as a
  // `user_prompt_submit` carrying the text — in order, never out of turn
  // with what was posted.
  source.emit("event", aPrompt("keep going", "01E9"))

  await waitFor(() => expect(screen.queryByText("Sending…")).toBeNull())
  expect(screen.getAllByText("keep going")).toHaveLength(1)
  expect(screen.getByText("keep going").closest("[aria-busy='true']")).toBeNull()
})

it("clears a pending line whose confirming prompt arrived while the stream was down", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  vi.useFakeTimers({ shouldAdvanceTime: true })
  const source = open()
  source.emit("snapshot", [])
  await waitFor(() =>
    expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("disabled", false),
  )

  await user.type(screen.getByPlaceholderText("Message the agent…"), "keep going")
  await user.keyboard("{Enter}")
  expect(screen.getByText("Sending…")).not.toBeNull()

  // The stream drops before the daemon's confirmation reaches it. The retry
  // opens on a fresh snapshot, and that is where the confirmation is.
  source.fail()
  vi.advanceTimersByTime(60_000)
  const fresh = latestSource()
  expect(fresh).not.toBe(source)
  fresh.succeed()
  fresh.emit("snapshot", [aPrompt("keep going", "01E9")])

  await waitFor(() => expect(screen.queryByText("Sending…")).toBeNull())
  expect(screen.getAllByText("keep going")).toHaveLength(1)
  vi.useRealTimers()
})

it("takes no input before the first snapshot, so a history ending on the same text confirms nothing", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  const source = open()

  // Connected, but the history is not on record yet: nothing can be posted,
  // so no line can be waiting on a snapshot that would confirm it wrongly.
  const input = screen.getByPlaceholderText("Waiting for the console…")
  expect(input).toHaveProperty("disabled", true)

  // The history's newest console prompt reads exactly what is typed next.
  source.emit("snapshot", [
    aPrompt("start here", "01E1"),
    aStop("end_turn", "01E2"),
    aPrompt("keep going", "01E3"),
    aStop("end_turn", "01E4"),
  ])
  await waitFor(() =>
    expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("disabled", false),
  )

  await user.type(screen.getByPlaceholderText("Message the agent…"), "keep going")
  await user.keyboard("{Enter}")

  // Pending, beside the identical line of the history: that one was seen
  // before the line was posted and confirms nothing.
  expect(screen.getByText("Sending…")).not.toBeNull()
  expect(screen.getAllByText("keep going")).toHaveLength(2)

  source.emit("event", aPrompt("keep going", "01E5"))

  await waitFor(() => expect(screen.queryByText("Sending…")).toBeNull())
  expect(screen.getAllByText("keep going")).toHaveLength(2)
})

it("inserts a newline on Shift+Enter instead of sending", async () => {
  const user = userEvent.setup()
  const source = open()
  source.emit("snapshot", [])
  await waitFor(() =>
    expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("disabled", false),
  )

  await user.type(screen.getByPlaceholderText("Message the agent…"), "one")
  await user.keyboard("{Shift>}{Enter}{/Shift}two")

  expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("value", "one\ntwo")
  expect(daemonFetch).not.toHaveBeenCalled()
})

it("does not queue a message once the session has ended", () => {
  open("exited")

  expect(screen.getByPlaceholderText("This session has ended.")).toHaveProperty("disabled", true)
})

it("answers an inline permission question", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  const source = open()

  source.emit("snapshot", [
    anAgentEvent({
      id: "01EP",
      kind: "permission_request",
      summary: "Write: src/main.rs",
      payload: {
        tool_name: "Write",
        options: [
          { optionId: "no", name: "Reject", kind: "reject_once" },
          { optionId: "yes", name: "Allow", kind: "allow_once" },
        ],
      },
    }),
  ])

  expect(await screen.findByRole("button", { name: "Allow" })).not.toBeNull()
  await user.click(screen.getByRole("button", { name: "Allow" }))

  // Answered on the spot — the same input endpoint a typed message uses,
  // carrying the option's own label as the text.
  expect(screen.getByText("Answered: Allow")).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Allow" })).toBeNull()
  expect(screen.queryByRole("button", { name: "Reject" })).toBeNull()

  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(1))
  expect(requestUrl()).toContain(`/v1/sessions/${SESSION}/console/input`)
  expect(await requestBody()).toEqual({ text: "Allow" })
})

it("answers a pending permission with its first option on the 1 key, and keeps the key in the pane", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  const source = open()
  const outside = vi.fn()
  window.addEventListener("keydown", outside)

  source.emit("snapshot", [
    anAgentEvent({
      id: "01EP",
      kind: "permission_request",
      payload: {
        tool_name: "Write",
        options: [
          { optionId: "no", name: "Reject", kind: "reject_once" },
          { optionId: "yes", name: "Allow", kind: "allow_once" },
        ],
      },
    }),
  ])
  expect(await screen.findByRole("button", { name: "Reject" })).not.toBeNull()

  // The pane has the keyboard and the input is empty: a digit is a pick,
  // not a character.
  await user.click(screen.getByPlaceholderText("Message the agent…"))
  await user.keyboard("1")

  await waitFor(() => expect(screen.getByText("Answered: Reject")).not.toBeNull())
  expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("value", "")
  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(1))
  expect(await requestBody()).toEqual({ text: "Reject" })
  expect(outside).not.toHaveBeenCalled()
  window.removeEventListener("keydown", outside)
})

it("types a digit into the input once there is text in it", async () => {
  const user = userEvent.setup()
  const source = open()
  source.emit("snapshot", [
    anAgentEvent({
      id: "01EP",
      kind: "permission_request",
      payload: { tool_name: "Write", options: [{ optionId: "yes", name: "Allow" }] },
    }),
  ])
  expect(await screen.findByRole("button", { name: "Allow" })).not.toBeNull()

  await user.type(screen.getByPlaceholderText("Message the agent…"), "step 1")

  expect(screen.getByPlaceholderText("Message the agent…")).toHaveProperty("value", "step 1")
  expect(screen.getByRole("button", { name: "Allow" })).not.toBeNull()
  expect(daemonFetch).not.toHaveBeenCalled()
})

// The daemon replies to every permission request itself under `auto` —
// reaching the console as its own event, with no id back to the request it
// answers, paired here in the order both arrived.
it("shows a permission request the daemon already answered as resolved", async () => {
  const source = open()

  source.emit("snapshot", [
    anAgentEvent({
      id: "01EP",
      kind: "permission_request",
      summary: "Write: src/main.rs",
      payload: {
        tool_name: "Write",
        options: [{ optionId: "yes", name: "Allow", kind: "allow_once" }],
      },
    }),
    anAgentEvent({ id: "01ER", kind: "permission.replied", payload: { option_id: "yes" } }),
  ])

  expect(await screen.findByText("Answered: Allow")).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Allow" })).toBeNull()
})

it("stops a running turn from the Stop button and from Escape in the input, and shows the stop", async () => {
  const user = userEvent.setup()
  daemonFetch.mockResolvedValue(new Response(null, { status: 204 }))
  const source = open()
  const outside = vi.fn()
  window.addEventListener("keydown", outside)

  // A prompt with no stop after it is a turn still running.
  source.emit("snapshot", [aPrompt("go", "01P1")])

  await user.click(await screen.findByRole("button", { name: "Stop" }))
  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(1))
  expect(requestUrl(0)).toContain(`/v1/sessions/${SESSION}/console/cancel`)

  await user.click(screen.getByPlaceholderText("Message the agent…"))
  await user.keyboard("{Escape}")
  await waitFor(() => expect(daemonFetch).toHaveBeenCalledTimes(2))
  expect(requestUrl(1)).toContain(`/v1/sessions/${SESSION}/console/cancel`)
  expect(outside).not.toHaveBeenCalled()

  source.emit("event", aStop("cancelled", "01S1"))

  expect(await screen.findByText("Stopped")).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Stop" })).toBeNull()
  window.removeEventListener("keydown", outside)
})

it("offers no Stop between turns", async () => {
  const source = open()
  source.emit("snapshot", [aPrompt("go", "01P1"), aStop("end_turn", "01S1")])

  expect(await screen.findByText("go")).not.toBeNull()
  expect(screen.queryByRole("button", { name: "Stop" })).toBeNull()
})

it("follows new output until the reader scrolls up, and the chip brings it back", async () => {
  const user = userEvent.setup()
  const source = open()
  source.emit("snapshot", [aPrompt("go", "01P1")])
  expect(await screen.findByText("go")).not.toBeNull()

  // jsdom lays nothing out: the pane is given a height and an overflow by
  // hand, the way a browser would measure them.
  const log = transcript()
  Object.defineProperty(log, "scrollHeight", { configurable: true, value: 1000 })
  Object.defineProperty(log, "clientHeight", { configurable: true, value: 200 })

  source.emit("event", aMessageChunk("first", "01C1"))
  await screen.findByText("first")
  expect(log.scrollTop).toBe(1000)
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull()

  // Scrolling up to read releases the pane, and it says how to come back.
  fireEvent.scroll(log, { target: { scrollTop: 100 } })
  expect(await screen.findByRole("button", { name: "Jump to latest" })).not.toBeNull()

  source.emit("event", aMessageChunk(" second", "01C2"))
  await screen.findByText("first second")
  expect(log.scrollTop).toBe(100)

  await user.click(screen.getByRole("button", { name: "Jump to latest" }))
  expect(log.scrollTop).toBe(1000)
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull()

  // Scrolling back down by hand re-arms the follow too.
  fireEvent.scroll(log, { target: { scrollTop: 100 } })
  expect(await screen.findByRole("button", { name: "Jump to latest" })).not.toBeNull()
  fireEvent.scroll(log, { target: { scrollTop: 800 } })
  await waitFor(() => expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull())
})
