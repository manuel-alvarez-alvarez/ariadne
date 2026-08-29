// @vitest-environment jsdom

/**
 * The connection state, end to end: an `EventSource` on one side and what the
 * footer and the banner read on the other.
 *
 * This is the test that says the poll is gone. The daemon is asked nothing —
 * `daemonFetch` never rings — and every state the shell shows comes out of the
 * one stream: the heartbeat names the daemon, a silence longer than the idle
 * budget loses it with nobody clicking anything, and the next open finds it
 * again.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { ConnectionBanner } from "@/components/connection-banner"
import { EventStreamProvider } from "@/events/provider"
import { useConnection } from "@/hooks/use-connection"
import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch } from "@/test/setup"

/** The stream's idle budget: two and a half of the daemon's 15 s beats. */
const IDLE_BUDGET_MS = 37_500

const NOW = new Date("2026-08-29T12:00:00Z")
/** An hour before {@link NOW}, so the uptime is a round 3600. */
const STARTED_AT = "2026-08-29T11:00:00Z"

beforeEach(() => {
  vi.useFakeTimers({ now: NOW })
  stubEventSource()
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

/** The whole of `useConnection`, as one line a test can read. */
function Probe() {
  const { status, version, uptimeSecs } = useConnection()
  return <p data-testid="conn">{`${status} ${version ?? "?"} ${uptimeSecs ?? "?"}`}</p>
}

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <EventStreamProvider>
        <ConnectionBanner onOpenSettings={() => {}} />
        <Probe />
      </EventStreamProvider>
    </QueryClientProvider>,
  )
}

function connection(): string {
  return screen.getByTestId("conn").textContent ?? ""
}

/** Timers move state that React has to see: the store's, and the shared clock's. */
function advance(ms: number) {
  act(() => {
    vi.advanceTimersByTime(ms)
  })
}

/** The daemon accepts the pending connection and introduces itself. */
function daemonAccepts(version = "0.4.0", startedAt = STARTED_AT) {
  act(() => {
    latestSource().succeed()
    latestSource().beat({ version, started_at: startedAt })
  })
}

it("connects, and names the daemon from its heartbeat", () => {
  mount()
  expect(connection()).toBe("connecting ? ?")

  daemonAccepts()

  expect(connection()).toBe("connected 0.4.0 3600")
  expect(screen.queryByRole("status")).toBeNull()
})

it("keeps the uptime ticking without asking the daemon anything", () => {
  mount()
  daemonAccepts()

  advance(30_000)
  expect(connection()).toBe("connected 0.4.0 3630")
  act(() => latestSource().beat({ started_at: STARTED_AT }))
  advance(30_000)
  expect(connection()).toBe("connected 0.4.0 3660")

  // The whole point of the exercise: one connection, and no requests behind it.
  expect(FakeEventSource.instances).toHaveLength(1)
  expect(daemonFetch).not.toHaveBeenCalled()
})

it("loses the daemon when the beats stop, with nobody touching anything", () => {
  mount()
  daemonAccepts()

  // The socket is still OPEN — `ariadned` keeps it that way through its own
  // shutdown — so the silence is all there is to go on.
  advance(IDLE_BUDGET_MS)

  expect(connection()).toBe("disconnected 0.4.0 ?")
  expect(screen.getByRole("status").textContent).toContain("Daemon unreachable")
  expect(FakeEventSource.instances).toHaveLength(2)
  expect(daemonFetch).not.toHaveBeenCalled()
})

it("finds it again on the next open, with the version it reports then", () => {
  mount()
  daemonAccepts()
  advance(IDLE_BUDGET_MS)

  // A daemon that restarted: a new version, and an uptime that starts over
  // from the `started_at` this one reports (the shared clock is coarse, so it
  // reads 12:00:30 here rather than 12:00:37.5).
  daemonAccepts("0.5.0", "2026-08-29T12:00:00Z")

  expect(connection()).toBe("connected 0.5.0 30")
  expect(screen.queryByRole("status")).toBeNull()
})

it("reconnects at once when the banner's Retry is pressed", () => {
  mount()
  daemonAccepts()
  act(() => latestSource().fail())
  expect(connection()).toBe("disconnected 0.4.0 ?")
  // Nothing has been reopened yet: the retry is sitting out its backoff.
  expect(FakeEventSource.instances).toHaveLength(1)

  fireEvent.click(screen.getByRole("button", { name: "Retry" }))

  // A connection now, rather than whenever the backoff was going to expire.
  expect(FakeEventSource.instances).toHaveLength(2)
  daemonAccepts()
  expect(connection()).toBe("connected 0.4.0 3600")
})
