// @vitest-environment jsdom

/**
 * The expanded terminal and the renderer behind it.
 *
 * A panel is a small window onto a pane, so the terminal can be lifted into a
 * dialog. What is worth pinning down about that move is not the size — that is
 * layout, and jsdom has none — but the three things it could quietly break:
 * the emulator has to *travel*, keeping the connection and the output it
 * already has rather than opening a new stream and fetching the pane again; it
 * has to still be the pane's keyboard once it is in there; and it is inside a
 * dialog that closes on Escape, which is also a keystroke the agent's TUI
 * expects to be handed.
 *
 * The renderer is the other half. The GPU one is fetched only once the
 * emulator is open, and every way of not getting it — no WebGL2 context, an
 * addon that will not load, a context the driver takes back — has to leave the
 * terminal drawing through the DOM rather than not drawing at all.
 *
 * xterm needs a browser this environment only half is, so `matchMedia`,
 * `ResizeObserver` and the canvas are stubbed for it. Everything else is real:
 * a real emulator, keystrokes typed into it the way a user types them, and the
 * real `openapi-fetch` path behind a controllable `fetch`.
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

/** What the page answers when something asks the canvas for a context. */
let webglContext: WebGL2RenderingContext | null
/** Whether the addon refuses to load, the way it does with no context to draw into. */
let webglRefuses: boolean
/** How many times the addon was asked for, and how many times it took. */
let webglRequests: number
let webglLoads: number
let webglDisposals: number
/** Takes the context back, the way a driver reset does. */
let loseWebglContext: (() => void) | null

// The real addon is a WebGL renderer, which is exactly the thing this
// environment does not have; what these tests are about is what the component
// does with it, so the addon is a stand-in that records being used.
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    constructor() {
      webglRequests += 1
    }

    activate(): void {
      if (webglRefuses) throw new Error("WebGL2 is not supported")
      webglLoads += 1
    }

    dispose(): void {
      webglDisposals += 1
    }

    onContextLoss(handler: () => void): { dispose: () => void } {
      loseWebglContext = handler
      return { dispose: () => {} }
    }
  },
}))

const realGetContext = HTMLCanvasElement.prototype.getContext

beforeEach(() => {
  connections = []
  keystrokes = []
  webglContext = {
    getExtension: () => ({ loseContext: () => {} }),
  } as unknown as WebGL2RenderingContext
  webglRefuses = false
  webglRequests = 0
  webglLoads = 0
  webglDisposals = 0
  loseWebglContext = null
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
  // jsdom has no canvas at all, and says so on every call; the answer to the
  // one question the component asks it is what decides the renderer.
  HTMLCanvasElement.prototype.getContext = ((id: string) =>
    id === "webgl2" ? webglContext : null) as typeof HTMLCanvasElement.prototype.getContext
})

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  HTMLCanvasElement.prototype.getContext = realGetContext
  globalThis.fetch = originalFetch
  setApiBaseUrl(DEFAULT_BASE_URL)
})

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

function expand(): Promise<void> {
  return userEvent.setup().click(screen.getByRole("button", { name: /expand the terminal/i }))
}

/** xterm's own input, which is what a focused terminal actually is. */
function keyboardOf(container: HTMLElement): HTMLTextAreaElement {
  const textarea = container.querySelector("textarea")
  if (!textarea) throw new Error("the emulator has no keyboard")
  return textarea
}

/** The one emulator on the page — there is never a second one. */
function emulator(): HTMLElement {
  const found = document.querySelectorAll<HTMLElement>(".xterm")
  const only = found.length === 1 ? found[0] : undefined
  if (!only) throw new Error(`expected one emulator on the page, found ${found.length}`)
  return only
}

/** A turn of the event loop: long enough for the addon's import to have landed. */
function settle(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0)
  })
}

it("carries the same emulator and stream into the dialog and back", async () => {
  const user = userEvent.setup()
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  expect(connections).toHaveLength(1)
  const pane = emulator()

  await expand()

  // The panel says where the terminal went rather than going blank.
  expect(screen.getByText(/open in the expanded view/i)).toBeTruthy()
  const dialog = screen.getByRole("dialog")
  // Moved, not rebuilt: a second emulator would mean a second connection, and
  // a whole snapshot fetched for output that is already on screen.
  expect(emulator()).toBe(pane)
  expect(dialog.contains(pane)).toBe(true)
  expect(connections).toHaveLength(1)
  expect(connections[0]?.closed).toBe(false)

  // And it is still the pane's keyboard in there.
  keyboardOf(dialog).focus()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")

  // The dialog is modal, so the way back is its own collapse control; the
  // panel's "Back to the panel" is behind it, for once the dialog is gone.
  await user.click(screen.getByRole("button", { name: /collapse the terminal/i }))

  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.queryByText(/open in the expanded view/i)).toBeNull()
  expect(emulator()).toBe(pane)
  expect(connections).toHaveLength(1)
  expect(connections[0]?.closed).toBe(false)
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

it("draws through the GPU where there is one, and drops it with its context", async () => {
  render(<SessionTerminal sessionId={SESSION} status="running" />)

  // The addon is fetched only once the emulator is open, so it lands a turn
  // after the terminal does.
  await vi.waitFor(() => expect(webglLoads).toBe(1))

  // A context the driver takes back leaves xterm drawing through the DOM
  // again, which is all disposing the addon means; asking for another one
  // would only lose it the same way.
  expect(loseWebglContext).toBeTypeOf("function")
  loseWebglContext?.()
  expect(webglDisposals).toBe(1)
  expect(emulator()).toBeTruthy()
})

it("falls back to the DOM renderer when the WebGL addon will not load", async () => {
  const user = userEvent.setup()
  webglRefuses = true
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await settle()

  expect(webglRequests).toBe(1)
  expect(webglLoads).toBe(0)
  // The renderer xterm opened with never went anywhere, so this is still a
  // terminal: it is on the page, and it is still typed into.
  keyboardOf(emulator()).focus()
  await user.keyboard("hi")
  expect(keystrokes.join("")).toBe("hi")
})

it("does not fetch the WebGL addon without a context to draw into", async () => {
  webglContext = null
  render(<SessionTerminal sessionId={SESSION} status="running" />)
  await settle()

  // Not merely unused: a renderer this browser cannot run is a download it
  // should never have made.
  expect(webglRequests).toBe(0)
  expect(loseWebglContext).toBeNull()
  expect(emulator()).toBeTruthy()
})
