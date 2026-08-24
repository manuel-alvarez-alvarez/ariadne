// @vitest-environment jsdom

/**
 * The card's hints, reached the way a keyboard reaches them.
 *
 * The card says four things that are only explained in a tooltip — what the
 * sub-status means, what R2 counts, what it is waiting for, when it last moved
 * — and it says them in `<span>`s and a `<time>`, which take no focus of their
 * own. So the test is Tab and read: nothing here checks that a tooltip *exists*,
 * only that pressing Tab opens one, which is the whole point of the card not
 * using `title=` (see its docblock, and `components/ui/tooltip.tsx`).
 */

import { act, cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { afterEach, expect, it, vi } from "vitest"

import type { TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { SessionAttention } from "@/features/sessions/session-display"

import { TaskCard } from "./task-card"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: "01JGOAL0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  title: "Make the hints reachable",
  description: "",
  status: "changes_requested",
  branch: "make-the-hints-reachable-000001",
  depends_on: ["01JTASK0000000000000000002"],
  engineer_profile_id: "01JPROF0000000000000000ENG",
  reviewers: [],
  review_round: 2,
  stalled: true,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mountCard(attention?: SessionAttention, task: TaskDto = TASK) {
  render(
    <TooltipProvider delay={0}>
      <MemoryRouter>
        <TaskCard task={task} attention={attention} />
      </MemoryRouter>
    </TooltipProvider>,
  )
  return userEvent.setup()
}

/** Tabs until a tooltip is showing `text`, or runs out of stops. */
async function tabUntilHint(user: ReturnType<typeof userEvent.setup>, text: RegExp) {
  for (let stop = 0; stop < 12; stop++) {
    await user.tab()
    if (screen.queryByText(text)) return true
  }
  return false
}

it.each([
  ["the sub-status", /A reviewer asked for changes/],
  ["the review round", /Review round 2/],
  ["the dependency count", /Waits for 1 task/],
  ["the stall", /idle without advancing/],
  ["the timestamp", /updated /],
])("opens %s hint on focus", async (_what, text) => {
  const user = mountCard()
  expect(await tabUntilHint(user, text)).toBe(true)
})

/**
 * A blocked agent is the one thing on the card addressed to the reader, so it
 * is said in full on the card itself — the strip above the board is where it
 * used to be the only place it was said at all.
 */
it("says which of the task's agents is waiting on a person", () => {
  mountCard("waiting_permission")
  expect(screen.getByText("Waiting for permission")).not.toBeNull()
})

it("says nothing when no agent of the task is waiting", () => {
  mountCard()
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})

/**
 * A board is left open, and the card is the surface that goes stale fastest —
 * it used to keep whatever "N minutes ago" it was rendered with. The clock is
 * shared and the card only reads it (`components/when.tsx`), so advancing time
 * is enough: nothing here refetches.
 */
it("keeps its timestamp true as the clock moves", () => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date("2026-08-19T12:00:00Z"))
  mountCard(undefined, { ...TASK, updated_at: "2026-08-19T11:59:00Z" })
  expect(screen.getByText("1 minute ago")).not.toBeNull()

  act(() => void vi.advanceTimersByTime(4 * 60_000))
  expect(screen.getByText("5 minutes ago")).not.toBeNull()
})
