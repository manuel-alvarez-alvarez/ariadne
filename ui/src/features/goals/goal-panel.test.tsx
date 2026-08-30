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

import { screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it } from "vitest"

import { type GoalDto, type ProfileDto, qk, type SessionDto, type TaskDto } from "@/api"
import { aGoal, aProfile, aSession, aTask } from "@/test/fixtures"
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

/**
 * The goal's tasks, and the sessions the daemon holds for it — which is every
 * role's, because the daemon takes no role filter and the tab narrows the one
 * list it answers with (see `sessions/queries.ts`).
 */
function mount(
  goal: GoalDto = GOAL,
  { tasks = [], sessions = [] }: { tasks?: TaskDto[]; sessions?: SessionDto[] } = {},
) {
  renderScreen(<GoalPanel goalId={goal.id} onClose={() => {}} />, {
    route: `/goals?goal=${goal.id}`,
    seed: (client) => {
      client.setQueryData(qk.goals.detail(goal.id), goal)
      client.setQueryData(qk.profiles.list({}), [PLANNER])
      client.setQueryData(qk.tasks.list({ goal: goal.id }), tasks)
      client.setQueryData(qk.sessions.list({ goal: goal.id }), sessions)
    },
  })
}

/** A tab by its name, whatever count is sitting on it. */
function tab(name: string): HTMLElement {
  return screen.getByRole("tab", { name: new RegExp(`^${name}`) })
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
  const exact = await screen.findByText("Input")
  const popup = exact.closest<HTMLElement>("[data-slot='tooltip-content']")
  if (!popup) throw new Error("no hint around the exact counts")
  return popup
}

it("shows the goal's total among its facts, as the pair it is", () => {
  mount()

  // The pair on its own line: both halves, and the share the cache served of
  // the input riding on the half it belongs to.
  expect(detail("Tokens").textContent).toBe("1.2M in, 89% cached, 45k out")
})

it("says zero for a goal whose agents have reported nothing", () => {
  mount(aGoal())

  expect(detail("Tokens").textContent).toBe("0 in, 0% cached, 0 out")
})

it("breaks the total down by the role that spent it, behind the figure", async () => {
  mount()
  const popup = await hint(detail("Tokens"))

  // In the order the work goes through them, and every role listed — a
  // reviewer that has spent nothing is an answer, not a line to drop.
  const roles = [...popup.querySelectorAll("dt")].map((role) => role.textContent)
  expect(roles).toEqual(["Planner", "Engineers", "Reviewers"])
  const figures = [...popup.querySelectorAll("dd")].map((figure) => figure.textContent)
  expect(figures).toEqual([
    "235k in, 85% cached, 5.3k out",
    "1M in, 90% cached, 40k out",
    "0 in, 0% cached, 0 out",
  ])

  // The two halves lead the hint, named and each on its own line, and they are
  // the goal's own total: the planner and the two roles under it, none of it
  // added up here.
  const total = within(popup)
  const input = total.getByText("Input")
  expect(input.nextElementSibling?.textContent).toBe("1.2M")
  // The share rides beside the input count, part of it rather than a count of
  // its own — the same share the figure itself shows.
  expect(input.nextElementSibling?.nextElementSibling?.textContent).toBe("89%")
  expect(total.getByText("Output").nextElementSibling?.textContent).toBe("45k")
  // Nothing in the hint is spelled to the digit any more: not the halves, not
  // the rows under them.
  expect(popup.textContent).not.toMatch(/\d,\d/)
})

it("keeps the sessions tab to the sessions, with no breakdown above them", async () => {
  mount()
  await userEvent.setup().click(tab("Planner sessions"))

  // The figure in the facts carries the total and its split; a card repeating
  // both above a table whose rows carry their own figures said it all twice.
  expect(screen.queryByRole("heading", { name: "Tokens" })).toBeNull()
})

it("shows what the planner runs on: the goal's pin, and that it is a pin", () => {
  mount(aGoal({ model: "codex:gpt-5.3-codex" }))

  const planner = detail("Planner").textContent ?? ""
  expect(planner).toContain("codex:gpt-5.3-codex")
  // One word for "this is not what the profile says", where "(overrides)" left
  // the reader to work out which of the two won.
  expect(planner).toContain("pinned")
  expect(planner).not.toContain("grok-4")
})

it("shows the effort that model is run at, beside it", () => {
  mount(aGoal({ model: "codex:gpt-5.3-codex", effort: "high" }))

  expect(detail("Planner").textContent).toContain("codex:gpt-5.3-codex @ high")
})

it("says `auto` for a goal that pinned nothing, rather than the profile's own", () => {
  mount(aGoal({ model: null }))

  expect(detail("Planner").textContent).toContain("auto")
})

describe("which tab the panel opens on", () => {
  it("opens a goal still being planned on its tasks, the list still growing", () => {
    mount(aGoal({ status: "planning" }))

    expect(tab("Tasks")).toHaveProperty("ariaSelected", "true")
  })

  it("opens every other goal on its tasks too, which is what a goal comes down to", () => {
    mount(aGoal({ status: "active" }))

    expect(tab("Tasks")).toHaveProperty("ariaSelected", "true")
  })

  it("still opens where the URL says, whatever the status", () => {
    const goal = aGoal({ status: "planning", description: "The plan so far." })
    renderScreen(<GoalPanel goalId={goal.id} onClose={() => {}} />, {
      route: `/goals?goal=${goal.id}&tab=description`,
      seed: (client) => client.setQueryData(qk.goals.detail(goal.id), goal),
    })

    expect(tab("Description")).toHaveProperty("ariaSelected", "true")
  })
})

describe("what the tabs are called, and how much is behind them", () => {
  it("counts the tasks and the planner sessions, and only the planner ones", () => {
    mount(GOAL, {
      tasks: [
        aTask({ id: "01JTASK000000000000000000A" }),
        aTask({ id: "01JTASK000000000000000000B" }),
      ],
      sessions: [
        aSession({ id: "01JSESS000000000000000PLAN", role: "planner", task_id: null }),
        aSession({ id: "01JSESS000000000000000ENG1", role: "engineer" }),
        aSession({ id: "01JSESS000000000000000REV1", role: "reviewer" }),
      ],
    })

    expect(tab("Tasks").textContent).toBe("Tasks2")
    // The tab is the goal's own agent, so it says so — and a goal with four
    // sessions under it has one planner session, not four.
    expect(tab("Planner sessions").textContent).toBe("Planner sessions1")

    // A bare number beside a label is read out as "Tasks 2", which says
    // nothing about what the two are; each pill names what it counts.
    expect(within(tab("Tasks")).getByLabelText("2 tasks").textContent).toBe("2")
    expect(within(tab("Planner sessions")).getByLabelText("1 session").textContent).toBe("1")
  })

  it("says nothing about a count it does not have yet", () => {
    const goal = aGoal()
    renderScreen(<GoalPanel goalId={goal.id} onClose={() => {}} />, {
      route: `/goals?goal=${goal.id}`,
      seed: (client) => client.setQueryData(qk.goals.detail(goal.id), goal),
    })

    // A count that starts at zero and jumps says the goal is empty for as long
    // as the request takes.
    expect(tab("Tasks").textContent).toBe("Tasks")
  })

  it("names an empty planner tab for the role it lists, not for the goal", async () => {
    mount(GOAL, { sessions: [aSession({ role: "engineer" })] })
    await userEvent.setup().click(tab("Planner sessions"))

    expect(screen.getByText("No planner session yet")).toBeDefined()
  })
})
