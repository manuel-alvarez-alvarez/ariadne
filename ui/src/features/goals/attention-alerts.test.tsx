// @vitest-environment jsdom

/**
 * The half of the attention list that is not the strip: the count in the
 * window title and on the sidebar, and the toast for something that got stuck
 * while the user was looking at another screen.
 *
 * What is worth pinning down is when it stays *quiet*. A window opened onto
 * six stuck agents must not open onto six toasts, and the board — where the
 * strip already says all of it, in a place that does not disappear after six
 * seconds — must raise none at all. The rest is that news actually gets
 * through, once, with the way to answer it on it.
 *
 * The daemon's lists stand in for the event stream here: an item arriving over
 * SSE is `sessions.lists` being invalidated and answered differently, which is
 * exactly what the dispatcher does with the event (see `events/dispatch.ts`).
 */

import type { QueryClient } from "@tanstack/react-query"
import { fireEvent, screen, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { beforeEach, expect, it, vi } from "vitest"
import type { SessionDto } from "@/api"

import { Toaster } from "@/components/ui/sonner"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"

import { AttentionAlerts, AttentionBadge } from "./attention-alerts"

const GOAL = aGoal()
const TASK = aTask({ title: "Wire the strip", goal_id: GOAL.id })
const ENGINEER = "01JPROF0000000000000000ENG"

/** The agent that gets blocked halfway through each test below. */
const BLOCKED: SessionDto = aSession({
  id: "01JSESS0000000000000000001",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: ENGINEER,
  status: "running",
  attention_reason: "waiting_permission",
  attention_since: "2026-01-01T03:00:00Z",
})

/** What the daemon answers right now; a test moves it and re-asks. */
let sessions: SessionDto[]

function stubDaemon() {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      url.pathname === "/v1/goals" ? [GOAL] : url.pathname === "/v1/tasks" ? [TASK] : sessions
    return Promise.resolve(jsonResponse(body))
  })
}

beforeEach(() => {
  // Sonner's store outlives a render, and a toast raised by the test before
  // this one is still in it — it would render into the next test's toaster and
  // read as news that never happened.
  toast.dismiss()
  sessions = []
  document.title = "Ariadne Desktop"
  // The toaster asks the browser whether motion is welcome; jsdom has no
  // opinion and no `matchMedia` to hold one.
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  }))
  stubDaemon()
})

/** The shell's two silent parts, on whichever screen the test is on. */
function renderAlerts(route: string) {
  return renderScreen(
    <>
      <AttentionAlerts />
      <Toaster />
    </>,
    { route },
  )
}

/** What the stream would have delivered: a different answer, re-asked for. */
async function arrives(queryClient: QueryClient, next: SessionDto[]) {
  sessions = next
  await queryClient.invalidateQueries()
}

/**
 * The three lists, answered. Everything below turns on the difference between
 * the list that was already there and what arrived after it, so nothing may
 * arrive before the first answer has landed.
 */
async function settled(queryClient: QueryClient) {
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
}

it("counts what needs attention in the window title", async () => {
  sessions = [BLOCKED]
  renderAlerts("/profiles")

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})

it("gives the title back when the last of it is answered", async () => {
  sessions = [BLOCKED]
  const { queryClient } = renderAlerts("/profiles")
  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
  await settled(queryClient)

  await arrives(queryClient, [{ ...BLOCKED, attention_reason: null }])
  await waitFor(() => expect(document.title).toBe("Ariadne Desktop"))
})

// A window opened onto six stuck agents opens onto six toasts otherwise, none
// of which is news.
it("raises no toast for what was already stuck when it opened", async () => {
  sessions = [BLOCKED]
  renderAlerts("/profiles")

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})

it("raises one toast for an agent that gets stuck on another screen", async () => {
  const { queryClient, location } = renderAlerts("/profiles")
  await settled(queryClient)

  await arrives(queryClient, [BLOCKED])

  expect(await screen.findByText("Waiting for permission")).not.toBeNull()
  expect(screen.getAllByText("Waiting for permission")).toHaveLength(1)
  // What it is about, so the toast is worth reading before it is clicked.
  expect(screen.getByText(TASK.title)).not.toBeNull()

  // The same place the strip's row would have gone: the pane the prompt is
  // waiting in, with the keyboard already in it.
  // Fired rather than typed: a toast tracks a swipe through pointer capture,
  // which jsdom does not implement.
  fireEvent.click(screen.getByRole("button", { name: "Open" }))
  expect(location.url).toBe(`/profiles?session=${BLOCKED.id}&tab=terminal&focus=terminal`)
})

// Re-asking for the same lists — a reconnect, a refetch, any other event —
// re-answers with the same rows, and none of them is new.
it("does not announce the same item twice", async () => {
  const { queryClient } = renderAlerts("/profiles")
  await settled(queryClient)

  await arrives(queryClient, [BLOCKED])
  expect(await screen.findByText("Waiting for permission")).not.toBeNull()

  await arrives(queryClient, [BLOCKED])
  await waitFor(() => expect(screen.getAllByText("Waiting for permission")).toHaveLength(1))
})

// The board carries the strip, which says the same thing where it cannot be
// missed and does not vanish.
it("stays quiet while the board is up", async () => {
  const { queryClient } = renderAlerts("/goals")
  await settled(queryClient)

  await arrives(queryClient, [BLOCKED])

  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})

it("counts the same list on the sidebar entry, and nothing when it is empty", async () => {
  sessions = [BLOCKED, { ...BLOCKED, id: "01JSESS0000000000000000002", task_id: null }]
  const { rerender } = renderScreen(<AttentionBadge />, { route: "/profiles" })

  expect(await screen.findByText("2")).not.toBeNull()
  expect(screen.getByLabelText("2 items needing attention")).not.toBeNull()

  rerender(<AttentionBadge />)
  expect(screen.queryByText("0")).toBeNull()
})
