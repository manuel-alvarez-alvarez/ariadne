// @vitest-environment jsdom
/**
 * What the shipped client does when the window comes back.
 *
 * Nothing: the stream is the only source of freshness, so a query that holds an
 * answer holds the current one until an event, a mutation or a reconnect says
 * otherwise. This is the assertion that keeps that true — react-query refetches
 * on focus by default, and a `staleTime` anyone reintroduces here would put a
 * burst of reads back on every alt-tab, on every screen at once.
 */

import { QueryClientProvider, useQuery } from "@tanstack/react-query"
import { act, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { createQueryClient } from "./queries"
import { qk } from "./query-keys"

/** A screen with one read on it, as every screen in the app is. */
function Board({ read }: { read: () => Promise<unknown> }) {
  const { data } = useQuery({ queryKey: qk.repositories.list(), queryFn: read })
  return <span>{data ? "loaded" : "loading"}</span>
}

/** Leaving the window and coming back, as the browser reports it. */
async function leaveAndReturn(): Promise<void> {
  await act(async () => {
    window.dispatchEvent(new Event("visibilitychange"))
    window.dispatchEvent(new Event("focus"))
  })
}

describe("createQueryClient", () => {
  it("does not read again when the window regains focus", async () => {
    const read = vi.fn().mockResolvedValue([])
    render(
      <QueryClientProvider client={createQueryClient()}>
        <Board read={read} />
      </QueryClientProvider>,
    )
    // Waiting for the answer, not just for the call: a read still in flight
    // would swallow a second one and pass for the wrong reason.
    await screen.findByText("loaded")
    expect(read).toHaveBeenCalledTimes(1)

    await leaveAndReturn()

    expect(read).toHaveBeenCalledTimes(1)
  })

  it("does not read at all when the cache was seeded and nothing invalidated it", async () => {
    const read = vi.fn().mockResolvedValue([])
    const queryClient = createQueryClient()
    queryClient.setQueryData(qk.repositories.list(), [])
    render(
      <QueryClientProvider client={queryClient}>
        <Board read={read} />
      </QueryClientProvider>,
    )

    await leaveAndReturn()

    expect(read).not.toHaveBeenCalled()
  })
})
