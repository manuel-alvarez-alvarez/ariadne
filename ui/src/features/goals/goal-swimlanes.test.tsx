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
 * one of its tasks in the first column, whatever each task's own status says,
 * and a failed task sitting in that column as the retry candidate it is.
 *
 * And the reading order, which is the whole point of a board: what is asking
 * for a person, then what is running, then what is done with — the last of
 * those folded down to the one line its header says.
 *
 * The lane header carries what the whole goal has spent, which is the one
 * figure the board shows without opening anything — and it is a number on a
 * goal nothing has been spent on too, since a lane that goes blank reads as a
 * lane the daemon lost track of.
 */

import { cleanup, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { aGoal, aRepository, aSession, aTask } from "@/test/fixtures"
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

// The collapsed lanes live in `localStorage`, which the suite's stand-in keeps
// for the whole file: one test folding a lane away must not reach the next.
beforeEach(() => localStorage.clear())

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

function renderBoard(goals: GoalDto | GoalDto[] = GOAL) {
  renderScreen(<GoalSwimlanes goals={Array.isArray(goals) ? goals : [goals]} />, {
    route: "/goals",
  })
}

/** The grid cell a card sits in, and its index among the lane's columns. */
function cellOf(card: HTMLElement): { cell: HTMLElement; column: number } {
  const cell = card.closest("a")?.parentElement?.parentElement as HTMLElement
  return { cell, column: [...(cell?.parentElement?.children ?? [])].indexOf(cell) }
}

/** The goals the board is showing, top to bottom — its whole reading order. */
function laneOrder(): string[] {
  const board = screen.getByRole("region", { name: "Goals board" })
  return [...board.querySelectorAll("section > header")].map(
    (header) => header.querySelector("a")?.textContent ?? "",
  )
}

/** The lane header naming `title`, which is where a folded lane says everything. */
function laneHeader(title: string): HTMLElement {
  const header = screen.getByText(title).closest("header")
  if (!header) throw new Error(`no lane header for "${title}"`)
  return header
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

it("keeps a failed task in the Pending column, outlined and named", async () => {
  const failed: TaskDto = { ...TASK, id: `${TASK.id}F`, title: "Died on the merge" }
  failed.status = "failed"
  stubDaemon({ tasks: [failed] })
  renderBoard()

  // Where a retry would put it back, not in a box under the lane: the one
  // thing anybody does with a failure is start it again.
  const { cell, column } = cellOf(await screen.findByText(failed.title))
  expect(column).toBe(0)
  // The outline is what catches the eye in a column of tasks that have simply
  // not started; the badge is what names it.
  expect(cell.querySelector(".border-status-danger\\/40")).not.toBeNull()
  expect(within(cell).getByText("Failed")).not.toBeNull()
  expect(screen.queryByText("Cancelled")).toBeNull()
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

  // What they are waiting for is the lane's own status badge, said once —
  // never repeated on every card in the lane.
  expect(laneHeader(planning.title).textContent).toContain("Planning")
})

it("carries the goal's own total in the lane header", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard()

  await screen.findByText(TASK.title)
  // In the header, beside where the lane is up to and the goal's age — not on
  // a card, which is one task's worth of a figure that is the whole goal's.
  const meta = screen.getByText(/0\/1 merged · created/)
  expect(meta.closest("header")).not.toBeNull()
  // Input first and output after it, each behind its own arrow; the word the
  // arrows stand for is there for a screen reader and nowhere else, since the
  // header has no room to spell "tokens" out beside them.
  expect(meta.textContent).toContain("1.2M in, 89% cached, 45k out")
  expect(meta.textContent).not.toContain("tokens")
})

it("says zero for a goal whose agents have spent nothing", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard(aGoal())

  await screen.findByText(TASK.title)
  // Both halves, and a figure rather than a dash: an agent that has spent
  // nothing has spent nothing, which is a number the daemon knows.
  expect(screen.getByText(/0\/1 merged · created/).textContent).toContain("0 in, 0% cached, 0 out")
})

// ── The repository, in the header itself ──────────────────────────────────

it("names the goal's repository and branch in the lane header", async () => {
  const goal = aGoal({
    repos: [aRepository({ path: "/home/me/dev/ariadne", base_branch: "main" })],
  })
  stubDaemon({ tasks: [{ ...TASK, goal_id: goal.id }] })
  renderBoard(goal)

  await screen.findByText(TASK.title)
  expect(laneHeader(goal.title).textContent).toContain("ariadne [main]")
})

it("joins several repositories, folder and branch, in the order the goal has them", async () => {
  const goal = aGoal({
    repos: [
      aRepository({
        id: "01JREPO0000000000000000001",
        path: "/home/me/dev/a",
        base_branch: "main",
      }),
      aRepository({
        id: "01JREPO0000000000000000002",
        path: "/home/me/dev/b",
        base_branch: "next",
      }),
    ],
  })
  stubDaemon({ tasks: [{ ...TASK, goal_id: goal.id }] })
  renderBoard(goal)

  await screen.findByText(TASK.title)
  expect(laneHeader(goal.title).textContent).toContain("a [main], b [next]")
})

