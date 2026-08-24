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
 * that.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, within } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { TooltipProvider } from "@/components/ui/tooltip"

import { GoalSwimlanes } from "./goal-swimlanes"

/**
 * Hoisted, and not `vi.stubGlobal`: `openapi-fetch` takes its `fetch` when the
 * client is built, which is when `@/api` is imported — a stub installed after
 * that is a stub the daemon client never sees.
 */
const { daemonFetch } = vi.hoisted(() => {
  const daemonFetch = vi.fn()
  globalThis.fetch = daemonFetch as unknown as typeof fetch
  return { daemonFetch }
})

const GOAL: GoalDto = {
  id: "01JGOAL0000000000000000001",
  title: "Ship the board",
  description: "",
  planner_profile_id: "01JPROF00000000000000PLAN",
  repos: [],
  required_approvals: 1,
  status: "active",
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: GOAL.id,
  repo_id: "01JREPO0000000000000000001",
  title: "Wire the strip",
  description: "",
  status: "in_progress",
  branch: "wire-the-strip-000001",
  depends_on: [],
  engineer_profile_id: "01JPROF0000000000000000ENG",
  integrator_profile_id: "01INTEGRATOR",
  reviewers: [],
  review_round: 0,
  stalled: false,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

/** An engineer blocked on a prompt: the card's own reason to be badged. */
const SESSION: SessionDto = {
  id: "01JSESS0000000000000000001",
  goal_id: GOAL.id,
  task_id: TASK.id,
  profile_id: "01JPROF0000000000000000ENG",
  role: "engineer",
  agent_kind: "claude_code",
  status: "idle",
  attention_reason: "waiting_permission",
  attention_since: "2026-01-01T03:00:00Z",
  tmux_session: "ariadne-01JSESS0000000000000000001",
  created_at: "2026-01-01T00:00:00Z",
}

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
    return Promise.resolve(
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    )
  })
}

function renderBoard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  })
  render(
    <MemoryRouter initialEntries={["/goals"]}>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delay={0}>
          <GoalSwimlanes goals={[GOAL]} />
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  // jsdom lays nothing out, so it does not implement this.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  daemonFetch.mockReset()
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

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
    "Integrating",
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

it("puts an integrating task — and the approved one behind it — in the Integrating column", async () => {
  const landing: TaskDto = { ...TASK, id: `${TASK.id}I`, title: "Rebasing onto main" }
  landing.status = "integrating"
  const approved: TaskDto = { ...TASK, id: `${TASK.id}A`, title: "Waiting for its integrator" }
  approved.status = "approved"
  stubDaemon({ tasks: [landing, approved] })
  renderBoard()

  const cell = (await screen.findByText(landing.title)).closest("a")?.parentElement?.parentElement
  expect(cell).toBeDefined()
  // Fourth cell of the lane's grid — the column the header row calls
  // Integrating — and both tasks are in it.
  expect([...(cell?.parentElement?.children ?? [])].indexOf(cell as Element)).toBe(3)
  expect(cell?.textContent).toContain(approved.title)

  // `approved` keeps saying what it really is, on the card and nowhere else.
  expect(within(cell as HTMLElement).getByText("Approved")).not.toBeNull()
  expect(screen.getAllByText("Approved")).toHaveLength(1)
})
