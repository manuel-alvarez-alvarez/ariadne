/**
 * Tests for the two things this stream has to get right, both of which are
 * invisible until they are wrong:
 *
 * - a reconnect delivers a *snapshot*, not a continuation, so the consumer has
 *   to be told to start over rather than append;
 * - `end` means the session is over. The daemon closes the connection right
 *   after it, and retrying that close would reopen a finished stream forever.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { SessionLogStream, type SessionLogStreamHandlers, sessionLogStreamUrl } from "./log-stream"
import { SNAPSHOT_PREFIX, writeDelta, writeResize, writeSnapshot } from "./terminal-sink"

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
    onResize: vi.fn(),
    onSnapshot: vi.fn(),
    onDelta: vi.fn(),
    onEnd: vi.fn(),
    onStatus: vi.fn(),
  } satisfies SessionLogStreamHandlers
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

let stream: SessionLogStream | null = null

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
  stream = new SessionLogStream("http://127.0.0.1:7676/v1/sessions/01S/logs/stream", spies)
  return stream
}

const statuses = (spies: ReturnType<typeof handlers>) =>
  spies.onStatus.mock.calls.map(([status]) => status)

describe("sessionLogStreamUrl", () => {
  it("builds the endpoint on the configured daemon", () => {
    expect(sessionLogStreamUrl("http://127.0.0.1:7676/", "01SESSION")).toBe(
      "http://127.0.0.1:7676/v1/sessions/01SESSION/logs/stream",
    )
  })
})

describe("SessionLogStream payloads", () => {
  it("unwraps snapshot and delta chunks", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().emit("snapshot", { chunk: "[32mhello[0m\n" })
    latest().emit("delta", { chunk: "more" })

    expect(spies.onSnapshot).toHaveBeenCalledWith("[32mhello[0m\n")
    expect(spies.onDelta).toHaveBeenCalledWith("more")
  })

  it("reports the pane's grid", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().emit("resize", { cols: 120, rows: 40 })

    expect(spies.onResize).toHaveBeenCalledWith({ cols: 120, rows: 40 })
  })

  it("ignores a grid that is not one", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    // Drawing at zero columns is worse than drawing at the size we have.
    latest().emit("resize", { cols: 0, rows: 24 })
    latest().emit("resize", { cols: "80", rows: 24 })

    expect(spies.onResize).not.toHaveBeenCalled()
  })

  it("drops an unparseable chunk instead of throwing", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    vi.spyOn(console, "error").mockImplementation(() => {})

    for (const handler of latest().listeners.get("delta") ?? []) {
      handler({ data: "{not json" })
    }

    expect(spies.onDelta).not.toHaveBeenCalled()
  })
})

describe("SessionLogStream reconnection", () => {
  it("reconnects after a drop and reports the new connection as a snapshot", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    latest().emit("snapshot", { chunk: "before" })

    latest().fail()
    advancePastBackoff()
    expect(sources()).toHaveLength(2)
    latest().succeed()
    latest().emit("snapshot", { chunk: "after" })

    // Two snapshots, no deltas: the consumer has to clear and rewrite, not
    // append, or the reconnect would invent output the agent never printed.
    expect(spies.onSnapshot.mock.calls).toEqual([["before"], ["after"]])
    expect(statuses(spies)).toContain("reconnecting")
  })

  it("stops for good on the end event", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    const source = latest()
    source.emit("end", { session_id: "01S" })
    // What the daemon does next: it hangs up on a stream it has finished.
    source.fail()
    advancePastBackoff()

    expect(spies.onEnd).toHaveBeenCalledTimes(1)
    expect(statuses(spies).at(-1)).toBe("ended")
    expect(sources()).toHaveLength(1)
  })

  it("re-reads a finished log on restart", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().succeed()
    latest().emit("end", { session_id: "01S" })

    s.restart()
    latest().succeed()
    latest().emit("snapshot", { chunk: "the whole log" })

    expect(sources()).toHaveLength(2)
    expect(spies.onSnapshot).toHaveBeenCalledWith("the whole log")
  })

  it("stops retrying after stop()", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().fail()

    s.stop()
    advancePastBackoff()

    expect(sources()).toHaveLength(1)
  })
})

describe("SessionLogStream feeding a terminal", () => {
  /** The wiring `SessionTerminal` uses, against a terminal that only records. */
  function terminalStream() {
    const terminal = {
      write: vi.fn<(data: string, callback?: () => void) => void>(),
      resize: vi.fn(),
      reset: vi.fn(),
    }
    stream = new SessionLogStream("http://127.0.0.1:7676/v1/sessions/01S/logs/stream", {
      onResize: ({ cols, rows }) => writeResize(terminal, cols, rows),
      onSnapshot: (chunk) => writeSnapshot(terminal, chunk),
      onDelta: (chunk) => writeDelta(terminal, chunk),
      onEnd: vi.fn(),
      onStatus: vi.fn(),
    })
    return { terminal, stream }
  }

  it("replaces the terminal on reconnect, in order with what the old connection queued", () => {
    const { terminal, stream: s } = terminalStream()
    s.start()
    latest().succeed()
    latest().emit("snapshot", { chunk: "first snapshot\r\n" })
    latest().emit("delta", { chunk: "output before the drop\r\n" })
    // A chunk cut off mid-sequence is exactly what a dropped connection leaves
    // behind, and it is what a bare `reset()` would fail to recover from.
    latest().emit("delta", { chunk: "truncated \x1b[3" })

    latest().fail()
    advancePastBackoff()
    latest().succeed()
    latest().emit("snapshot", { chunk: "second snapshot\r\n" })

    expect(terminal.write.mock.calls.map(([data]) => data)).toEqual([
      `${SNAPSHOT_PREFIX}first snapshot\r\n`,
      "output before the drop\r\n",
      "truncated \x1b[3",
      // The reset rides along with the replacement snapshot, after everything
      // the dropped connection had written, and cancels the unfinished
      // sequence it left in the parser.
      `${SNAPSHOT_PREFIX}second snapshot\r\n`,
    ])
    expect(terminal.reset).not.toHaveBeenCalled()
  })

  it("queues the pane's grid behind the output that was drawn at the old one", () => {
    const { terminal, stream: s } = terminalStream()
    s.start()
    latest().succeed()
    latest().emit("snapshot", { chunk: "80 columns wide\r\n" })
    latest().emit("delta", { chunk: "still 80 columns wide\r\n" })
    latest().emit("resize", { cols: 120, rows: 40 })

    // Writes are parsed asynchronously, so a resize applied the moment it
    // arrives would lay the two chunks above out at a width they were never
    // drawn for. It goes into the stream instead, as an empty write whose
    // callback runs once everything before it has been parsed.
    expect(terminal.resize).not.toHaveBeenCalled()
    const [data, applyResize] = terminal.write.mock.calls.at(-1) ?? []
    expect(data).toBe("")
    applyResize?.()
    expect(terminal.resize).toHaveBeenCalledWith(120, 40)
  })

  it("takes a resized pane's replacement screen as a replacement, at the new grid", () => {
    const { terminal, stream: s } = terminalStream()
    s.start()
    latest().succeed()
    latest().emit("resize", { cols: 80, rows: 24 })
    latest().emit("snapshot", { chunk: "80 columns\r\n" })

    // Somebody attached with a wider terminal: the daemon starts the client
    // over rather than continuing with deltas drawn at neither size.
    latest().emit("resize", { cols: 120, rows: 40 })
    latest().emit("snapshot", { chunk: "120 columns\r\n" })

    // The resize is queued ahead of the screen it applies to, and the screen
    // brings its own reset — so nothing of the 80-column one survives, and
    // none of it is laid out at 120 columns on the way out.
    expect(terminal.write.mock.calls.map(([data]) => data)).toEqual([
      "",
      `${SNAPSHOT_PREFIX}80 columns\r\n`,
      "",
      `${SNAPSHOT_PREFIX}120 columns\r\n`,
    ])
    for (const [, apply] of terminal.write.mock.calls) apply?.()
    expect(terminal.resize.mock.calls).toEqual([
      [80, 24],
      [120, 40],
    ])
  })

  it("re-reads a finished log without carrying the old contents over", () => {
    const { terminal, stream: s } = terminalStream()
    s.start()
    latest().succeed()
    latest().emit("snapshot", { chunk: "the log\r\n" })
    latest().emit("end", { session_id: "01S" })

    s.restart()
    latest().succeed()
    latest().emit("snapshot", { chunk: "the log again\r\n" })

    expect(terminal.write.mock.calls.at(-1)).toEqual([`${SNAPSHOT_PREFIX}the log again\r\n`])
  })
})
