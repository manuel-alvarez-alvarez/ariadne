/**
 * The browser a real xterm emulator needs, as much of it as jsdom is not.
 *
 * `session-terminal.test.tsx` runs the actual emulator — a real terminal, real
 * keystrokes typed into it, the real `openapi-fetch` path — against a daemon
 * and a page that are stubbed here: the log stream, the resize and input
 * endpoints, the canvas the GPU renderer asks for a context, the
 * `ResizeObserver` the fit watches its frame through, and `matchMedia`, which
 * xterm reads the device pixel ratio from.
 *
 * Everything a test drives or reads back lives in {@link stub} and the arrays
 * beside it, all of them reset by {@link installTerminalEnvironment}.
 */

import { afterEach, beforeEach, vi } from "vitest"

import { DEFAULT_BASE_URL, setApiBaseUrl } from "@/api"
import type { PaneSize } from "@/features/sessions/log-stream"

/** Anywhere but a real daemon: these requests must never leave the process. */
const BASE_URL = "http://daemon.test"

/** One log-stream connection, and whether it is still open. */
export interface Connection {
  url: string
  closed: boolean
}

export const connections: Connection[] = []
/** The streams themselves, for the tests that deliver something down one. */
export const streams: StubEventSource[] = []
/** The `data` of every keystroke request the terminal made, oldest first. */
export const keystrokes: string[] = []
/** The grid of every resize request it made, oldest first. */
export const resizes: PaneSize[] = []
/** The session each of those resizes was addressed to. */
export const resizeTargets: string[] = []

/** Everything a test sets before rendering, or reads back after. */
export const stub = {
  /** What the frame would measure to, if this environment measured anything. */
  roomForPane: null as PaneSize | null,
  /** How many times the frame was measured at all. */
  fits: 0,
  /** Whether the daemon refuses to resize the pane. */
  resizeFails: false,
  /** What the page answers when something asks the canvas for a context. */
  webglContext: null as WebGL2RenderingContext | null,
  /** Whether the addon refuses to load, as it does with no context to draw into. */
  webglRefuses: false,
  /** How many times the addon was asked for, and how many times it took. */
  webglRequests: 0,
  webglLoads: 0,
  webglDisposals: 0,
  /** Takes the context back, the way a driver reset does. */
  loseWebglContext: null as (() => void) | null,
}

/** The `ResizeObserver` callbacks watching a frame, in the order they started. */
let frameObservers: Array<(entries: { contentRect: { width: number; height: number } }[]) => void> =
  []

/** Enough of `EventSource` for the stream to open, deliver and be closed. */
class StubEventSource {
  onopen: ((event: Event) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  readonly #connection: Connection
  readonly #listeners = new Map<string, (event: MessageEvent) => void>()

  constructor(url: string) {
    this.#connection = { url, closed: false }
    connections.push(this.#connection)
    streams.push(this)
  }

  addEventListener(type: string, handler: (event: MessageEvent) => void): void {
    this.#listeners.set(type, handler)
  }

  /** Deliver one event, the way the daemon's stream would. */
  emit(type: string, payload: unknown): void {
    this.#listeners.get(type)?.({ data: JSON.stringify(payload) } as MessageEvent)
  }

  close(): void {
    this.#connection.closed = true
  }
}

const realGetContext = HTMLCanvasElement.prototype.getContext
let originalFetch: typeof globalThis.fetch

/** Registers the whole environment around the test file that calls it. */
export function installTerminalEnvironment(): void {
  beforeEach(() => {
    connections.length = 0
    streams.length = 0
    keystrokes.length = 0
    resizes.length = 0
    resizeTargets.length = 0
    frameObservers = []
    Object.assign(stub, {
      roomForPane: { cols: 137, rows: 41 },
      fits: 0,
      resizeFails: false,
      webglContext: {
        getExtension: () => ({ loseContext: () => {} }),
      } as unknown as WebGL2RenderingContext | null,
      webglRefuses: false,
      webglRequests: 0,
      webglLoads: 0,
      webglDisposals: 0,
      loseWebglContext: null,
    })

    originalFetch = globalThis.fetch
    globalThis.fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const request = input instanceof Request ? input : new Request(String(input), init)
      const body = JSON.parse(await request.clone().text())
      if (request.url.endsWith("/resize")) {
        if (stub.resizeFails) return new Response(null, { status: 409 })
        resizes.push(body)
        resizeTargets.push(request.url.split("/sessions/")[1]?.replace("/resize", "") ?? "")
      } else {
        keystrokes.push(body.data)
      }
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
    // Real enough to be driven: the terminal fits itself to what this reports,
    // and a frame that moves twice in quick succession is the whole point of
    // the debounce.
    vi.stubGlobal(
      "ResizeObserver",
      class {
        readonly #notify: (typeof frameObservers)[number]

        constructor(notify: (typeof frameObservers)[number]) {
          this.#notify = notify
        }

        observe(): void {
          frameObservers.push(this.#notify)
        }

        unobserve(): void {}

        disconnect(): void {
          frameObservers = frameObservers.filter((notify) => notify !== this.#notify)
        }
      },
    )
    // jsdom has no canvas at all, and says so on every call; the answer to the
    // one question the component asks it is what decides the renderer.
    HTMLCanvasElement.prototype.getContext = ((id: string) =>
      id === "webgl2" ? stub.webglContext : null) as typeof HTMLCanvasElement.prototype.getContext
  })

  afterEach(() => {
    HTMLCanvasElement.prototype.getContext = realGetContext
    globalThis.fetch = originalFetch
    setApiBaseUrl(DEFAULT_BASE_URL)
  })
}

/** Tell the terminal its frame is now this big, the way the browser would. */
export function frameResizedTo(width: number, height: number): void {
  for (const notify of frameObservers) notify([{ contentRect: { width, height } }])
}

/** xterm's own input, which is what a focused terminal actually is. */
export function keyboardOf(container: HTMLElement): HTMLTextAreaElement {
  const textarea = container.querySelector("textarea")
  if (!textarea) throw new Error("the emulator has no keyboard")
  return textarea
}

/** The one emulator on the page — there is never a second one. */
export function emulator(): HTMLElement {
  const found = document.querySelectorAll<HTMLElement>(".xterm")
  const only = found.length === 1 ? found[0] : undefined
  if (!only) throw new Error(`expected one emulator on the page, found ${found.length}`)
  return only
}

/** A turn of the event loop: long enough for the addon's import to have landed. */
export function settle(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0)
  })
}