it("shows nothing extra for a goal on no repository", async () => {
  stubDaemon({ tasks: [TASK] })
  renderBoard(aGoal({ repos: [] }))

  await screen.findByText(TASK.title)
  // No trailing separator or empty span left where the summary would sit.
  expect(laneHeader(GOAL.title).textContent).not.toMatch(/\[.*\]/)
})

// ── Where the lane is up to ───────────────────────────────────────────────

it("counts how far through the pipeline the lane is, and what is stuck in it", async () => {
  const tasks = [
    ...statuses("merged", 3),
    ...statuses("in_progress", 2),
    ...statuses("failed", 1),
    ...statuses("pending", 1),
  ]
  stubDaemon({ tasks })
  renderBoard()

  await screen.findByText("merged 1")
  // "N tasks" was the one number about a goal that stops changing the moment
  // the planner is done; this is the one that does not.
  expect(laneHeader(GOAL.title).textContent).toContain("3/7 merged · 1 failed · 1 waiting")
})

it("says nothing about what a lane has none of", async () => {
  stubDaemon({ tasks: statuses("in_progress", 2) })
  renderBoard()

  await screen.findByText("in_progress 1")
  const header = laneHeader(GOAL.title).textContent ?? ""
  expect(header).toContain("0/2 merged")
  expect(header).not.toContain("failed")
  expect(header).not.toContain("waiting")
})

// ── The reading order, and what a finished lane says ──────────────────────

/** An old goal still being worked on: the lane the board is open for. */
const ACTIVE: GoalDto = aGoal({
  id: "01JGOAL0000000000000000001",
  title: "The old active goal",
  status: "active",
  updated_at: "2026-01-01T00:00:00Z",
})

/** Finished after it, which is what used to put it on top. */
const COMPLETED: GoalDto = aGoal({
  id: "01JGOAL0000000000000000002",
  title: "The newer finished goal",
  status: "completed",
  updated_at: "2026-03-01T00:00:00Z",
})

const PLANNING: GoalDto = aGoal({
  id: "01JGOAL0000000000000000003",
  title: "The goal being planned",
  status: "planning",
  updated_at: "2026-02-01T00:00:00Z",
})

/** The three, newest id first — the order the goals list arrives in. */
const THREE = [PLANNING, COMPLETED, ACTIVE]

it("puts the work that is still moving above the work that is done with", async () => {
  stubDaemon({ tasks: [{ ...TASK, goal_id: ACTIVE.id }] })
  renderBoard(THREE)

  await screen.findByText(TASK.title)
  // Newest-first by id put the finished goal on top of an active one older
  // than it, which is the wrong way round on a board open to answer "what
  // now". Within a band, whichever moved last leads.
  expect(laneOrder()).toEqual([PLANNING.title, ACTIVE.title, COMPLETED.title])
})

it("puts a lane that is asking for a person above every other lane", async () => {
  const failed: TaskDto = { ...TASK, id: `${TASK.id}F`, goal_id: ACTIVE.id, title: "It broke" }
  failed.status = "failed"
  stubDaemon({ tasks: [failed] })
  renderBoard(THREE)

  // Above the goal that moved more recently than it: a lane with something
  // stuck in it is the reason the board is open.
  await screen.findByText(failed.title)
  expect(laneOrder()).toEqual([ACTIVE.title, PLANNING.title, COMPLETED.title])
})

it("opens a finished lane as the one line its header says", async () => {
  const tasks = [
    ...statuses("merged", 7).map((task) => ({ ...task, goal_id: COMPLETED.id })),
    ...statuses("cancelled", 1).map((task) => ({ ...task, goal_id: COMPLETED.id })),
  ]
  stubDaemon({ tasks })
  renderBoard(COMPLETED)

  // Expanded, this is five columns with every card in the far-right one: a
  // 270px box that is 85% empty, and on a 900px screen it reads as an empty
  // lane because the cards are scrolled off it.
  const header = await screen.findByText(/7 tasks merged · 1 cancelled/)
  expect(header.closest("header")).not.toBeNull()
  expect(screen.queryByText("merged 1")).toBeNull()
})

it("opens a lane that is still running, and remembers a finished one the user opened", async () => {
  stubDaemon({
    tasks: [
      { ...TASK, goal_id: ACTIVE.id },
      { ...TASK, id: `${TASK.id}M`, goal_id: COMPLETED.id, title: "Landed", status: "merged" },
    ],
  })
  const user = userEvent.setup()
  renderBoard([COMPLETED, ACTIVE])

  // The lane that is running is open; the finished one is not.
  expect(await screen.findByText(TASK.title)).not.toBeNull()
  expect(screen.queryByText("Landed")).toBeNull()

  await user.click(screen.getByRole("button", { name: `Expand ${COMPLETED.title}` }))
  expect(screen.getByText("Landed")).not.toBeNull()

  // And the answer is the user's from here on: it survives a remount, where
  // the board's own default would fold the lane away again.
  cleanup()
  renderBoard([COMPLETED, ACTIVE])
  expect(await screen.findByText("Landed")).not.toBeNull()
})

/** `count` tasks at one status, told apart by their titles and their ids. */
function statuses(status: TaskDto["status"], count: number): TaskDto[] {
  return Array.from({ length: count }, (_, index) => ({
    ...TASK,
    id: `${TASK.id}${status}${index}`,
    title: `${status} ${index + 1}`,
    status,
  }))
}
