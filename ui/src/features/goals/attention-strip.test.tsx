// @vitest-environment jsdom

/**
 * The strip as the board mounts it: what it shows, and what it links to.
 *
 * Rendered rather than unit-tested because the two things worth checking are
 * not in the aggregation (`attention.test.ts` has that) — that the strip is
 * *absent* when nothing is stuck, and that its rows keep the board's own
 * search params while adding their panel's, which is what makes a panel open
 * over the board instead of replacing it. Only the mounted strip shows either.
 *
 * A plan waiting to be approved is the third kind of row, and the one that is
 * not about anything going wrong: it is the user's move, and its row is how
 * the board says so above the lanes.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { formatAbsolute } from "@/lib/format"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionStrip } from "./attention-strip"

const GOAL: GoalDto = aGoal()

const TASK: TaskDto = aTask({
  title: "Wire the strip",
  branch: "wire-the-strip-000001",
  engineer_profile_id: "01JPROF0000000000000000ENG",
  goal_id: GOAL.id,
})

const SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000001",
  status: "failed",
  // The daemon's flag is the whole reason a session is on the strip; a death
  // it raised nothing for is not.
  attention_reason: "disconnected",
  ended_at: "2026-01-01T02:00:00Z",
  goal_id: GOAL.id,
  profile_id: "01JPROF0000000000000000ENG",
  tmux_session: "ariadne-01JSESS0000000000000000001",
})

/**
 * The three lists the strip reads, each answering its own path. `fails` is the
 * one that does not answer — the three are independent, and a strip that has
 * rows from the other two is a list with a hole in it, not a failure.
 */
function stubDaemon({
  goals = [GOAL],
  tasks = [] as TaskDto[],
  sessions = [] as SessionDto[],
  fails,
}: {
  goals?: GoalDto[]
  tasks?: TaskDto[]
  sessions?: SessionDto[]
  fails?: "/v1/goals" | "/v1/tasks" | "/v1/sessions"
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (url.pathname === fails) return Promise.resolve(new Response("boom", { status: 500 }))
    const body =
      url.pathname === "/v1/goals" ? goals : url.pathname === "/v1/tasks" ? tasks : sessions
    return Promise.resolve(jsonResponse(body))
  })
}

/** The strip where the board puts it: on `/goals`, with a filter already set. */
function renderStrip() {
  return renderScreen(<AttentionStrip />, { route: "/goals?status=active" }).queryClient
}

it("stays out of the way when nothing is stuck", async () => {
  stubDaemon({
    tasks: [
      { ...TASK, status: "in_progress" },
      // Waiting on the engineer the daemon resumes, not on the user.
      { ...TASK, id: "01JTASK0000000000000000002", status: "changes_requested" },
    ],
    // Dead with nothing owed to it: the daemon raised no reason, and neither
    // does the strip.
    sessions: [{ ...SESSION, attention_reason: null }],
  })
  const queryClient = renderStrip()

  // Not "nothing yet": every one of the three lists has answered, and none of
  // them had a row — which is the only state that may render nothing.
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  expect(daemonFetch).toHaveBeenCalledTimes(3)
  expect(screen.queryByRole("region", { name: "Needs attention" })).toBeNull()
})

it("mixes tasks and stuck sessions into one list, each naming its goal", async () => {
  stubDaemon({
    tasks: [{ ...TASK, status: "failed", updated_at: "2026-01-01T01:00:00Z" }],
    sessions: [SESSION],
  })
  renderStrip()

  const rows = await screen.findAllByRole("listitem")
  // The session ended an hour after the task moved, so it leads.
  expect(rows).toHaveLength(2)
  expect(rows[0]?.textContent).toContain("Disconnected")
  expect(rows[1]?.textContent).toContain("Wire the strip")
  for (const row of rows) expect(row.textContent).toContain(GOAL.title)
})

it("shows a live session that is waiting on the user, why, and on what", async () => {
  stubDaemon({
    tasks: [TASK],
    sessions: [
      {
        ...SESSION,
        task_id: TASK.id,
        status: "running",
        ended_at: undefined,
        attention_reason: "waiting_permission",
        attention_since: "2026-01-01T03:00:00Z",
      },
      // Running and nobody's problem: it is not on the list at all.
      {
        ...SESSION,
        id: "01JSESS0000000000000000002",
        status: "running",
        ended_at: undefined,
        attention_reason: null,
      },
    ],
  })
  renderStrip()

  const rows = await screen.findAllByRole("listitem")
  expect(rows).toHaveLength(1)
  // Three separate elements, none of them a tooltip: what it is, who is
  // asking, and what for — all of them readable without a pointer.
  expect(screen.getByText("Waiting for permission")).not.toBeNull()
  expect(screen.getByText("Wire the strip")).not.toBeNull()
  expect(
    screen.getByText("Engineer · The agent is blocked on a permission or approval prompt."),
  ).not.toBeNull()
})

