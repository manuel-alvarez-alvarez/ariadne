/**
 * The WebSocket under `GET /v1/sessions/{id}/console/terminal` — a session's
 * console as a terminal (008).
 *
 * The daemon runs the console itself and writes what a terminal would read:
 * binary frames of escape sequences, drawn for the size the client said. The
 * client speaks JSON text frames — its size first, then every key and every
 * paste — and reads one text frame beside the bytes, the session's status,
 * as the socket opens and as the console ends. The shapes here are
 * `ariadne-api`'s `TerminalClientMessage` and `TerminalServerMessage`, written
 * out by hand: a WebSocket has no body for the OpenAPI document to name them
 * in, so the generated schema carries only the path.
 *
 * Nothing drawn is kept on the daemon's side: every connection opens with the
 * console redrawn whole for the emulator's size, and the emulator's scrollback
 * is the only history there is. A socket the daemon closes on purpose — the
 * session ended, Ctrl-C twice, Ctrl-D — arrives as a clean close and is the
 * end of this console; anything else is a drop, retried on the backoff the
 * event stream uses (`events/reconnecting-stream.ts`). The two cannot be told
 * apart by the daemon going away, which ends in a drop, not a close frame.
 */

import type { SessionStatus } from "@/api"
import { normalizeBaseUrl } from "@/api"
import { INITIAL_BACKOFF_MS, parsePayload } from "@/events/reconnecting-stream"

/** The key a `key` message names: crossterm's `KeyCode`, as far as the console reads it. */
export type TerminalKey =
  | { char: string }
  | { f: number }
  | "enter"
  | "backspace"
  | "tab"
  | "back_tab"
  | "esc"
  | "left"
  | "right"
  | "up"
  | "down"
  | "home"
  | "end"
  | "page_up"
  | "page_down"
  | "delete"
  | "insert"

/** A modifier held with a key: crossterm's `KeyModifiers`, one flag each. */
export type TerminalModifier = "shift" | "control" | "alt" | "super" | "hyper" | "meta"

/** What the client sends, one per JSON text frame. */
export type TerminalClientMessage =
  | { type: "resize"; cols: number; rows: number }
  | { type: "key"; code: TerminalKey; modifiers: TerminalModifier[] }
  | { type: "paste"; text: string }

/** What the daemon sends as a text frame, beside the binary frames of bytes. */
type TerminalServerMessage = { type: "status"; status: SessionStatus }

export type TerminalSocketStatus =
  /** Opening the first connection. */
  | "connecting"
  /** Connected; bytes are flowing. */
  | "live"
  /** Dropped; a retry is scheduled. */
  | "reconnecting"
  /** The daemon closed the console on purpose. Nothing dials again until asked. */
  | "closed"

const MAX_BACKOFF_MS = 5_000

interface TerminalSocketHandlers {
  /** The connection is up. The size goes first, and it goes from here. */
  onOpen: () => void
  /** One binary frame: terminal bytes, to be written as they are. */
  onBytes: (bytes: Uint8Array) => void
  /** One text frame from the daemon. */
  onMessage: (message: TerminalServerMessage) => void
  onStatus: (status: TerminalSocketStatus, error?: string | null) => void
}

/** URL of one session's terminal socket on the given daemon. */
export function terminalUrl(baseUrl: string, sessionId: string): string {
  const url = new URL(
    `/v1/sessions/${encodeURIComponent(sessionId)}/console/terminal`,
    `${normalizeBaseUrl(baseUrl)}/`,
  )
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:"
  return url.toString()
}

export class TerminalSocket {
  #socket: WebSocket | null = null
  #retryTimer: ReturnType<typeof setTimeout> | null = null
  #backoff = INITIAL_BACKOFF_MS
  #stopped = true
  #everOpened = false
  #url: string
  #handlers: TerminalSocketHandlers

  constructor(url: string, handlers: TerminalSocketHandlers) {
    this.#url = url
    this.#handlers = handlers
  }

  /**
   * Connect, and keep reconnecting through drops until {@link stop} or a
   * close the daemon meant. Idempotent, and a fresh run each time: a start
   * after a close reports `connecting`, not `reconnecting`.
   */
  start(): void {
    if (!this.#stopped) return
    this.#stopped = false
    this.#everOpened = false
    this.#connect()
  }

  /** Disconnect and cancel any pending retry. Idempotent. */
  stop(): void {
    this.#stopped = true
    this.#clearRetry()
    this.#closeSocket()
  }

  /** Send one message, if the socket is open; `false` says it was not. */
  send(message: TerminalClientMessage): boolean {
    const socket = this.#socket
    if (socket === null || socket.readyState !== WebSocket.OPEN) return false
    socket.send(JSON.stringify(message))
    return true
  }

  #connect(): void {
    if (this.#stopped) return
    this.#closeSocket()
    this.#handlers.onStatus(this.#everOpened ? "reconnecting" : "connecting")

    let socket: WebSocket
    try {
      socket = new WebSocket(this.#url)
    } catch (cause) {
      this.#scheduleRetry(cause instanceof Error ? cause.message : String(cause))
      return
    }
    socket.binaryType = "arraybuffer"
    this.#socket = socket

    socket.onopen = () => {
      this.#backoff = INITIAL_BACKOFF_MS
      this.#everOpened = true
      this.#handlers.onStatus("live", null)
      this.#handlers.onOpen()
    }

    socket.onmessage = (event: { data: unknown }) => {
      const { data } = event
      if (data instanceof ArrayBuffer) {
        this.#handlers.onBytes(new Uint8Array(data))
        return
      }
      if (typeof data !== "string") return
      const message = parsePayload(data, "terminal-socket")
      if (message !== undefined) this.#handlers.onMessage(message as TerminalServerMessage)
    }

    // Every failure ends in a close: a refused dial, a daemon that went away
    // mid-console, and the daemon's own hang-up all arrive here, and only the
    // last one carries a close frame.
    socket.onclose = (event: { wasClean: boolean }) => {
      if (this.#socket !== socket) return
      this.#socket = null
      if (event.wasClean) {
        this.#stopped = true
        this.#handlers.onStatus("closed", null)
        return
      }
      this.#scheduleRetry("terminal socket disconnected")
    }
  }

  #scheduleRetry(error: string): void {
    if (this.#stopped || this.#retryTimer !== null) return
    this.#closeSocket()
    this.#handlers.onStatus("reconnecting", error)
    // Jitter so a daemon restart does not get a thundering herd of windows.
    const delay = this.#backoff * (0.5 + Math.random() / 2)
    this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS)
    this.#retryTimer = setTimeout(() => {
      this.#retryTimer = null
      this.#connect()
    }, delay)
  }

  #clearRetry(): void {
    if (this.#retryTimer === null) return
    clearTimeout(this.#retryTimer)
    this.#retryTimer = null
  }

  #closeSocket(): void {
    if (this.#socket === null) return
    const socket = this.#socket
    this.#socket = null
    socket.onopen = null
    socket.onmessage = null
    socket.onclose = null
    socket.onerror = null
    socket.close()
  }
}
