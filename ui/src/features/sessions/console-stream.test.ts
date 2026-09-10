/**
 * The one thing this stream has to get right, and the one thing invisible
 * until it is wrong: a reconnect delivers a fresh *snapshot*, never a
 * continuation, so the consumer has to be told to replace what it is showing
 * rather than append to it.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { anAgentEvent } from "@/test/fixtures"
import { ConsoleStream, type ConsoleStreamHandlers, consoleStreamUrl } from "./console-stream"

function handlers() {
  return {
    onSnapshot: vi.fn(),
    onEvent: vi.fn(),
    onStatus: vi.fn(),
  } satisfies ConsoleStreamHandlers
}

const sources = () => FakeEventSource.instances
const latest = latestSource

function advancePastBackoff() {
  vi.advanceTimersByTime(60_000)
}

let stream: ConsoleStream | null = null

beforeEach(() => {
  vi.useFakeTimers()
  stubEventSource()
})

afterEach(() => {
  stream?.stop()
  stream = null
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

function makeStream(spies: ReturnType<typeof handlers>) {
  stream = new ConsoleStream("http://127.0.0.1:7676/v1/sessions/01S/console/stream", spies)
  return stream
}

const statuses = (spies: ReturnType<typeof handlers>) =>
  spies.onStatus.mock.calls.map(([status]) => status)

describe("consoleStreamUrl", () => {
  it("builds the endpoint on the configured daemon", () => {
    expect(consoleStreamUrl("http://127.0.0.1:7676/", "01SESSION")).toBe(
      "http://127.0.0.1:7676/v1/sessions/01SESSION/console/stream",
    )
  })
})

describe("ConsoleStream payloads", () => {
  it("delivers the snapshot, then a later event as a delta", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    const before = anAgentEvent({ id: "01E1", kind: "session_start" })
    const after = anAgentEvent({ id: "01E2", kind: "stop" })
    latest().emit("snapshot", [before])
    latest().emit("event", after)

    expect(spies.onSnapshot).toHaveBeenCalledWith([before])
    expect(spies.onEvent).toHaveBeenCalledWith(after)
  })

  it("drops a snapshot that is not an array instead of throwing", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    vi.spyOn(console, "error").mockImplementation(() => {})

    latest().emit("snapshot", { not: "an array" })

    expect(spies.onSnapshot).not.toHaveBeenCalled()
  })

  it("drops an unparseable event instead of throwing", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    vi.spyOn(console, "error").mockImplementation(() => {})

    for (const handler of latest().listeners.get("event") ?? []) {
      handler({ data: "{not json" })
    }

    expect(spies.onEvent).not.toHaveBeenCalled()
  })
})

describe("ConsoleStream reconnection", () => {
  it("reconnects after a drop and reports the new connection as a snapshot", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    latest().emit("snapshot", [anAgentEvent({ id: "01E1" })])

    latest().fail()
    advancePastBackoff()
    expect(sources()).toHaveLength(2)
    latest().succeed()
    const fresh = [anAgentEvent({ id: "01E1" }), anAgentEvent({ id: "01E2" })]
    latest().emit("snapshot", fresh)

    // Two snapshots, and the second one replaces rather than extends the
    // first: the consumer would otherwise show 01E1 twice.
    expect(spies.onSnapshot.mock.calls.at(-1)).toEqual([fresh])
    expect(statuses(spies)).toContain("reconnecting")
  })

  it("also reconnects after the daemon's own resync hang-up", () => {
    // The daemon closes the connection right after a `resync` event; there is
    // no dedicated listener for it here — the close that follows is reported
    // through `onerror`, same as any other drop, and the retry's fresh
    // snapshot is already the correct recovery.
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().fail()
    advancePastBackoff()

    expect(sources()).toHaveLength(2)
    expect(statuses(spies)).toContain("reconnecting")
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