it("names a planner session by its role and the goal it is planning", async () => {
  stubDaemon({
    sessions: [
      {
        ...SESSION,
        role: "planner",
        task_id: null,
        status: "idle",
        ended_at: undefined,
        attention_reason: "waiting_input",
        attention_since: "2026-01-01T03:00:00Z",
      },
    ],
  })
  renderStrip()

  // No task to be named by, and the row still says whose it is and what it is
  // about — the goal, which is all a planner session has.
  expect(await screen.findByText(`Planner · ${GOAL.title}`)).not.toBeNull()
  expect(
    screen.getByText("The agent asked a question and is idle until it is answered."),
  ).not.toBeNull()
})

it("names a task session by the task, even when the task list has not got it", async () => {
  stubDaemon({
    sessions: [
      {
        ...SESSION,
        task_id: TASK.id,
        status: "running",
        ended_at: undefined,
        attention_reason: "waiting_permission",
      },
    ],
  })
  renderStrip()

  // The task list answered without it — a filtered list, a task just created —
  // so the row names it by its short id rather than dropping the subject.
  expect(await screen.findByText(`Task …${TASK.id.slice(-8)}`)).not.toBeNull()
})

it("flags a stalled task without a status of its own to show it", async () => {
  stubDaemon({ tasks: [{ ...TASK, stalled: true }] })
  renderStrip()

  expect(await screen.findByText("Stalled")).not.toBeNull()
})

it("lists a plan the planner has handed over, and opens the goal it belongs to", async () => {
  const planReady: GoalDto = { ...GOAL, status: "plan_ready" }
  // Nothing stuck anywhere: the row is there because the goal is waiting on
  // the reader, not because anything failed.
  stubDaemon({ goals: [planReady], tasks: [{ ...TASK, status: "pending" }] })
  renderStrip()

  const row = await screen.findByRole("listitem")
  expect(row.textContent).toContain(planReady.title)
  expect(row.textContent).toContain("Plan ready")
  expect(row.textContent).toContain("Plan ready for approval")
  expect(within(row).getByRole("link").getAttribute("href")).toBe(
    `/goals?status=active&goal=${planReady.id}`,
  )
})

it("opens each row's panel over the board, keeping the board's filter", async () => {
  stubDaemon({ tasks: [{ ...TASK, status: "failed" }], sessions: [SESSION] })
  renderStrip()

  const links = await screen.findAllByRole("link")
  const hrefs = links.map((link) => link.getAttribute("href"))
  expect(hrefs).toContain(`/goals?status=active&session=${SESSION.id}`)
  expect(hrefs).toContain(`/goals?status=active&task=${TASK.id}`)
})

it("keeps the rows that did load when one of the three lists failed", async () => {
  stubDaemon({ tasks: [{ ...TASK, status: "failed" }], fails: "/v1/sessions" })
  renderStrip()

  const rows = await screen.findAllByRole("listitem")
  expect(rows).toHaveLength(1)
  expect(rows[0]?.textContent).toContain("Wire the strip")
  expect(screen.getByText(/could not be loaded/)).not.toBeNull()
})

/**
 * Two things a row shows only part of: the loose stamp, and the tail of an id.
 * Both said the rest of themselves in a `title=`, which opens for a pointer
 * and for nothing else — so the test is Tab and read, the way the board's
 * cards are tested. (The subject and the goal are their own full text on the
 * row, truncated by CSS alone; there is nothing here that can assert that.)
 */
it("puts what a row shortens in reach of a keyboard", async () => {
  stubDaemon({ tasks: [{ ...TASK, status: "failed" }] })
  renderStrip()
  await screen.findAllByRole("listitem")
  const user = userEvent.setup()

  // In the row's own order, since Tab walks it: the stamp, then the id.
  for (const hint of [`last moved ${formatAbsolute(TASK.updated_at)}`, TASK.id]) {
    expect(await tabUntilHint(user, hint)).toBe(true)
  }
})

/** Tabs until something on screen reads `text`, or runs out of stops. */
async function tabUntilHint(user: ReturnType<typeof userEvent.setup>, text: string) {
  for (let stop = 0; stop < 8; stop++) {
    await user.tab()
    if (screen.queryAllByText(text).length > 0) return true
  }
  return false
}
