// @vitest-environment jsdom

/**
 * The board's status filter, end to end: what the URL says, what the menu
 * shows, and what the daemon is asked for.
 *
 * Rendered rather than unit-tested because the part worth pinning is not the
 * parsing (`filters.test.ts` has that) — it is that the selection reaches
 * `GET /v1/goals` as one `?status=` and nothing is narrowed on the client, and
 * that a click on a checkbox item lands back in the URL. Only the mounted
 * board shows either.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter, useLocation } from "react-router-dom"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GoalDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"

import { GoalsListPage } from "./goals-list-page"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is a stub the daemon client never sees.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const GOAL: GoalDto = {
  id: "01JGOAL0000000000000000001",
  title: "Ship the board",
  description: "",
  planner_profile_id: "01JPROF00000000000000PLAN",
  repos: [],
  required_approvals: 1,
  status: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

/** Every list the board and its attention strip read, each answering empty. */
function stubDaemon(goals: GoalDto[] = [GOAL]) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body = url.pathname === "/v1/goals" ? goals : []
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
  })
}

/** What `GET /v1/goals` was asked to filter by, oldest request first. */
function goalRequests(): (string | null)[] {
  return daemonFetch.mock.calls
    .map(([input]) => new URL(typeof input === "string" ? input : (input as Request).url))
    .filter((url) => url.pathname === "/v1/goals")
    .map((url) => url.searchParams.get("status"))
}

function renderBoard(entry: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  })
  const seen = { url: entry }
  function Probe() {
    const location = useLocation()
    seen.url = `${location.pathname}${location.search}`
    return null
  }
  render(
    <MemoryRouter initialEntries={[entry]}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delay={0}>
          <GoalsListPage />
          <Probe />
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
  return seen
}

/** The trigger, which doubles as the summary of what is selected. */
function trigger() {
  return screen.getByRole("button", { name: "Filter by status" })
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

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

it("asks the daemon for every selected status, in one request", async () => {
  renderBoard("/goals?status=completed,active")

  await waitFor(() => expect(goalRequests()).toContain("active,completed"))
  expect(trigger().textContent).toContain("2 statuses")
})

it("names the one status it is narrowed to", async () => {
  renderBoard("/goals?status=planning")

  await waitFor(() => expect(goalRequests()).toContain("planning"))
  expect(trigger().textContent).toContain("Planning")
})

it("ignores a status the daemon does not define", async () => {
  renderBoard("/goals?status=nonsense")

  await waitFor(() => expect(goalRequests().length).toBeGreaterThan(0))
  expect(goalRequests()).not.toContain("nonsense")
  expect(goalRequests().every((status) => status === null)).toBe(true)
  expect(trigger().textContent).toContain("All statuses")
})

it("puts a checked status in the URL and in the next request", async () => {
  const user = userEvent.setup()
  const seen = renderBoard("/goals")
  await waitFor(() => expect(goalRequests().length).toBeGreaterThan(0))

  await user.click(trigger())
  await user.click(await screen.findByRole("menuitemcheckbox", { name: "Active" }))

  await waitFor(() => expect(seen.url).toBe("/goals?status=active"))
  await waitFor(() => expect(goalRequests()).toContain("active"))

  // The menu stays open, so a second status is one click away. The comma the
  // daemon reads is what `URLSearchParams` percent-encodes it to on the way out.
  await user.click(await screen.findByRole("menuitemcheckbox", { name: "Completed" }))
  await waitFor(() => expect(seen.url).toBe("/goals?status=active%2Ccompleted"))
})

it("clears back to all statuses, dropping the param", async () => {
  const user = userEvent.setup()
  const seen = renderBoard("/goals?status=active,completed")
  await waitFor(() => expect(goalRequests().length).toBeGreaterThan(0))

  await user.click(trigger())
  await user.click(await screen.findByRole("menuitemcheckbox", { name: "All statuses" }))

  await waitFor(() => expect(seen.url).toBe("/goals"))
  expect(trigger().textContent).toContain("All statuses")
})

it("opens and toggles from the keyboard", async () => {
  const user = userEvent.setup()
  const seen = renderBoard("/goals")
  await waitFor(() => expect(goalRequests().length).toBeGreaterThan(0))

  trigger().focus()
  await user.keyboard("{Enter}")
  // Opening highlights the first item; the statuses are one arrow below it.
  const all = await screen.findByRole("menuitemcheckbox", { name: "All statuses" })
  await waitFor(() => expect(document.activeElement).toBe(all))
  await user.keyboard("{ArrowDown}")
  await waitFor(() =>
    expect(document.activeElement).toBe(screen.getByRole("menuitemcheckbox", { name: "Planning" })),
  )
  await user.keyboard("{Enter}")

  await waitFor(() => expect(seen.url).toBe("/goals?status=planning"))
})

it("says what the filter, not the board, came up empty on", async () => {
  stubDaemon([])
  renderBoard("/goals?status=active,completed")

  expect(await screen.findByText("No goals match this filter")).toBeDefined()
})
