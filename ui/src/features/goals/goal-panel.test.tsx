// @vitest-environment jsdom

/**
 * What the goal panel says the goal has cost.
 *
 * A goal's total is the one figure that cannot be read anywhere else: its
 * orchestrator belongs to no task, its authors and reviewers belong to tasks the
 * panel does not list, and the sessions tab under it holds the orchestrator's alone.
 * So the panel shows the daemon's own aggregate twice over — the pair in the
 * facts, and the split by the seat that spent it in the hint behind that pair
 * — and neither is added up here.
 *
 * Grouped by seat rather than by profile on purpose: past the orchestrator, each
 * seat is as many agents as the goal has tasks, and the task panels are where
 * those names are. The Orchestrator fact is the exception, and it says what that
 * agent runs on: the goal's own pin, not the profile as edited since.
 *
 * Everything is seeded into the query cache; what the daemon returns is
 * `queries.ts`'s story.
 */

import { act, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it } from "vitest"

import { type GoalDto, qk, type SessionDto, type TaskDto } from "@/api"
import { aGoal, aRepository, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, errorResponse, renderScreen } from "@/test/harness"
import { GoalPanel } from "./goal-panel"

const GOAL: GoalDto = aGoal({
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    orchestrator: { input_tokens: 234_567, cached_input_tokens: 200_000, output_tokens: 5_300 },
    authors: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    // Nothing has been reviewed yet, which is a row of zeros rather than no row.
    reviewers: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
  },
})

