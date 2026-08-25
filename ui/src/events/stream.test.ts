/**
 * Tests for the one thing in this layer that is easy to get subtly wrong: when
 * an open counts as a *reconnect*.
 *
 * It matters because the daemon has no replay. Every open that follows a gap
 * has to tell the caller so, or `EventStreamProvider` skips its full
 * invalidation and the screens keep showing whatever they last fetched.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { DomainEventStream, type DomainEventStreamHandlers } from "./stream"

function handlers() {
  return {
    onEvent: vi.fn(),
    onResync: vi.fn(),
    onOpen: vi.fn(),
    onStatus: vi.fn(),
  } satisfies DomainEventStreamHandlers
}

/** `reconnected` flags of every `onOpen` call so far, in order. */
function opens(spies: ReturnType<typeof handlers>): boolean[] {
  return spies.onOpen.mock.calls.map(([info]) => info.reconnected)
}

const sources = () => FakeEventSource.instances
const latest = latestSource

/** Run out the backoff timer so the scheduled retry connects. */
function advancePastBackoff() {
  vi.advanceTimersByTime(60_000)
}

let stream: DomainEventStream | null = null

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
  stream = new DomainEventStream(() => "http://127.0.0.1:7676/v1/events/stream", spies)
  return stream
}

describe("DomainEventStream open/reconnect reporting", () => {
  it("does not claim a reconnect when the first connection succeeds straight away", () => {
    const spies = handlers()
    makeStream(spies).start()

    latest().succeed()

    expect(opens(spies)).toEqual([false])
    expect(spies.onStatus).toHaveBeenCalledWith("connecting", undefined)
  })

  it("reports a reconnect when the first attempt fails before one succeeds", () => {
    const spies = handlers()
    makeStream(spies).start()

    // Daemon not up yet: the first attempt never opens.
    latest().fail()
    advancePastBackoff()
    expect(sources()).toHaveLength(2)

    latest().succeed()

    // Nothing was ever received on the failed attempt, but REST queries may
    // have loaded during the gap, so this open still has to invalidate.
    expect(opens(spies)).toEqual([true])
  })

  it("reports a reconnect after several failed first attempts", () => {
    const spies = handlers()
    makeStream(spies).start()

    for (let attempt = 0; attempt < 3; attempt++) {
      latest().fail()
      advancePastBackoff()
    }
    latest().succeed()

    expect(sources()).toHaveLength(4)
    expect(opens(spies)).toEqual([true])
  })

  it("reports a reconnect after open → disconnect → reopen", () => {
    const spies = handlers()
    makeStream(spies).start()

    latest().succeed()
    expect(opens(spies)).toEqual([false])

    latest().fail()
    advancePastBackoff()
    latest().succeed()

    expect(opens(spies)).toEqual([false, true])
  })

  it("reports a reconnect after a forced drop, even from a healthy connection", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().succeed()

    // What the health watchdog does when REST says the daemon is gone: the
    // socket can still look OPEN even though nothing is coming through it.
    s.forceReconnect("daemon health probe failed")
    expect(sources()).toHaveLength(2)
    latest().succeed()

    expect(opens(spies)).toEqual([false, true])
  })

  it("clears the reconnect flag once an open has been reported", () => {
    const spies = handlers()
    makeStream(spies).start()

    latest().fail()
    advancePastBackoff()
    latest().succeed()
    expect(opens(spies)).toEqual([true])

    // A later forced reconnect that succeeds immediately is still a reconnect,
    // but an unrelated second open must not inherit the earlier gap.
    latest().fail()
    advancePastBackoff()
    latest().succeed()
    expect(opens(spies)).toEqual([true, true])
  })

  it("says connecting on a clean start and reconnecting once an attempt failed", () => {
    const spies = handlers()
    makeStream(spies).start()

    const statuses = () => spies.onStatus.mock.calls.map(([status]) => status)
    expect(statuses()).toEqual(["connecting"])

    latest().fail()
    advancePastBackoff()

    // The retry is honestly a reconnect attempt, not a first connect.
    expect(statuses()).toEqual(["connecting", "reconnecting", "reconnecting"])
  })
})

describe("DomainEventStream connection handling", () => {
  it("closes the previous source before opening another", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    const first = latest()
    first.succeed()

    s.forceReconnect("switch")

    expect(first.closed).toBe(true)
    expect(sources()).toHaveLength(2)
  })

  it("does not interrupt a connection that is still being made", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()

    // Still CONNECTING: the health probe coming good must not tear this down.
    s.reconnectIfClosed("daemon health probe recovered")

    expect(sources()).toHaveLength(1)
    latest().succeed()
    expect(opens(spies)).toEqual([false])
  })

  it("skips the backoff wait when the source is already closed", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().fail()

    s.reconnectIfClosed("daemon health probe recovered")

    expect(sources()).toHaveLength(2)
    latest().succeed()
    expect(opens(spies)).toEqual([true])
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

describe("DomainEventStream payloads", () => {
  it("rebuilds the tagged union from the SSE event name and data", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    const goal = { id: "01GOAL", title: "ship it" }
    latest().emit("goal_updated", goal)

    expect(spies.onEvent).toHaveBeenCalledWith({ event: "goal_updated", data: goal })
  })

  it("surfaces the resync control event", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().emit("resync", { missed: 7 })

    expect(spies.onResync).toHaveBeenCalledWith({ missed: 7 })
  })

  it("drops an unparseable payload instead of throwing", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()
    vi.spyOn(console, "error").mockImplementation(() => {})

    for (const handler of latest().listeners.get("task_updated") ?? []) {
      handler({ data: "{not json" })
    }

    expect(spies.onEvent).not.toHaveBeenCalled()
  })
})
