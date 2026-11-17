// @vitest-environment jsdom

/**
 * One domain stream per reachable host, each dispatching under its own host's
 * keys — and the connection state still `local`'s alone.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it } from "vitest"

import { qk } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { useConnection } from "@/hooks/use-connection"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { aGoal } from "@/test/fixtures"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

const FAR = { ...BOX, name: "far", endpoint: null, kind: "ssh" as const, status: "down" as const }

beforeEach(() => {
  stubEventSource()
})

afterEach(() => {
  forgetHosts()
})

function Probe() {
  return <p data-testid="conn">{useConnection().status}</p>
}

function mount(): QueryClient {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <EventStreamProvider>
        <Probe />
      </EventStreamProvider>
    </QueryClientProvider>,
  )
  return client
}

/** The stream opened on one origin. */
function sourceOn(origin: string): FakeEventSource | undefined {
  return FakeEventSource.instances.find((source) => new URL(source.url).origin === origin)
}

it("opens one stream per reachable host, and none for a host that is down", async () => {
  stubHosts({}, [LOCAL, BOX, FAR])
  mount()

  await waitFor(() => expect(sourceOn("http://box.test:7676")).toBeDefined())
  expect(FakeEventSource.instances.map((source) => source.url).sort()).toEqual([
    "http://127.0.0.1:7676/v1/events/stream",
    "http://box.test:7676/v1/events/stream",
  ])
})

it("dispatches a named host's event under that host's keys", async () => {
  stubHosts({}, [LOCAL, BOX])
  const client = mount()
  await waitFor(() => expect(sourceOn("http://box.test:7676")).toBeDefined())
  const goal = aGoal({ id: "g-box", title: "On box" })

  act(() => {
    const box = sourceOn("http://box.test:7676")
    box?.succeed()
    box?.emit("goal_created", goal)
  })

  expect(client.getQueryData(qk.goals.detail("box", goal.id))).toEqual(goal)
  expect(client.getQueryData(qk.goals.detail("local", goal.id))).toBeUndefined()
})

it("keeps the connection state about local while a named host's stream drops", async () => {
  stubHosts({}, [LOCAL, BOX])
  mount()
  await waitFor(() => expect(sourceOn("http://box.test:7676")).toBeDefined())

  act(() => {
    sourceOn("http://127.0.0.1:7676")?.succeed()
    sourceOn("http://box.test:7676")?.fail()
  })

  expect(screen.getByTestId("conn").textContent).toBe("connected")
})
