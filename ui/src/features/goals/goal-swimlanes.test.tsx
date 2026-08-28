// @vitest-environment jsdom

/**
 * The board's own answer to "who is asking for me": the badge on the card of a
 * task whose agent is blocked, and the one in the lane header for a planner,
 * which has no card of its own.
 *
 * Rendered rather than unit-tested because the aggregation is already covered
 * (`attention.test.ts` has `collectBoardAttention`); what only the mounted
 * board shows is *where* a reason lands — on the task it belongs to, in the
 * lane of the goal whose planner raised it, and nowhere at all when nothing is
 * stuck.
 *
 * The columns are here for the same reason: which cell a card lands in is a
 * property of the mounted grid, and `ready` folding into Pending is exactly
 * that — as is a goal the planner is still writing a plan for holding every
 * one of its tasks in the first column, whatever each task's own status says.
 *
 * The lane header carries what the whole goal has spent, which is the one
 * figure the board shows without opening anything — and it is a number on a
 * goal nothing has been spent on too, since a lane that goes blank reads as a
 * lane the daemon lost track of.
 */

import { screen, within } from "@testing-library/react"
import { expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { GoalSwimlanes } from "./goal-swimlanes"

const GOAL: GoalDto = aGoal({
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    planner: { input_tokens: 234_567, cached_input_tokens: 200_000, output_tokens: 5_300 },
    engineers: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    reviewers: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
  },
})

const TASK: TaskDto = aTask({
  title: "Wire the strip",
  branch: "wire-the-strip-000001",
  engineer_profile_id: "01JPROF0000000000000000ENG",
  goal_id: GOAL.id,
})

/** An engineer blocked on a prompt: the card's own reason to be badged. */
const SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000001",
  status: "idle",
  attention_reason: "waiting_permission",
  attention_since: "2026-01-01T03:00:00Z",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: "01JPROF0000000000000000ENG",
  tmux_session: "ariadne-01JSESS0000000000000000001",
})

/** The planner, which belongs to no task and so to no card. */
const PLANNER: SessionDto = {
  ...SESSION,
  id: "01JSESS0000000000000000002",
  task_id: undefined,
  role: "planner",
  attention_reason: "waiting_input",
}

function stubDaemon({
  tasks = [] as TaskDto[],
  sessions = [] as SessionDto[],
}: {
  tasks?: TaskDto[]
  sessions?: SessionDto[]
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    const body = url.pathname === "/v1/tasks" ? tasks : sessions
    return Promise.resolve(jsonResponse(body))
  })
}

function renderBoard(goal: GoalDto = GOAL) {
  renderScreen(<GoalSwimlanes goals={[goal]} />, { route: "/goals" })
}

/** The grid cell a card sits in, and its index among the lane's columns. */
function cellOf(card: HTMLElement): { cell: HTMLElement; column: number } {
  const cell = card.closest("a")?.parentElement?.parentElement as HTMLElement
  return { cell, column: [...(cell?.parentElement?.children ?? [])].indexOf(cell) }
}

it("badges the card of the task whose agent is waiting on a person", async () => {
  stubDaemon({ tasks: [TASK], sessions: [SESSION] })
  renderBoard()

  // The same wording the strip and the sessions table use, on the card of the
  // task the blocked agent was working on.
  const badge = await screen.findByText("Waiting for permission")
  expect(badge.closest("a")?.textContent).toContain(TASK.title)
})

it("leaves a task whose agents want nothing alone", async () => {
  stubDaemon({ tasks: [TASK], sessions: [{ ...SESSION, attention_reason: null }] })
  renderBoard()

  expect(await screen.findByText(TASK.title)).not.toBeNull()
  expect(screen.queryByText("Waiting for permission")).toBeNull()
})

it("badges the lane header when a goal's planner is waiting", async () => {
  stubDaemon({ tasks: [TASK], sessions: [PLANNER] })
  renderBoard()

  // A planner has no card, so the lane header — the only place its goal is
  // named — is where it asks.
  const badge = await screen.findByText("Waiting for input")
  expect(badge.closest("header")).not.toBeNull()
  expect(badge.closest("a")).toBeNull()
})

