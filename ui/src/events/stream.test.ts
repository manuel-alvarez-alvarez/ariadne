/**
 * Tests for the two things in this layer that are easy to get subtly wrong.
 *
 * When an open counts as a *reconnect*, because the daemon has no replay: every
 * open that follows a gap has to tell the caller so, or `EventStreamProvider`
 * skips its full invalidation and the screens keep showing whatever they last
 * fetched.
 *
 * And the idle budget, because nothing else notices a daemon that went away —
 * the socket stays `OPEN` and no `error` ever fires. Every frame has to re-arm
 * it and a silence longer than it has to end the connection.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { DomainEventStream, type DomainEventStreamHandlers } from "./stream"

function handlers() {
  return {
    onEvent: vi.fn(),
    onResync: vi.fn(),
    onHeartbeat: vi.fn(),
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

/** The stream's idle budget: two and a half of the daemon's 15 s beats. */
const IDLE_BUDGET_MS = 37_500

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

    // What the idle budget does when the beats stop: the socket can still look
    // OPEN even though nothing is coming through it.
    s.forceReconnect("no heartbeat from the daemon")
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

describe("DomainEventStream idle budget", () => {
  it("reconnects, saying why, once the daemon stops beating", () => {
    const spies = handlers()
    makeStream(spies).start()
    const first = latest()
    first.succeed()

    // The socket never errors — `ariadned` keeps it open through its own
    // shutdown — so the silence is the only evidence there is.
    vi.advanceTimersByTime(IDLE_BUDGET_MS)

    expect(first.closed).toBe(true)
    expect(sources()).toHaveLength(2)
    expect(spies.onStatus).toHaveBeenLastCalledWith("reconnecting", "no heartbeat from the daemon")
  })

  it("does not start counting before the connection is open", () => {
    const spies = handlers()
    makeStream(spies).start()

    // Still CONNECTING: a daemon that has said nothing yet owes us nothing.
    vi.advanceTimersByTime(IDLE_BUDGET_MS * 2)

    expect(sources()).toHaveLength(1)
  })

  it.each([
    ["a heartbeat", (source: ReturnType<typeof latest>) => source.beat()],
    [
      "a domain event",
      (source: ReturnType<typeof latest>) => source.emit("task_updated", { id: "01T" }),
    ],
    ["a resync", (source: ReturnType<typeof latest>) => source.emit("resync", { missed: 2 })],
  ])("is re-armed by %s, whatever it carried", (_label, deliver) => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    vi.advanceTimersByTime(IDLE_BUDGET_MS - 1_000)
    deliver(latest())
    vi.advanceTimersByTime(IDLE_BUDGET_MS - 1_000)

    // Two budgets have passed in total, but never one without a frame.
    expect(sources()).toHaveLength(1)

    vi.advanceTimersByTime(1_000)
    expect(sources()).toHaveLength(2)
  })

  it("gives the new connection a budget of its own", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    vi.advanceTimersByTime(IDLE_BUDGET_MS)
    expect(sources()).toHaveLength(2)
    latest().succeed()
    latest().beat()

    vi.advanceTimersByTime(IDLE_BUDGET_MS - 1_000)
    expect(sources()).toHaveLength(2)
    vi.advanceTimersByTime(1_000)
    expect(sources()).toHaveLength(3)
  })

  it("stops counting once the stream is stopped", () => {
    const spies = handlers()
    const s = makeStream(spies)
    s.start()
    latest().succeed()

    s.stop()
    vi.advanceTimersByTime(IDLE_BUDGET_MS * 2)

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

  it("surfaces the heartbeat, and never as a domain event", () => {
    const spies = handlers()
    makeStream(spies).start()
    latest().succeed()

    latest().beat({ version: "0.4.0", started_at: "2026-08-29T09:00:00Z" })

    expect(spies.onHeartbeat).toHaveBeenCalledWith({
      version: "0.4.0",
      started_at: "2026-08-29T09:00:00Z",
    })
    expect(spies.onEvent).not.toHaveBeenCalled()
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
