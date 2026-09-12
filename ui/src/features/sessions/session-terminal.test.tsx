// @vitest-environment jsdom

/**
 * The terminal pane on the daemon's terminal socket, driven from the
 * client's side of the wire.
 *
 * What the pane owes the protocol is what is pinned here: its size before
 * anything else, since the daemon draws nothing until it knows one; every
 * binary frame written into the emulator, which is where the transcript then
 * reads; a key press and a paste as the messages the console reads them
 * from; and a drop that is retried rather than left as a dead pane. The
 * emulator is the real xterm.js — jsdom lays nothing out, so it keeps its
 * default 80 by 24 grid, and what it draws is read back off its rows.
 */

import { act, fireEvent, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { renderScreen } from "@/test/harness"
import { FakeWebSocket, latestSocket, stubWebSocket } from "@/test/web-socket"

import { SessionTerminal } from "./session-terminal"

const SESSION_ID = "01JSESS0000000000000000001"

beforeEach(() => {
  stubWebSocket()
})

afterEach(() => {
  vi.useRealTimers()
})

/** The pane, with its socket dialled: the daemon has not answered yet. */
async function renderPane() {
  renderScreen(<SessionTerminal sessionId={SESSION_ID} status="running" />)
  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1))
  return latestSocket()
}

/** What the emulator shows on its rows, with the padding of the grid trimmed. */
function screenText(): string {
  return screen.getByLabelText("Console terminal").querySelector(".xterm-rows")?.textContent ?? ""
}

/** The keyboard the emulator listens on: its own hidden textarea. */
function keyboard(): HTMLTextAreaElement {
  const textarea = screen.getByLabelText("Console terminal").querySelector("textarea")
  if (!textarea) throw new Error("the terminal has no textarea to type into")
  return textarea
}

it("sends its size before anything else", async () => {
  const socket = await renderPane()
  expect(socket.url).toBe(`ws://127.0.0.1:7676/v1/sessions/${SESSION_ID}/console/terminal`)
  // Typed before the daemon accepted: there is no console to take it yet.
  fireEvent.keyDown(keyboard(), { key: "a", code: "KeyA" })
  expect(socket.sent).toHaveLength(0)

  socket.succeed()

  expect(socket.messages[0]).toEqual({ type: "resize", cols: 80, rows: 24 })
})

it("writes the bytes of a binary frame into the terminal", async () => {
  const socket = await renderPane()
  socket.succeed()

  socket.deliver(new TextEncoder().encode("Hello from the \x1b[1mconsole\x1b[0m"))

  await waitFor(() => expect(screenText()).toContain("Hello from the console"))
})

it("sends a key press as a key message, and a paste as a paste message", async () => {
  const socket = await renderPane()
  socket.succeed()
  const sentAfterOpen = socket.messages.length

  fireEvent.keyDown(keyboard(), { key: "a", code: "KeyA" })
  fireEvent.keyDown(keyboard(), { key: "Enter", code: "Enter", ctrlKey: true })
  fireEvent.keyDown(keyboard(), { key: "Tab", code: "Tab", shiftKey: true })
  fireEvent.keyDown(keyboard(), { key: "∫", code: "KeyB", altKey: true })
  fireEvent.keyDown(keyboard(), { key: "F5", code: "F5" })
  // A bare modifier is no key press, and a command-key chord is the app's.
  fireEvent.keyDown(keyboard(), { key: "Shift", code: "ShiftLeft", shiftKey: true })
  fireEvent.keyDown(keyboard(), { key: "k", code: "KeyK", metaKey: true })
  fireEvent.paste(keyboard(), { clipboardData: { getData: () => "two\nlines" } })

  expect(socket.messages.slice(sentAfterOpen)).toEqual([
    { type: "key", code: { char: "a" }, modifiers: [] },
    { type: "key", code: "enter", modifiers: ["control"] },
    { type: "key", code: "back_tab", modifiers: ["shift"] },
    { type: "key", code: { char: "b" }, modifiers: ["alt"] },
    { type: "key", code: { f: 5 }, modifiers: [] },
    { type: "paste", text: "two\nlines" },
  ])
})

