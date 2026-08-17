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

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { MemoryRouter } from "react-router-dom"
import { afterEach, expect, it } from "vitest"

import type { TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"

import { TaskCard } from "./task-card"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: "01JGOAL0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  title: "Make the hints reachable",
  description: "",
  status: "changes_requested",
  branch: "ariadne/task-01JTASK0000000000000000001",
  depends_on: ["01JTASK0000000000000000002"],
  engineer_profile_id: "01JPROF0000000000000000ENG",
  reviewer_profile_ids: [],
  review_round: 2,
  stalled: true,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

function mountCard() {
  render(
    <TooltipProvider delay={0}>
      <MemoryRouter>
        <TaskCard task={TASK} />
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
