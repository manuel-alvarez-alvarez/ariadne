/**
 * The browser's `EventSource`, as the tests drive it.
 *
 * Four streams are tested here — the domain events, a session's log, the
 * daemon's log, and the drawer that shows the last of them — and each of them
 * had written this stand-in out for itself. What a test needs of it is the same
 * every time: the connections that were opened, in order, and the three things
 * a server does to one (accept it, drop it, send on it).
 *
 * Install it with {@link stubEventSource} in a `beforeEach`, which also clears
 * the connections the previous test left behind.
 */

import { vi } from "vitest"

export class FakeEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2

  /** Every connection opened since the last {@link stubEventSource}, in order. */
  static instances: FakeEventSource[] = []

  readyState = FakeEventSource.CONNECTING
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  closed = false
  readonly listeners = new Map<string, ((event: { data: string }) => void)[]>()
  readonly url: string

  constructor(url: string) {
    this.url = url
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, handler: (event: { data: string }) => void): void {
    const existing = this.listeners.get(type)
    if (existing) existing.push(handler)
    else this.listeners.set(type, [handler])
  }

  close(): void {
    this.readyState = FakeEventSource.CLOSED
    this.closed = true
  }

  /** The server accepted the connection. */
  succeed(): void {
    this.readyState = FakeEventSource.OPEN
    this.onopen?.()
  }

  /** The connection failed or dropped, the only thing `EventSource` reports. */
  fail(): void {
    this.readyState = FakeEventSource.CLOSED
    this.onerror?.()
  }

  emit(type: string, data: unknown): void {
    for (const handler of this.listeners.get(type) ?? []) {
      handler({ data: JSON.stringify(data) })
    }
  }

  /**
   * The daemon's `heartbeat`, which it sends on open and every 15 idle seconds.
   * A default daemon identity, because most tests care that a beat arrived at
   * all and not who sent it.
   */
  beat(daemon: { version?: string; started_at?: string } = {}): void {
    this.emit("heartbeat", {
      version: daemon.version ?? "0.4.0",
      started_at: daemon.started_at ?? "2026-01-01T00:00:00Z",
    })
  }
}

/** Installs the stand-in and forgets the connections of the last test. */
export function stubEventSource(): void {
  FakeEventSource.instances = []
  vi.stubGlobal("EventSource", FakeEventSource)
}

/** The connection most recently opened; throws — failing the test — if none was. */
export function latestSource(): FakeEventSource {
  const source = FakeEventSource.instances.at(-1)
  if (!source) throw new Error("no EventSource was opened")
  return source
}
