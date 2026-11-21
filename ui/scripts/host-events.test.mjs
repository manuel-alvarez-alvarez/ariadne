// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, waitFor } from "@testing-library/react"
import { createElement } from "react"
import { afterEach, expect, it, vi } from "vitest"
import { qk, rememberHosts } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { FakeEventSource, stubEventSource } from "@/test/event-source"
import { BOX, forgetHosts, LOCAL, stubHosts } from "@/test/hosts"

afterEach(() => {
  cleanup()
  forgetHosts()
  vi.unstubAllGlobals()
})

it("closes a deleted host stream and removes its cache only from local events", async () => {
  stubEventSource()
  stubHosts({})
  rememberHosts([LOCAL, BOX])
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  client.setQueryData(qk.hosts.list(), [LOCAL, BOX])
  client.setQueryData(qk.goals.list("box"), [{ id: "worker-goal" }])
  client.setQueryData(qk.goals.list("local"), [{ id: "local-goal" }])
  render(createElement(QueryClientProvider, { client }, createElement(EventStreamProvider)))
  await waitFor(() => expect(FakeEventSource.instances).toHaveLength(2))
  const local = FakeEventSource.instances.find((source) => !source.url.includes("box.test"))
  const box = FakeEventSource.instances.find((source) => source.url.includes("box.test"))
  act(() => box.emit("host_deleted", BOX))
  expect(client.getQueryData(qk.hosts.list())).toEqual([LOCAL, BOX])
  expect(box.closed).toBe(false)
  act(() => local.emit("host_deleted", BOX))
  await waitFor(() => expect(box.closed).toBe(true))
  expect(client.getQueryData(qk.hosts.list())).toEqual([LOCAL])
  expect(client.getQueryData(qk.goals.list("box"))).toBeUndefined()
  expect(client.getQueryData(qk.goals.list("local"))).toEqual([{ id: "local-goal" }])
})
