/**
 * The browser's `WebSocket`, as the tests drive it.
 *
 * One socket is opened in this app — a session's console as a terminal — and
 * what a test needs of it is what `event-source.ts` gives the streams: the
 * connections that were opened, in order, what the client sent on each, and
 * the things a server does to one (accept it, send on it, close it, drop it).
 *
 * Install it with {@link stubWebSocket} in a `beforeEach`, which also clears
 * the connections the previous test left behind.
 */

import { vi } from "vitest"

export class FakeWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  /** Every connection opened since the last {@link stubWebSocket}, in order. */
  static instances: FakeWebSocket[] = []

  readyState = FakeWebSocket.CONNECTING
  binaryType: "blob" | "arraybuffer" = "blob"
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null
  onerror: (() => void) | null = null
  /** Every frame the client sent, in order. */
  readonly sent: (string | ArrayBufferLike | ArrayBufferView)[] = []
  readonly url: string

  constructor(url: string) {
    this.url = url
    FakeWebSocket.instances.push(this)
  }

  send(data: string | ArrayBufferLike | ArrayBufferView): void {
    this.sent.push(data)
  }

  /** Closed by the client. Nothing is reported back: the client asked. */
  close(): void {
    this.readyState = FakeWebSocket.CLOSED
  }

  /** The server accepted the connection. */
  succeed(): void {
    this.readyState = FakeWebSocket.OPEN
    this.onopen?.()
  }

  /** A binary frame from the server: terminal bytes. */
  deliver(bytes: Uint8Array): void {
    const copy = new Uint8Array(bytes)
    this.onmessage?.({ data: copy.buffer })
  }

  /** A text frame from the server: one JSON message. */
  deliverText(message: unknown): void {
    this.onmessage?.({ data: JSON.stringify(message) })
  }

  /** The server closed the connection on purpose, close frame and all. */
  end(): void {
    this.readyState = FakeWebSocket.CLOSED
    this.onclose?.({ code: 1005, reason: "", wasClean: true })
  }

  /** The connection failed or dropped, with no close frame from the server. */
  drop(): void {
    this.readyState = FakeWebSocket.CLOSED
    this.onerror?.()
    this.onclose?.({ code: 1006, reason: "", wasClean: false })
  }

  /** The text frames the client sent, parsed, in order. */
  get messages(): unknown[] {
    return this.sent.flatMap((frame) => (typeof frame === "string" ? [JSON.parse(frame)] : []))
  }
}

/** Installs the stand-in and forgets the connections of the last test. */
export function stubWebSocket(): void {
  FakeWebSocket.instances = []
  vi.stubGlobal("WebSocket", FakeWebSocket)
}

/** The connection most recently opened; throws — failing the test — if none was. */
export function latestSocket(): FakeWebSocket {
  const socket = FakeWebSocket.instances.at(-1)
  if (!socket) throw new Error("no WebSocket was opened")
  return socket
}