/**
 * The goal's tasks, and the sessions the daemon holds for it — which is every
 * seat's, because the daemon takes no seat filter and the tab narrows the one
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
  expect(detail("Tokens").textContent).toBe("1.2M in, 89.1% cached, 45k out")
})

it("says zero for a goal whose agents have reported nothing", () => {
  mount(aGoal())

  expect(detail("Tokens").textContent).toBe("0 in, 0.0% cached, 0 out")
})

it("breaks the total down by the seat that spent it, behind the figure", async () => {
  mount()
  const popup = await hint(detail("Tokens"))

  // In the order the work goes through them, and every seat listed — a
  // reviewer that has spent nothing is an answer, not a line to drop.
  const roles = [...popup.querySelectorAll("dt")].map((seat) => seat.textContent)
  expect(roles).toEqual(["Orchestrator", "Authors", "Reviewers"])
  const figures = [...popup.querySelectorAll("dd")].map((figure) => figure.textContent)
  expect(figures).toEqual([
    "235k in, 85.3% cached, 5.3k out",
    "1M in, 90.0% cached, 40k out",
    "0 in, 0.0% cached, 0 out",
  ])

  // The two halves lead the hint, named and each on its own line, and they are
  // the goal's own total: the orchestrator and the two roles under it, none of it
  // added up here.
  const total = within(popup)
  const input = total.getByText("Input")
  expect(input.nextElementSibling?.textContent).toBe("1.2M")
  // The share rides beside the input count, part of it rather than a count of
  // its own — the same share the figure itself shows.
  expect(input.nextElementSibling?.nextElementSibling?.textContent).toBe("89.1%")
  expect(total.getByText("Output").nextElementSibling?.textContent).toBe("45k")
  // Nothing in the hint is spelled to the digit any more: not the halves, not
  // the rows under them.
  expect(popup.textContent).not.toMatch(/\d,\d/)
})

it("keeps the sessions tab to the sessions, with no breakdown above them", async () => {
  mount()
  await userEvent.setup().click(tab("Sessions"))

  // The figure in the facts carries the total and its split; a card repeating
  // both above a table whose rows carry their own figures said it all twice.
  expect(screen.queryByRole("heading", { name: "Tokens" })).toBeNull()
})

it("drops the Updated fact, moving the two stamps onto a meta line under the title", () => {
  mount()

  expect(screen.queryByText("Updated")).toBeNull()
  // Beside the goal id, on the line the id itself sits on.
  const line = screen.getByText(GOAL.id).closest("div")
  if (!line) throw new Error("no meta line around the goal id")
  expect(line.textContent).toContain("created")
  expect(line.textContent).toContain("updated")
  expect(line.querySelector(`time[datetime="${GOAL.created_at}"]`)).not.toBeNull()
  expect(line.querySelector(`time[datetime="${GOAL.updated_at}"]`)).not.toBeNull()
})

it("shows what the orchestrator runs on, as a pin held to one line", () => {
  mount(aGoal({ model: "codex-acp:gpt-5.3-codex" }))

  const orchestrator = detail("Orchestrator")
  expect(orchestrator.textContent).toContain("codex-acp:gpt-5.3-codex")
  // Cut in the middle rather than wrapped, the way every fact's pin now
  // holds to its one line.
  expect(orchestrator.querySelector("[data-slot='tooltip-trigger']")).not.toBeNull()
})

it("shows the effort that model is run at, beside it", () => {
  mount(aGoal({ model: "codex-acp:gpt-5.3-codex", effort: "high" }))

  expect(detail("Orchestrator").textContent).toContain("codex-acp:gpt-5.3-codex @ high")
})

it("says an unorchestrated goal has no orchestrator", () => {
  mount(aGoal({ orchestrated: false }))

  expect(detail("Orchestrator").textContent).toBe("No orchestrator")
})

it("shows the goal's landing among its facts", () => {
  mount(aGoal({ landing: "pull_request" }))

  expect(detail("Landing").textContent).toBe("Open a request and see it through")
})

describe("a feature-branch goal's repositories", () => {
  it("names each repository by its folder, with its base branch bracketed and its goal branch after", () => {
    const goal = aGoal({
      landing: "feature_branch",
      repos: [
        {
          ...aRepository(),
          path: "/home/me/dev/ariadne",
          base_branch: "main",
          goal_branch: "ship-the-board-000001",
        },
        {
          ...aRepository({ id: "01JREPO0000000000000000002", path: "/home/me/dev/sandbox" }),
          base_branch: "trunk",
          goal_branch: null,
        },
      ],
    })
    mount(goal)

    const list = detail("Repositories")
    expect(list.textContent).toContain("ariadne [main] · ship-the-board-000001")
    // The second repository has no branch of its own yet: no stray separator
    // where there is nothing to follow it.
    expect(list.textContent).toContain("sandbox [trunk]")
    expect(list.textContent).not.toContain("sandbox [trunk] ·")
  })

  it("holds the full path of each repository in its tooltip, and keeps it copyable", async () => {
    const goal = aGoal({ repos: [aRepository({ path: "/home/me/dev/ariadne" })] })
    mount(goal)

    const repo = within(detail("Repositories"))
    expect(repo.getByTitle("/home/me/dev/ariadne")).toBeDefined()
    expect(repo.getByRole("button", { name: "Copy repository path" })).toBeDefined()
  })
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
  it("counts the tasks and the goal's own sessions, and only the goal's own", () => {
    mount(GOAL, {
      tasks: [
        aTask({ id: "01JTASK000000000000000000A" }),
        aTask({ id: "01JTASK000000000000000000B" }),
      ],
      sessions: [
        aSession({ id: "01JSESS000000000000000PLAN", seat: "orchestrator", task_id: null }),
        aSession({ id: "01JSESS000000000000000ENG1", seat: "author" }),
        aSession({ id: "01JSESS000000000000000REV1", seat: "reviewer" }),
      ],
    })

    expect(tab("Tasks").textContent).toBe("Tasks2")
    // The tab holds the goal's own agent, and the sessions its tasks have run
    // are each task panel's: a goal with four sessions under it counts one.
    expect(tab("Sessions").textContent).toBe("Sessions1")

    // A bare number beside a label is read out as "Tasks 2", which says
    // nothing about what the two are; each pill names what it counts.
    expect(within(tab("Tasks")).getByLabelText("2 tasks").textContent).toBe("2")
    expect(within(tab("Sessions")).getByLabelText("1 session").textContent).toBe("1")
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

  it("names an empty sessions tab for the seat it lists, not for the goal", async () => {
    mount(GOAL, { sessions: [aSession({ seat: "author" })] })
    await userEvent.setup().click(tab("Sessions"))

    expect(screen.getByText("No orchestrator session yet")).toBeDefined()
  })
})

it("renders a goal without a modal dialog", () => {
  mount()
  expect(screen.queryByRole("dialog")).toBeNull()
  expect(screen.getByRole("region", { name: GOAL.title })).toBeDefined()
})

/**
 * The shared header's own rows, top to bottom: the title (truncating rather
 * than wrapping, with the full text as its `title` attribute), then the meta
 * row of status, id and the two stamps — the goal panel opens on no
 * breadcrumb, since a goal is never drilled into from anything.
 */
