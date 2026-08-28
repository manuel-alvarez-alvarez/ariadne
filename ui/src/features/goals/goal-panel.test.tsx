// @vitest-environment jsdom

/**
 * What the goal panel says the goal has cost.
 *
 * A goal's total is the one figure that cannot be read anywhere else: its
 * planner belongs to no task, its engineers and reviewers belong to tasks the
 * panel does not list, and the sessions tab under it holds the planner's alone.
 * So the panel shows the daemon's own aggregate twice over — the pair in the
 * facts, and the split by the role that spent it in the hint behind that pair
 * — and neither is added up here.
 *
 * Grouped by role rather than by profile on purpose: past the planner, each
 * role is as many agents as the goal has tasks, and the task panels are where
 * those names are. The Planner fact is the exception, and it says what that
 * agent runs on: the goal's own pin, not the profile as edited since.
 *
 * Everything is seeded into the query cache; what the daemon returns is
 * `queries.ts`'s story.
 */

import { screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { type GoalDto, type ProfileDto, qk } from "@/api"
import { aGoal, aProfile } from "@/test/fixtures"
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

/** The planner profile as it stands today, moved on from the goal's own pin. */
const PLANNER: ProfileDto = aProfile({
  id: GOAL.planner_profile_id,
  // Not "Planner": that is also the label of the fact this reads.
  name: "plan-lead",
  role: "planner",
  model: "opencode:grok-4",
})

function mount(goal: GoalDto = GOAL) {
  renderScreen(<GoalPanel goalId={goal.id} onClose={() => {}} />, {
    route: `/goals?goal=${goal.id}`,
    seed: (client) => {
      client.setQueryData(qk.goals.detail(goal.id), goal)
      client.setQueryData(qk.profiles.list({}), [PLANNER])
    },
  })
}

/** The value of a fact of the metadata card, by the label above it. */
function detail(label: string): HTMLElement {
  const term = screen.getByText(label)
  const value = term.nextElementSibling
  if (!(value instanceof HTMLElement)) throw new Error(`no value under "${label}"`)
  return value
}

/** The hint behind a figure, opened the way a keyboard opens it. */
async function hint(value: HTMLElement): Promise<HTMLElement> {
  const figure = value.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")
  if (!figure) throw new Error("no figure to open a hint on")
  figure.focus()
  const exact = await screen.findByText(/^In \d/)
  const popup = exact.closest<HTMLElement>("[data-slot='tooltip-content']")
  if (!popup) throw new Error("no hint around the exact counts")
  return popup
}

it("shows the goal's total among its facts, as the pair it is", () => {
  mount()

  expect(detail("Tokens").textContent).toBe("1.2M in, 45k out")
})

it("says zero for a goal whose agents have reported nothing", () => {
  mount(aGoal())

  expect(detail("Tokens").textContent).toBe("0 in, 0 out")
})

it("breaks the total down by the role that spent it, behind the figure", async () => {
  mount()
  const popup = await hint(detail("Tokens"))

  // In the order the work goes through them, and every role listed — a
  // reviewer that has spent nothing is an answer, not a line to drop.
  const roles = [...popup.querySelectorAll("dt")].map((role) => role.textContent)
  expect(roles).toEqual(["Planner", "Engineers", "Reviewers"])
  const figures = [...popup.querySelectorAll("dd")].map((figure) => figure.textContent)
  expect(figures).toEqual(["235k in, 5.3k out", "1M in, 40k out", "0 in, 0 out"])

  // The exact counts lead the hint, and they are the goal's own total: the
  // planner and the two roles under it, none of it added up here.
  expect(popup.textContent).toContain("In 1,234,567 (cached 1,100,000) · Out 45,300")
})

it("keeps the sessions tab to the sessions, with no breakdown above them", async () => {
  mount()
  await userEvent.setup().click(screen.getByRole("tab", { name: "Sessions" }))

  // The figure in the facts carries the total and its split; a card repeating
  // both above a table whose rows carry their own figures said it all twice.
  expect(screen.queryByRole("heading", { name: "Tokens" })).toBeNull()
})

it("shows what the planner runs on: the goal's pin, and that it overrides", () => {
  mount(aGoal({ model: "codex:gpt-5.3-codex" }))

  const planner = detail("Planner").textContent ?? ""
  expect(planner).toContain("codex:gpt-5.3-codex")
  expect(planner).toContain("(overrides)")
  expect(planner).not.toContain("grok-4")
})

it("says `auto` for a goal that pinned nothing, rather than the profile's own", () => {
  mount(aGoal({ model: null }))

  expect(detail("Planner").textContent).toContain("auto")
})
