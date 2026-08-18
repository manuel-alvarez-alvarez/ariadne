// @vitest-environment jsdom

/**
 * The expanded terminal: how it is reached, and who gets Escape once it is.
 *
 * A panel is a small window onto a pane, so the terminal can be lifted into a
 * dialog. What is worth pinning down about that move is not the size — that is
 * layout, and jsdom has none — but the two things it could quietly break: the
 * emulator is remounted there, so it has to attach a stream of its own rather
 * than leave the old one dangling; and it is inside a dialog that closes on
 * Escape, which is also a keystroke the agent's TUI expects to be handed.
 *
 * xterm needs a browser this environment only half is, so `matchMedia` and
 * `ResizeObserver` are stubbed for it. Everything else is real: a real
 * emulator, keystrokes typed into it the way a user types them, and the real
 * `openapi-fetch` path behind a controllable `fetch`.
 */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { DEFAULT_BASE_URL, setApiBaseUrl } from "@/api"

import { SessionTerminal } from "./session-terminal"

const SESSION = "01ARZ3NDEKTSV4RRFFQ69G5FAV"
/** Anywhere but a real daemon: these requests must never leave the process. */
const BASE_URL = "http://daemon.test"

/** One log-stream connection, and whether it is still open. */
interface Connection {
  url: string
  closed: boolean
}

let connections: Connection[]
/** The `data` of every keystroke request the terminal made, oldest first. */
let keystrokes: string[]
let originalFetch: typeof globalThis.fetch

/** Enough of `EventSource` for the stream to open and be closed again. */
class StubEventSource {
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  readonly #connection: Connection

  constructor(url: string) {
    this.#connection = { url, closed: false }
    connections.push(this.#connection)
  }

  addEventListener(): void {}

  close(): void {
    this.#connection.closed = true
  }
}

beforeEach(() => {
  connections = []
  keystrokes = []
  originalFetch = globalThis.fetch
  globalThis.fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    keystrokes.push(JSON.parse(await request.clone().text()).data)
    return new Response(null, { status: 204 })
  }) as typeof globalThis.fetch
  // `openapi-fetch` captures `globalThis.fetch` when the client is built, so
  // the client has to be rebuilt after the stub is in place.
  setApiBaseUrl(BASE_URL)
  vi.stubGlobal("EventSource", StubEventSource)
  // xterm measures the device pixel ratio and watches the frame; neither
  // exists here, and neither is what these tests are about.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }))
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    },
  )
})

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  globalThis.fetch = originalFetch
  setApiBaseUrl(DEFAULT_BASE_URL)
})

function expand(): Promise<void> {
  return userEvent.setup().click(screen.getByRole("button", { name: /expand the terminal/i }))
}

/** xterm's own input, which is what a focused terminal actually is. */
function keyboardOf(container: HTMLElement): HTMLTextAreaElement {
  const textarea = container.querySelector("textarea")
  if (!textarea) throw new Error("the emulator has no keyboard")
  return textarea
}

it("moves the terminal into a dialog and back, on a stream of its own each time", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  expect(connections).toHaveLength(1)

  await expand()

  // The panel says where the terminal went rather than going blank.
  expect(screen.getByText(/open in the expanded view/i)).toBeTruthy()
  const dialog = screen.getByRole("dialog")
  expect(keyboardOf(dialog)).toBeTruthy()
  // Remounted, so the old connection is gone and a new one opened: every
  // connection starts from a full snapshot, which is what redraws the pane.
  expect(connections).toHaveLength(2)
  expect(connections[0]?.closed).toBe(true)
  expect(connections[1]?.closed).toBe(false)

  // The dialog is modal, so the way back is its own collapse control; the
  // panel's "Back to the panel" is behind it, for once the dialog is gone.
  await user.click(screen.getByRole("button", { name: /collapse the terminal/i }))

  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.queryByText(/open in the expanded view/i)).toBeNull()
  expect(connections).toHaveLength(3)
  expect(connections[1]?.closed).toBe(true)
  expect(connections[2]?.closed).toBe(false)
})

it("hands Escape to the agent while the terminal has the keyboard", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await expand()

  keyboardOf(screen.getByRole("dialog")).focus()
  await user.keyboard("{Escape}")

  // The dialog closing on that press would take the pane away from under the
  // keystroke that was meant for it.
  expect(screen.getByRole("dialog")).toBeTruthy()

  // And the pane is still being typed into from in here.
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

it("still closes on Escape from outside the terminal", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await expand()

  screen.getByRole("dialog").focus()
  await user.keyboard("{Escape}")

  expect(screen.queryByRole("dialog")).toBeNull()
})
