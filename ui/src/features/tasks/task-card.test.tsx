// @vitest-environment jsdom

/**
 * The card's hints, reached the way a keyboard reaches them, and the tab stops
 * it costs to get there.
 *
 * The card says several things that are only explained in a tooltip — what the
 * sub-status means, what R2 counts, what it is waiting for, when exactly it
 * last moved, what it is running on instead of its profile's model — and it
 * says them in `<span>`s and a `<time>`. Those used to be focusable (the
 * tooltip primitive makes every trigger so; see `components/ui/tooltip.tsx`),
 * which reached the hints but cost some seven tab stops per card and nested
 * interactive nodes inside the card's own link. So the test is now the other
 * way round: the card is *one* stop, and every hint hangs off its link as
 * `aria-describedby`.
 *
 * Both halves matter, and each is worthless without the other — a card with
 * one stop and hints nobody can read is the same regression wearing different
 * clothes. So the stop count is asserted on the busiest card there is (an
 * engine override on top of everything else) and each hint is read back off
 * the link.
 *
 * The card also says each thing once. A badge that repeats a label already on
 * the card — the daemon's `stalled` reason beside the task's own `Stalled`
 * flag — is dropped rather than drawn twice. And what the task runs on is not
 * one of the things it says at all: that is a fact about the task, and the
 * task panel is where facts live.
 */

import { act, cleanup, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"

import type { TaskDto } from "@/api"
import type { SessionAttention } from "@/features/sessions/session-display"
import { formatAbsolute } from "@/lib/format"
import { aTask } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { TaskCard } from "./task-card"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const TASK: TaskDto = aTask({
  title: "Make the hints reachable",
  status: "changes_requested",
  branch: "make-the-hints-reachable-000001",
  depends_on: ["01JTASK0000000000000000002"],
  engineer_profile_id: "01JPROF0000000000000000ENG",
  review_round: 2,
  stalled: true,
  goal_id: "01JGOAL0000000000000000001",
})

/**
 * The card under the providers the app gives it: a router for its link and a
 * tooltip provider with no hover delay.
 */
function mountCard(attention?: SessionAttention, task: TaskDto = TASK) {
  renderScreen(<TaskCard task={task} attention={attention} />)
  return userEvent.setup()
}

/** The card's link, which is the whole of what a keyboard lands on. */
function cardLink(): HTMLElement {
  return screen.getByRole("link", { name: /Make the hints reachable/ })
}

/** The card's own box: the link's parent, and everything the card draws. */
function card(): HTMLElement {
  return cardLink().parentElement as HTMLElement
}

/**
 * The busiest card there is: a sub-status, a review round, a dependency, a
 * stall and a blocked agent — so a stop count taken on it is a stop count for
 * every card.
 */
const LOADED: TaskDto = { ...TASK, model: "codex:gpt-5.3-codex" }

/** What the link says about itself past its own text: its `aria-describedby`. */
function description(): string {
  const id = cardLink().getAttribute("aria-describedby")
  if (!id) throw new Error("the card's link describes itself with nothing")
  return document.getElementById(id)?.textContent ?? ""
}

it.each([
  ["the sub-status", /A reviewer asked for changes/],
  ["the review round", /Review round 2/],
  ["the dependency count", /Waits for 1 task/],
  ["the stall", /idle without advancing/],
  // The stamp behind "2 hours ago", which is the whole reason the relative
  // form is safe to show: without this the exact time is nowhere at all.
  ["the exact timestamp", new RegExp(formatAbsolute(TASK.updated_at))],
])("carries %s hint on the link itself", (_what, text) => {
  mountCard()
  expect(description()).toMatch(text)
})

it("is one tab stop for the whole card, hints and controls alike", async () => {
  const user = mountCard(undefined, LOADED)
  // Everything that could add a stop is on this card: the branch's copy button
  // and five hints inside the link.
  expect(screen.getByRole("button", { name: "Copy branch" })).not.toBeNull()

  await user.tab()
  expect(document.activeElement).toBe(cardLink())

  // And out the other side: the next Tab leaves the card altogether rather
  // than walking its pills. Seven stops per card is what this replaced.
  await user.tab()
  expect(card().contains(document.activeElement)).toBe(false)
})

it("keeps the copy button a target and a name, only not a stop", () => {
  mountCard(undefined, LOADED)
  // What it gives up is the tab order; the click and the accessible name are
  // untouched, and the same control is one Enter away in the task panel.
  expect(screen.getByRole("button", { name: "Copy branch" }).getAttribute("tabindex")).toBe("-1")
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

it("says a stall once, however many places report it", () => {
  // The daemon raises `stalled` on the session and the task carries a flag of
  // its own: two views of one fact, which the card drew as `Stalled` beside
  // `⚠ Stalled`.
  mountCard("stalled")
  expect(screen.getAllByText("Stalled")).toHaveLength(1)
  // Still outlined by it, though — the badge is what was dropped, not the fact.
  expect(cardLink().parentElement?.className).toContain("border-status-warn/40")
})

it("keeps a reason the card does not otherwise say", () => {
  mountCard("disconnected", { ...TASK, stalled: false })
  expect(screen.getByText("Disconnected")).not.toBeNull()
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

/**
 * What a task runs on is not on the card. It is a fact about the task, spelled
 * out in the panel one click away (`task-facts.tsx`), and on a card it was
 * both the least useful line and the longest — a `profile:model @ effort` in a
 * mono font, in a column that can be 11rem wide, which is how a board of
 * narrow cards ended up with pills painted across their own borders.
 */
it("says nothing about what the task runs on, however it is pinned", () => {
  mountCard(undefined, { ...TASK, model: "claude_code:claude-fable-5", effort: "high" })

  expect(screen.queryByText(/claude-fable-5/)).toBeNull()
  expect(description()).not.toContain("overrides")
})

/**
 * And what is left under the link stays inside it: the branch is a ULID-tailed
 * slug, and it is only middle-truncated because the pill it sits in is bounded
 * by the card. jsdom lays nothing out, so the ceiling itself is what is read.
 */
it("keeps the branch pill inside the card", () => {
  mountCard()

  // The pill around the branch and its copy button, bounded by the card: the
  // ellipsis is the pill's doing, so a pill that can outgrow the card is a
  // branch that paints across it instead.
  const pill = screen.getByTitle(TASK.branch).closest("span.max-w-full")
  expect(pill?.contains(screen.getByRole("button", { name: "Copy branch" }))).toBe(true)
})
