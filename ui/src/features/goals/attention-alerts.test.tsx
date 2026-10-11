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
import type { AttentionItemDto, SessionDto } from "@/api"

import { Toaster } from "@/components/ui/sonner"
import { aGoal, aSession, aSessionPage, aTask } from "@/test/fixtures"
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
  task_agent_id: ENGINEER,
  status: "running",
  attention_reason: "waiting_permission",
  attention_since: "2026-01-01T03:00:00Z",
})

/** What the daemon answers right now; a test moves it and re-asks. */
let sessions: SessionDto[]
/** The `pull_request` (or any other) producer's own items right now. */
let recoveryItems: AttentionItemDto[]

/** A `pull_request` producer item, as `GET /v1/attention` answers one. */
function aPullRequestItem(overrides: Partial<AttentionItemDto> = {}): AttentionItemDto {
  return {
    id: "pull_request:review_start:pull-1",
    producer: "pull_request",
    reason: "configuration",
    summary: "acme/widgets#1 Fix widgets asks for your review, and no agent is assigned to it.",
    required_action: "Start a review yourself, on a model you pick.",
    since: "2026-01-01T03:00:00Z",
    affected: [{ kind: "pull_request", id: "pull-1", label: "acme/widgets#1 Fix widgets" }],
    target: { kind: "pull_request", pull_request_id: "pull-1" },
    ...overrides,
  }
}

function stubDaemon() {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body =
      url.pathname === "/v1/goals"
        ? [GOAL]
        : url.pathname === "/v1/tasks"
          ? [TASK]
          : url.pathname === "/v1/attention"
            ? { items: recoveryItems, complete: true }
            : aSessionPage(sessions)
    return Promise.resolve(jsonResponse(body))
  })
}

beforeEach(() => {
  // Sonner's store outlives a render, and a toast raised by the test before
  // this one is still in it — it would render into the next test's toaster and
  // read as news that never happened.
  toast.dismiss()
  sessions = []
  recoveryItems = []
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

/** The same, for the `pull_request` (or any other) producer's own items. */
async function attentionArrives(queryClient: QueryClient, next: AttentionItemDto[]) {
  recoveryItems = next
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

  // The same place the strip's row would have gone: the console the prompt is
  // waiting in, with the keyboard already in it.
  // Fired rather than typed: a toast tracks a swipe through pointer capture,
  // which jsdom does not implement.
  fireEvent.click(screen.getByRole("button", { name: "Open" }))
  expect(location.url).toBe(`/profiles?session=${BLOCKED.id}&tab=terminal&focus=terminal`)
  // Open also dismisses the toast. Sonner's exit timer survives unmount, so
  // finish that dismissal before cleanup can remove the browser environment.
  await waitFor(() => expect(screen.queryByText("Waiting for permission")).toBeNull())
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

// A `pull_request` producer item renders through the same shared extension
// point every other recovery item does: its own headline, not the task or
// session fallback, which would otherwise read every one of these as
// "Failed".
it("raises a toast titled for an unassigned review request, naming the request and its action", async () => {
  const { queryClient } = renderAlerts("/profiles")
  await settled(queryClient)

  await attentionArrives(queryClient, [aPullRequestItem()])

  expect(await screen.findByText("Review needed")).not.toBeNull()
  expect(
    screen.getByText(
      "acme/widgets#1 Fix widgets asks for your review, and no agent is assigned to it.",
    ),
  ).not.toBeNull()
  await waitFor(() => expect(document.title).toBe("(1) Ariadne"))
})

it("titles a readiness item as ready to merge", async () => {
  const { queryClient } = renderAlerts("/profiles")
  await settled(queryClient)

  await attentionArrives(queryClient, [
    aPullRequestItem({
      id: "pull_request:ready:pull-1",
      reason: "unknown",
      summary:
        "acme/widgets#1 Fix widgets is approved, every check passes, and the forge reports it clear to merge.",
      required_action: "Merge it yourself: Ariadne gives no approval.",
    }),
  ])

  expect(await screen.findByText("Ready to merge")).not.toBeNull()
})

// A completed automated review (a pr-reviewer session's own `waiting_user`,
// which the daemon no longer raises at all — see `session-display.tsx`) is
// not a producer item, and must raise no notification, no badge and no
// attention row of its own.
it("raises no notification or badge for a session with no recovery item behind it", async () => {
  const reviewer: SessionDto = {
    ...BLOCKED,
    id: "01JSESS0000000000000000003",
    task_id: null,
    seat: "reviewer",
    pull_request_id: "pull-1",
    attention_reason: "waiting_user",
  }
  const { queryClient } = renderAlerts("/profiles")
  await settled(queryClient)

  await arrives(queryClient, [reviewer])

  await waitFor(() => expect(document.title).toBe("Ariadne Desktop"))
  expect(screen.queryByText("Ready to merge")).toBeNull()
  expect(screen.queryByText("Review needed")).toBeNull()
})

// Repeated reads of the same still-open item — a poll, a reconnect — raise
// no second toast; withdrawing it raises none either; and a later,
// different readiness transition on the same request raises its own,
// separate toast.
it("raises one toast per readiness transition, none for a repeat, and none for a withdrawal", async () => {
  const { queryClient } = renderAlerts("/profiles")
  await settled(queryClient)

  await attentionArrives(queryClient, [aPullRequestItem()])
  expect(await screen.findByText("Review needed")).not.toBeNull()

  // The same item again: no second toast.
  await attentionArrives(queryClient, [aPullRequestItem()])
  await waitFor(() => expect(screen.getAllByText("Review needed")).toHaveLength(1))

  // Withdrawn: no toast of its own, and the title falls quiet.
  await attentionArrives(queryClient, [])
  await waitFor(() => expect(document.title).toBe("Ariadne Desktop"))

  // A later, different transition — the request is now ready to merge —
  // raises its own toast.
  await attentionArrives(queryClient, [
    aPullRequestItem({
      id: "pull_request:ready:pull-1",
      reason: "unknown",
      required_action: "Merge it yourself: Ariadne gives no approval.",
    }),
  ])
  expect(await screen.findByText("Ready to merge")).not.toBeNull()
})
