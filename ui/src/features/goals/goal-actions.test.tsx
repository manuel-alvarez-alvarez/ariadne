// @vitest-environment jsdom

/**
 * Deleting a goal, against a stubbed daemon.
 *
 * What is worth pinning is the gate and the wire. The gate: a goal that has
 * not stopped is never offered the delete, because the daemon would refuse it
 * and there is nothing the user could do about it from here — so the row shows
 * "Cancel goal" and nothing else. The wire: the confirm is what sends
 * `DELETE /v1/goals/{id}`, once, and nothing leaves before it. And the 409,
 * which is a real answer rather than a failure — the goal went back to work
 * between the render and the click — so it stays on screen and the button
 * stops being clickable.
 *
 * jsdom is asked for by this file alone (the docblock above): the rest of the
 * goal tests are pure and have no business paying for a DOM.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { GoalDto, GoalStatus } from "@/api"

import { GoalActions } from "./goal-actions"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is a stub the daemon client never sees, and the test would go looking
 * for a real daemon.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const STAMP = "2026-01-01T00:00:00Z"

function goal(status: GoalStatus): GoalDto {
  return {
    id: "01JGOAL0000000000000000001",
    title: "Ship the board",
    description: "",
    planner_profile_id: "01JPROF00000000000000PLAN",
    repos: [],
    required_approvals: 1,
    status,
    created_at: STAMP,
    updated_at: STAMP,
  }
}

/** `DELETE /v1/goals/{id}` answers this instead of 204, when set. */
let deleteFailure: { status: number; code: string; message: string } | null = null

function stubDaemon() {
  deleteFailure = null
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(String(input), init)
    if (request.method === "DELETE" && deleteFailure) {
      const { status, ...error } = deleteFailure
      return new Response(JSON.stringify({ error }), {
        status,
        headers: { "content-type": "application/json" },
      })
    }
    return new Response(null, { status: 204 })
  })
}

/** Every request the daemon client made, oldest first. */
function wire(): { method: string; path: string }[] {
  return daemonFetch.mock.calls.map((call) => {
    const [input, init] = call as [Request | string | URL, RequestInit?]
    const request = input instanceof Request ? input : new Request(String(input), init)
    return { method: request.method, path: new URL(request.url).pathname }
  })
}

function renderActions(status: GoalStatus, onDeleted = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  render(
    <QueryClientProvider client={queryClient}>
      <GoalActions goal={goal(status)} onDeleted={onDeleted} />
    </QueryClientProvider>,
  )
  return { onDeleted }
}

/** Both the trigger and the confirming click say "Delete goal". */
async function confirmDelete() {
  const dialog = await screen.findByRole("dialog")
  return within(dialog).getByRole("button", { name: "Delete goal" })
}

beforeEach(() => {
  // jsdom lays nothing out, so it implements neither of these.
  Element.prototype.scrollIntoView = vi.fn()
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  daemonFetch.mockReset()
  stubDaemon()
})

// Testing Library only unmounts by itself under `globals: true`, which this
// project does not use — without this every screen stays in the document.
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe("deleting a goal", () => {
  it.each(["planning", "active"] as const)("is not offered on a %s goal", (status) => {
    renderActions(status)

    expect(screen.queryByRole("button", { name: "Delete goal" })).toBeNull()
    // What that goal is offered instead: the way to make it deletable.
    expect(screen.getByRole("button", { name: "Cancel goal" })).toBeDefined()
  })

  it.each(["completed", "cancelled"] as const)("is offered on a %s goal", (status) => {
    renderActions(status)

    expect(screen.getByRole("button", { name: "Delete goal" })).toBeDefined()
    expect(screen.queryByRole("button", { name: "Cancel goal" })).toBeNull()
  })

  it("asks first, and only the confirm sends the DELETE", async () => {
    const user = userEvent.setup()
    const { onDeleted } = renderActions("completed")

    await user.click(screen.getByRole("button", { name: "Delete goal" }))

    // The question names what goes, and nothing has gone yet.
    expect(await screen.findByText(/removed for good/)).toBeDefined()
    expect(wire()).toEqual([])

    await user.click(await confirmDelete())

    await waitFor(() => {
      expect(wire()).toEqual([{ method: "DELETE", path: `/v1/goals/${goal("completed").id}` }])
    })
    // The goal is gone, so whatever was showing it is told to close.
    await waitFor(() => {
      expect(onDeleted).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull()
    })
  })

  it("keeps the daemon's 409 on screen and stops offering the click", async () => {
    const user = userEvent.setup()
    const { onDeleted } = renderActions("completed")
    deleteFailure = {
      status: 409,
      code: "conflict",
      message: "goal is active, cancel it before deleting it",
    }

    await user.click(screen.getByRole("button", { name: "Delete goal" }))
    await user.click(await confirmDelete())

    expect(await screen.findByText("This goal is running again")).toBeDefined()
    expect(screen.getByText(/cancel it before deleting it/)).toBeDefined()
    // The dialog stays up, and re-confirming would only ask the same question.
    expect((await confirmDelete()).hasAttribute("disabled")).toBe(true)
    expect(onDeleted).not.toHaveBeenCalled()
  })
})