/** The five pipeline columns, in order; `ready` is folded into the first. */
it("lays the pipeline out in five columns", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard()

  await screen.findByText(TASK.title)
  const columns = screen.getAllByRole("heading", { level: 2 })
  expect(columns.map((column) => column.textContent)).toEqual([
    "Pending",
    "In progress",
    "Under review",
    "Approved",
    "Merged",
  ])
})

it("puts a ready task in the Pending column, badged with the status it is really in", async () => {
  const blocked: TaskDto = { ...TASK, id: `${TASK.id}P`, title: "Waiting on a dependency" }
  blocked.status = "pending"
  const ready: TaskDto = { ...TASK, id: `${TASK.id}R`, title: "Retried, no engineer yet" }
  ready.status = "ready"
  stubDaemon({ tasks: [blocked, ready] })
  renderBoard()

  // The card's own box is the link's parent; the cell is the box's.
  const cell = (await screen.findByText(ready.title)).closest("a")?.parentElement?.parentElement
  expect(cell).toBeDefined()
  // First cell of the lane's grid, holding the plainly pending task too: that
  // is what "folded into Pending" looks like on screen.
  expect([...(cell?.parentElement?.children ?? [])].indexOf(cell as Element)).toBe(0)
  expect(cell?.textContent).toContain(blocked.title)

  // And still told apart from the one that is only waiting on a dependency:
  // the sub-status badge is on the ready card and on nothing else.
  expect(within(cell as HTMLElement).getByText("Ready")).not.toBeNull()
  expect(screen.getAllByText("Ready")).toHaveLength(1)
})

it("puts the tasks their engineers are landing in the Approved column", async () => {
  const landing: TaskDto = { ...TASK, id: `${TASK.id}I`, title: "Squashing onto main" }
  landing.status = "approved"
  const published: TaskDto = { ...TASK, id: `${TASK.id}A`, title: "Waiting on its pull request" }
  published.status = "approved"
  stubDaemon({ tasks: [landing, published] })
  renderBoard()

  const cell = (await screen.findByText(landing.title)).closest("a")?.parentElement?.parentElement
  expect(cell).toBeDefined()
  // Fourth cell of the lane's grid — the column the header row calls
  // Approved — and both tasks are in it.
  expect([...(cell?.parentElement?.children ?? [])].indexOf(cell as Element)).toBe(3)
  expect(cell?.textContent).toContain(published.title)
})

it("holds every task of a plan still being written in the first column", async () => {
  const planning: GoalDto = { ...GOAL, status: "planning" }
  const pending: TaskDto = { ...TASK, id: `${TASK.id}P`, title: "Waiting on a dependency" }
  pending.status = "pending"
  const ready: TaskDto = { ...TASK, id: `${TASK.id}R`, title: "Dependencies all merged" }
  ready.status = "ready"
  stubDaemon({ tasks: [pending, ready] })
  renderBoard(planning)

  const { cell, column } = cellOf(await screen.findByText(ready.title))
  expect(column).toBe(0)
  expect(cell.textContent).toContain(pending.title)

  // And each card says what it is really waiting for, which is the planner.
  expect(within(cell).getAllByText("Awaiting plan")).toHaveLength(2)
})

it("says nothing about the plan once the goal is active", async () => {
  stubDaemon({ tasks: [{ ...TASK, status: "ready" }] })
  renderBoard()

  await screen.findByText(TASK.title)
  expect(screen.queryByText("Awaiting plan")).toBeNull()
})

it("carries the goal's own total in the lane header", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard()

  await screen.findByText(TASK.title)
  // In the header, beside the task count and the goal's age — not on a card,
  // which is one task's worth of a figure that is the whole goal's.
  const meta = screen.getByText(/1 task · created/)
  expect(meta.closest("header")).not.toBeNull()
  // Input first and output after it, each behind its own arrow; the word the
  // arrows stand for is there for a screen reader and nowhere else, since the
  // header has no room to spell "tokens" out beside them.
  expect(meta.textContent).toContain("1.2M in, 45k out")
  expect(meta.textContent).not.toContain("tokens")
})

it("says zero for a goal whose agents have spent nothing", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard(aGoal())

  await screen.findByText(TASK.title)
  // Both halves, and a figure rather than a dash: an agent that has spent
  // nothing has spent nothing, which is a number the daemon knows.
  expect(screen.getByText(/1 task · created/).textContent).toContain("0 in, 0 out")
})