it("says it is reconnecting after a drop, and dials again", async () => {
  const socket = await renderPane()
  act(() => socket.succeed())
  screen.getByText("Live")
  // Faked only now: the retry is the one timer under test, and the pane's
  // opening waits on a promise that real timers had to settle.
  vi.useFakeTimers()

  act(() => socket.drop())

  screen.getByText(/reconnecting/)
  expect(FakeWebSocket.instances).toHaveLength(1)

  // The first retry is due within the initial backoff.
  act(() => vi.advanceTimersByTime(500))

  expect(FakeWebSocket.instances).toHaveLength(2)
  act(() => latestSocket().succeed())
  expect(latestSocket().messages[0]).toEqual({ type: "resize", cols: 80, rows: 24 })
  screen.getByText("Live")
})

it("ends the console on a close the daemon meant, and a button opens another", async () => {
  const user = userEvent.setup()
  const socket = await renderPane()
  socket.succeed()
  socket.deliverText({ type: "status", status: "exited" })

  socket.end()

  await screen.findByText("This session has ended.")
  expect(FakeWebSocket.instances).toHaveLength(1)

  await user.click(screen.getByRole("button", { name: "Reopen" }))

  expect(FakeWebSocket.instances).toHaveLength(2)
})

it("expands the console into a near-fullscreen modal", async () => {
  const user = userEvent.setup()
  await renderPane()

  await user.click(screen.getByRole("button", { name: "Expand the console" }))

  const dialog = await screen.findByRole("dialog", { name: "Session console" })
  expect(dialog.className).toContain("h-[calc(100dvh-2rem)]")
  expect(
    screen.getByRole("button", { name: "Collapse the console back into the panel" }),
  ).not.toBeNull()
})

it("closes the panel socket before opening and resizing the modal console", async () => {
  const user = userEvent.setup()
  const panelSocket = await renderPane()
  panelSocket.succeed()

  await user.click(screen.getByRole("button", { name: "Expand the console" }))

  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
  const modalSocket = latestSocket()
  expect(panelSocket.readyState).toBe(FakeWebSocket.CLOSED)

  fireEvent.keyDown(keyboard(), { key: "a", code: "KeyA" })
  expect(modalSocket.messages).toHaveLength(0)

  modalSocket.succeed()
  fireEvent.keyDown(keyboard(), { key: "a", code: "KeyA" })

  expect(modalSocket.messages).toEqual([
    { type: "resize", cols: 80, rows: 24 },
    { type: "key", code: { char: "a" }, modifiers: [] },
  ])
})

it("collapses the modal console into the panel on a fresh socket", async () => {
  const user = userEvent.setup()
  const panelSocket = await renderPane()
  panelSocket.succeed()
  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
  const modalSocket = latestSocket()
  modalSocket.succeed()

  await user.click(screen.getByRole("button", { name: "Collapse the console back into the panel" }))

  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(3))
  expect(modalSocket.readyState).toBe(FakeWebSocket.CLOSED)
  expect(screen.queryByRole("dialog", { name: "Session console" })).toBeNull()
  expect(screen.getByRole("button", { name: "Expand the console" })).not.toBeNull()
})

it("keeps the modal open when focused Escape belongs to the console", async () => {
  const user = userEvent.setup()
  await renderPane()
  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2))
  const modalSocket = latestSocket()
  modalSocket.succeed()

  fireEvent.keyDown(keyboard(), { key: "Escape", code: "Escape" })

  expect(modalSocket.messages.at(-1)).toEqual({ type: "key", code: "esc", modifiers: [] })
  expect(screen.getByRole("dialog", { name: "Session console" })).not.toBeNull()
})

it("closes the modal when Escape occurs outside the console", async () => {
  const user = userEvent.setup()
  await renderPane()
  await user.click(screen.getByRole("button", { name: "Expand the console" }))
  await screen.findByRole("dialog", { name: "Session console" })

  fireEvent.keyDown(screen.getByRole("dialog", { name: "Session console" }), {
    key: "Escape",
    code: "Escape",
  })

  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Session console" })).toBeNull())
})
