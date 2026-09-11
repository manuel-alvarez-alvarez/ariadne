// @vitest-environment jsdom

/**
 * The console every session gets: pinning down that a
 * stream turns into a readable transcript, that a typed message reaches the
 * console's input endpoint and shows as pending until the daemon confirms it,
 * and that an inline permission question is answered the same way.
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

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { anAgentEvent } from "@/test/fixtures"
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
function open(): FakeEventSource {
  renderScreen(<AcpConsole sessionId={SESSION} status="running" />)
  const source = latestSource()
  source.succeed()
  return source
}

function requestBody(callIndex = 0): Promise<unknown> {
  const request = daemonFetch.mock.calls[callIndex]?.[0] as Request
  return request.text().then((body) => JSON.parse(body))
}

it("renders the transcript the stream delivers: a snapshot, then a delta", async () => {
  const source = open()

  source.emit("snapshot", [
    anAgentEvent({ id: "01E1", kind: "session_start", payload: { session_id: SESSION } }),
    anAgentEvent({
      id: "01E2",
      kind: "stop",
      payload: { last_assistant_message: "All done here." },
    }),
  ])

  expect(await screen.findByText("Session started")).not.toBeNull()
  expect(screen.getByText("All done here.")).not.toBeNull()

  // A later turn's tool call, delivered as a delta rather than a fresh
  // connection — the whole point of a stream over a snapshot.
  source.emit(
    "event",
    anAgentEvent({
      id: "01E3",
      kind: "pre_tool_use",
      summary: "Read: AGENTS.md",
      payload: { tool_name: "Read", tool_input: { file_path: "AGENTS.md" } },
    }),
  )

  expect(await screen.findByText("Read: AGENTS.md")).not.toBeNull()
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
  const request = daemonFetch.mock.calls[0]?.[0] as Request
  expect(request.url).toContain(`/v1/sessions/${SESSION}/console/input`)
  expect(await requestBody()).toEqual({ text: "keep going" })

  // Queued rather than lost: shown at once, before the daemon has said
  // anything back.
  expect(screen.getByText("Sending…")).not.toBeNull()
  expect(screen.getByText("keep going")).not.toBeNull()

  // The daemon turns it into a turn, which reaches the console as a
  // `user_prompt_submit` — in order, never out of turn with what was posted.
  source.emit("event", anAgentEvent({ id: "01E9", kind: "user_prompt_submit", payload: {} }))

  await waitFor(() => expect(screen.queryByText("Sending…")).toBeNull())
  expect(screen.getByText("keep going")).not.toBeNull()
})

it("does not queue a message once the session has ended", () => {
  renderScreen(<AcpConsole sessionId={SESSION} status="exited" />)

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
  const request = daemonFetch.mock.calls[0]?.[0] as Request
  expect(request.url).toContain(`/v1/sessions/${SESSION}/console/input`)
  expect(await requestBody()).toEqual({ text: "Allow" })
})

// The daemon replies to every permission request itself (021's known gap:
// every request is auto-approved) — reaching the console as its own event,
// with no id back to the request it answers, paired here in the order both
// arrived.
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
