/**
 * The things the daemon-log client has to get right on its own, because no
 * consumer can see them go wrong: a snapshot replaces (a reconnect would
 * otherwise duplicate the whole buffer), the cap holds against both a huge
 * snapshot and an endless run of deltas, and `stop()` really lets go of the
 * connection — the drawer closing is the only disconnect this stream gets.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { LogLineDto } from "@/api"

import {
  DAEMON_LOG_LINE_CAP,
  DaemonLogStream,
  type DaemonLogStreamHandlers,
  daemonLogStreamUrl,
} from "./daemon-log-stream"

/** Minimal stand-in for the browser's `EventSource`, driven by the tests. */
class FakeEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2

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

  succeed(): void {
    this.readyState = FakeEventSource.OPEN
    this.onopen?.()
  }

  fail(): void {
    this.readyState = FakeEventSource.CLOSED
    this.onerror?.()
  }

  emit(type: string, data: unknown): void {
    for (const handler of this.listeners.get(type) ?? []) {
      handler({ data: JSON.stringify(data) })
    }
  }
}

function handlers() {
  return {
    onLines: vi.fn(),
    onStatus: vi.fn(),
  } satisfies DaemonLogStreamHandlers
}

const sources = () => FakeEventSource.instances
const latest = () => {
  const source = sources().at(-1)
  if (!source) throw new Error("no EventSource was created")
  return source
}

/** Run out the backoff timer so the scheduled retry connects. */
function advancePastBackoff() {
  vi.advanceTimersByTime(60_000)
}

function line(message: string, over: Partial<LogLineDto> = {}): LogLineDto {
  return {
    ts: "2026-08-18T12:00:00.000000Z",
    level: "INFO",
    target: "ariadne_daemon::scheduler",
    message,
    ...over,
  }
}

let stream: DaemonLogStream | null = null

beforeEach(() => {
  vi.useFakeTimers()
  FakeEventSource.instances = []
  vi.stubGlobal("EventSource", FakeEventSource)
})

afterEach(() => {
  stream?.stop()
  stream = null
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function makeStream(spies: ReturnType<typeof handlers>) {
  stream = new DaemonLogStream("http://127.0.0.1:7676/v1/logs/stream", spies)
  return stream
}

/** Messages of the retained lines as of the last `onLines` call. */
const retained = (spies: ReturnType<typeof handlers>): string[] =>
  (spies.onLines.mock.lastCall?.[0] ?? []).map((l: LogLineDto) => l.message)

const statuses = (spies: ReturnType<typeof handlers>) =>
  spies.onStatus.mock.calls.map(([status]) => status)

describe("daemonLogStreamUrl", () => {
  it("builds the endpoint on the configured daemon", () => {
    expect(daemonLogStreamUrl("http://127.0.0.1:7676/")).toBe(
      "http://127.0.0.1:7676/v1/logs/stream",
    )
  })
})

describe("DaemonLogStream payloads", () => {
  it("hands over the snapshot, then appends deltas to it", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().emit("snapshot", { lines: [line("one"), line("two")] })
    expect(retained(spies)).toEqual(["one", "two"])

    latest().emit("delta", line("three"))
    latest().emit("delta", line("four"))
    expect(retained(spies)).toEqual(["one", "two", "three", "four"])
    expect(statuses(spies)).toEqual(["connecting", "live"])
  })

  it("hands out a fresh array per change, so consumers can treat it as state", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().emit("snapshot", { lines: [line("one")] })
    latest().emit("delta", line("two"))

    const [[first], [second]] = spies.onLines.mock.calls
    expect(first).not.toBe(second)
    expect(first).toEqual([line("one")])
  })

  it("drops malformed lines and unparseable payloads instead of throwing", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    vi.spyOn(console, "error").mockImplementation(() => {})

    latest().emit("snapshot", { lines: [line("good"), { level: "INFO" }, "junk"] })
    latest().emit("delta", { not: "a line" })
    for (const handler of latest().listeners.get("delta") ?? []) {
      handler({ data: "{not json" })
    }

    expect(retained(spies)).toEqual(["good"])
  })
})

describe("DaemonLogStream line cap", () => {
  it("keeps only the newest lines of an oversized snapshot", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    const lines = Array.from({ length: DAEMON_LOG_LINE_CAP + 5 }, (_, i) => line(`${i}`))
    latest().emit("snapshot", { lines })

    const kept = retained(spies)
    expect(kept).toHaveLength(DAEMON_LOG_LINE_CAP)
    expect(kept[0]).toBe("5")
    expect(kept.at(-1)).toBe(`${DAEMON_LOG_LINE_CAP + 4}`)
  })

  it("drops the oldest line when a delta overflows the cap", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    const lines = Array.from({ length: DAEMON_LOG_LINE_CAP }, (_, i) => line(`${i}`))
    latest().emit("snapshot", { lines })
    latest().emit("delta", line("overflow"))

    const kept = retained(spies)
    expect(kept).toHaveLength(DAEMON_LOG_LINE_CAP)
    expect(kept[0]).toBe("1")
    expect(kept.at(-1)).toBe("overflow")
  })
})

describe("DaemonLogStream lifecycle", () => {
  it("closes the connection on stop and does not retry", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().succeed()

    s.stop()
    advancePastBackoff()

    expect(latest().closed).toBe(true)
    expect(sources()).toHaveLength(1)
  })

  it("reconnects after a drop and takes the new snapshot as a replacement", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    latest().emit("snapshot", { lines: [line("before")] })

    latest().fail()
    advancePastBackoff()
    expect(sources()).toHaveLength(2)
    latest().succeed()
    latest().emit("snapshot", { lines: [line("after")] })

    // A replacement, not an append: the daemon replays nothing, so the old
    // buffer is the new snapshot's prefix at best and stale at worst.
    expect(retained(spies)).toEqual(["after"])
    expect(statuses(spies)).toContain("reconnecting")
  })

  it("stops retrying after stop() while disconnected", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().fail()

    s.stop()
    advancePastBackoff()

    expect(sources()).toHaveLength(1)
  })
})