it("opens on the shared header: a truncating title, then status, id and stamps", () => {
  mount()

  const title = screen.getByRole("heading", { name: GOAL.title })
  expect(title.className).toContain("truncate")
  expect(title.getAttribute("title")).toBe(GOAL.title)

  const header = title.parentElement?.parentElement
  if (!header) throw new Error("no header around the title")
  const [titleRow, meta] = [...header.children]
  expect(titleRow instanceof HTMLElement && titleRow.contains(title)).toBe(true)
  if (!(meta instanceof HTMLElement)) throw new Error("no meta row")
  // Status first, then the id, then the stamps.
  const text = meta.textContent ?? ""
  const statusAt = text.indexOf("Active")
  const idAt = text.indexOf(GOAL.id)
  const createdAt = text.indexOf("created")
  expect(statusAt).toBeGreaterThanOrEqual(0)
  expect(statusAt).toBeLessThan(idAt)
  expect(idAt).toBeLessThan(createdAt)
})

it("drills into a session with a breadcrumb back to the goal, and no Back button", async () => {
  const session = aSession({
    id: "01JSESS0000000000000ORC01",
    goal_id: GOAL.id,
    seat: "orchestrator",
    task_id: null,
    task_agent_id: null,
  })
  renderScreen(<GoalPanel goalId={GOAL.id} onClose={() => {}} />, {
    route: `/goals?goal=${GOAL.id}&tab=sessions&session=${session.id}`,
    seed: (client) => {
      client.setQueryData(qk.goals.detail(GOAL.id), GOAL)
      client.setQueryData(qk.sessions.detail(session.id), session)
    },
  })

  const crumb = await screen.findByRole("navigation", { name: "Breadcrumb" })
  expect(within(crumb).getByRole("button", { name: GOAL.title })).toBeDefined()
  expect(screen.queryByRole("button", { name: /^Back to /, hidden: true })).toBeNull()

  const console = await screen.findByLabelText("Console terminal")
  const body = console.closest('[data-slot="pane-body"]')
  const bodyContent = body?.firstElementChild
  const view = bodyContent?.firstElementChild
  expect(view?.className).toContain("min-h-0")
  expect(view?.className).toContain("flex")
  const tabs = view?.querySelector('[data-slot="tabs"]')
  expect(tabs?.className).toContain("min-h-0")
  expect(tabs?.className).toContain("flex")
  const content = console.closest('[data-slot="tabs-content"]')
  expect(content?.className).toContain("min-h-0")
  expect(content?.className).toContain("flex-col")
  const terminalBox = console.parentElement?.parentElement?.parentElement
  expect(terminalBox?.className).toContain("min-h-0")
  expect(terminalBox?.className).toContain("flex")
  expect(terminalBox?.className).not.toContain("min-h-[24rem]")
})

it("renders the loading state inside the pane's scrolling body", () => {
  // Never settling, so the panel stays on its skeleton.
  daemonFetch.mockImplementation(() => new Promise(() => {}))
  renderScreen(<GoalPanel goalId={GOAL.id} onClose={() => {}} />, {
    route: `/goals?goal=${GOAL.id}`,
  })

  const body = document.querySelector('[data-slot="pane-body"]')
  if (!body) throw new Error("no pane body around the loading state")
  expect(body.querySelector('[data-slot="skeleton"]')).not.toBeNull()
})

it("renders the goal's load failure inside the pane's scrolling body", async () => {
  daemonFetch.mockImplementation(() => Promise.resolve(errorResponse(500, "internal", "down")))
  renderScreen(<GoalPanel goalId={GOAL.id} onClose={() => {}} />, {
    route: `/goals?goal=${GOAL.id}`,
  })

  const alert = await screen.findByRole("alert")
  const body = document.querySelector('[data-slot="pane-body"]')
  if (!body) throw new Error("no pane body around the error state")
  expect(body.contains(alert)).toBe(true)
})

it("keeps the cached goal on a failed refetch, with one title and one notice", async () => {
  const { queryClient } = renderScreen(<GoalPanel goalId={GOAL.id} onClose={() => {}} />, {
    route: `/goals?goal=${GOAL.id}`,
    seed: (client) => client.setQueryData(qk.goals.detail(GOAL.id), GOAL),
  })

  daemonFetch.mockImplementation(() => Promise.resolve(errorResponse(500, "internal", "down")))
  await act(() => queryClient.refetchQueries({ queryKey: qk.goals.detail(GOAL.id) }))

  // The cached goal is still shown, under its one title, with one notice
  // above it rather than the panel losing it to the error.
  expect(screen.getAllByRole("heading", { name: GOAL.title })).toHaveLength(1)
  expect(await screen.findByText("Could not refresh this goal")).toBeDefined()
})
