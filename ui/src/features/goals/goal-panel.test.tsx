// @vitest-environment jsdom

/**
 * What the goal panel says the goal has cost.
 *
 * A goal's total is the one figure that cannot be read anywhere else: its
 * planner belongs to no task, its engineers and reviewers belong to tasks the
 * panel does not list, and the sessions tab under it holds the planner's alone.
 * So the panel shows the daemon's own aggregate twice — the total in the
 * facts, and the split by the role that spent it above the sessions — and
 * neither is added up here.
 *
 * Grouped by role rather than by profile on purpose: past the planner, each
 * role is as many agents as the goal has tasks, and the task panels are where
 * those names are.
 *
 * Everything is seeded into the query cache; what the daemon returns is
 * `queries.ts`'s story.
 */

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { type GoalDto, qk } from "@/api"
import { aGoal } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { GoalPanel } from "./goal-panel"

const GOAL: GoalDto = aGoal({
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    planner: { input_tokens: 234_567, cached_input_tokens: 200_000, output_tokens: 5_300 },
    engineers: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    // Nothing has been reviewed yet, which is a row of zeros rather than no row.
    reviewers: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
  },
})

function mount(goal: GoalDto = GOAL) {
  renderScreen(<GoalPanel goalId={goal.id} onClose={() => {}} />, {
    route: `/goals?goal=${goal.id}`,
    seed: (client) => client.setQueryData(qk.goals.detail(goal.id), goal),
  })
}

/** The value of a fact of the metadata card, by the label above it. */
function detail(label: string): string {
  const term = screen.getByText(label)
  const value = term.nextElementSibling
  if (!value) throw new Error(`no value under "${label}"`)
  return value.textContent ?? ""
}

/** The usage block above the sessions table, once that tab is open. */
async function breakdown(): Promise<HTMLElement> {
  await userEvent.setup().click(screen.getByRole("tab", { name: "Sessions" }))
  const heading = await screen.findByRole("heading", { name: "Tokens" })
  const block = heading.closest("section")
  if (!block) throw new Error("no usage block around the Tokens heading")
  return block
}

it("shows the goal's total among its facts", () => {
  mount()

  expect(detail("Tokens")).toBe("in 1.2M (cached 1.1M) · out 45.3k")
})

it("says zero for a goal whose agents have reported nothing", () => {
  mount(aGoal())

  expect(detail("Tokens")).toBe("in 0 (cached 0) · out 0")
})

it("breaks the total down by the role that spent it", async () => {
  mount()
  const block = await breakdown()

  // In the order the work goes through them, and every role listed — a
  // reviewer that has spent nothing is an answer, not a row to drop.
  const rows = [...block.querySelectorAll("dt")].map((row) => row.textContent)
  expect(rows).toEqual(["Planner", "Engineers", "Reviewers"])
  expect(within(block).getByText("234.6k/5.3k")).not.toBeNull()
  expect(within(block).getByText("1.0M/40.0k")).not.toBeNull()
  expect(within(block).getByText("0/0")).not.toBeNull()

  // The total leads the block, and it is the goal's own: the planner and the
  // two roles under it, which is more than the list below this shows.
  expect(block.textContent).toContain("in 1.2M (cached 1.1M) · out 45.3k")
})
