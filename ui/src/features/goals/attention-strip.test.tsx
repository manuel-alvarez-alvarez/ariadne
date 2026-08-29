// @vitest-environment jsdom

/**
 * The strip as the board mounts it: what it shows, and what it links to.
 *
 * Rendered rather than unit-tested because none of this is in the aggregation
 * (`attention.test.ts` has that) — that the strip is *absent* when nothing is
 * stuck and present when nothing could be loaded, that a long list is counted
 * rather than clipped, and that each row links at the control its reason is
 * answered through while keeping the board's own search params, which is what
 * makes a panel open over the board instead of replacing it. Only the mounted
 * strip shows any of it.
 */

import { screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"
import { formatAbsolute } from "@/lib/format"
import { aGoal, aSession, aTask } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { AttentionStrip } from "./attention-strip"

const GOAL: GoalDto = aGoal()

const ENGINEER = "01JPROF0000000000000000ENG"
const PLANNER = "01JPROF0000000000000000PLA"

const TASK: TaskDto = aTask({
  title: "Wire the strip",
  branch: "wire-the-strip-000001",
  engineer_profile_id: ENGINEER,
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
  task_id: TASK.id,
  profile_id: ENGINEER,
  tmux_session: "ariadne-01JSESS0000000000000000001",
})

/** A planner's: no task, and a thread of the goal's rather than of a task's. */
const PLANNER_SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000009",
  role: "planner",
  task_id: null,
  status: "idle",
  ended_at: undefined,
  attention_reason: "waiting_user",
  attention_since: "2026-01-01T03:00:00Z",
  goal_id: GOAL.id,
  profile_id: PLANNER,
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
  fails?: "/v1/goals" | "/v1/tasks" | "/v1/sessions" | "all"
} = {}) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(
      typeof input === "string" ? input : input instanceof URL ? input : input.url,
    )
    if (fails === "all" || url.pathname === fails) {
      return Promise.resolve(new Response("boom", { status: 500 }))
    }
    const body =
      url.pathname === "/v1/goals" ? goals : url.pathname === "/v1/tasks" ? tasks : sessions
    return Promise.resolve(jsonResponse(body))
  })
}

/** The strip where the board puts it: on `/goals`, with a filter already set. */
function renderStrip() {
  return renderScreen(<AttentionStrip />, { route: "/goals?status=active" }).queryClient
}

/** Every row's link target, which is the whole of where the strip sends you. */
async function hrefs(): Promise<(string | null)[]> {
  const links = await screen.findAllByRole("link")
  return links.map((link) => link.getAttribute("href"))
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

// "Nothing is stuck" and "nothing has answered yet" are the same empty list
// and must not read the same.
it("holds a placeholder while the lists are still loading", async () => {
  renderStrip()

  expect(await screen.findByRole("status", { name: "Loading what needs attention" })).not.toBeNull()
})

// The failure that used to read as an all-clear board.
it("says so when the list could not be loaded at all", async () => {
  stubDaemon({ fails: "all" })
  const queryClient = renderStrip()

  expect(await screen.findByText(/Could not load what needs attention/)).not.toBeNull()
  expect(screen.queryAllByRole("listitem")).toHaveLength(0)

  await userEvent.click(screen.getByRole("button", { name: "Retry" }))
  await waitFor(() => expect(queryClient.isFetching()).toBe(0))
  // Three lists asked again, on top of the three that failed.
  expect(daemonFetch).toHaveBeenCalledTimes(6)
})

it("mixes tasks and stuck sessions into one list, each naming its goal", async () => {
  stubDaemon({
    tasks: [{ ...TASK, status: "failed", updated_at: "2026-01-01T01:00:00Z" }],
    sessions: [PLANNER_SESSION],
  })
  renderStrip()

  const rows = await screen.findAllByRole("listitem")
  // The planner raised its reason two hours after the task moved, so it leads.
  expect(rows).toHaveLength(2)
  expect(rows[0]?.textContent).toContain("Waiting for you")
  expect(rows[1]?.textContent).toContain("Wire the strip")
  for (const row of rows) expect(row.textContent).toContain(GOAL.title)
})

// The failed task and the agent that died under it are one thing gone wrong,
// and used to be two rows saying it a line apart.
it("shows a task and the session that was running it as one row", async () => {
  stubDaemon({
    tasks: [{ ...TASK, status: "failed" }],
    sessions: [{ ...SESSION, attention_reason: "agent_error" }],
  })
  renderStrip()

  const rows = await screen.findAllByRole("listitem")
  expect(rows).toHaveLength(1)
  // Both reasons, on the one row: the task's status and the session's flag.
  expect(rows[0]?.textContent).toContain("Failed")
  expect(rows[0]?.textContent).toContain("Agent error")
  expect(rows[0]?.textContent).toContain("Wire the strip")
})

it("shows a live session that is waiting on the user, why, and on what", async () => {
  stubDaemon({
    tasks: [TASK],
    sessions: [
      {
        ...SESSION,
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
    sessions: [{ ...PLANNER_SESSION, attention_reason: "waiting_input" }],
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

// An agent waiting on a *person* asked its question in a thread, so the row
// opens that thread with the box focused and addressed to whoever asked —
// where it used to open a pane with nothing to type into.
it("sends a question in a task thread to that thread's compose box", async () => {
  stubDaemon({
    tasks: [TASK],
    sessions: [
      { ...SESSION, status: "idle", ended_at: undefined, attention_reason: "waiting_user" },
    ],
  })
  renderStrip()

  expect(await hrefs()).toEqual([
    `/goals?status=active&task=${TASK.id}&tab=conversation&focus=composer&to=${ENGINEER}`,
  ])
})

it("sends a planner's question to the goal thread, the only one it has", async () => {
  stubDaemon({ sessions: [PLANNER_SESSION] })
  renderStrip()

  expect(await hrefs()).toEqual([
    `/goals?status=active&goal=${GOAL.id}&tab=thread&focus=composer&to=${PLANNER}`,
  ])
})

// A prompt is answered with a keystroke, so this one does open the pane — and
// hands it the keyboard on the way in.
it("sends a blocked agent to its terminal, focused", async () => {
  stubDaemon({
    tasks: [TASK],
    sessions: [
      {
        ...SESSION,
        status: "running",
        ended_at: undefined,
        attention_reason: "waiting_permission",
      },
    ],
  })
  renderStrip()

  expect(await hrefs()).toEqual([
    `/goals?status=active&session=${SESSION.id}&tab=terminal&focus=terminal`,
  ])
})

it("opens what is only to be read where it always did, keeping the board's filter", async () => {
  stubDaemon({
    tasks: [{ ...TASK, status: "failed" }],
    // Another task's, so the two stay two rows.
    sessions: [{ ...SESSION, task_id: null }],
  })
  renderStrip()

  const targets = await hrefs()
  expect(targets).toContain(`/goals?status=active&session=${SESSION.id}`)
  expect(targets).toContain(`/goals?status=active&task=${TASK.id}`)
})

// The old strip capped its list at a height and let the rest scroll inside it,
// which on macOS is a row nobody can see and no scrollbar to say so.
it("counts the rows it does not show rather than clipping them", async () => {
  stubDaemon({
    tasks: Array.from({ length: 7 }, (_, index) => ({
      ...TASK,
      id: `01JTASK000000000000000000${index}`,
      title: `Stuck task ${index}`,
      status: "failed" as const,
      updated_at: `2026-01-0${index + 1}T00:00:00Z`,
    })),
  })
  renderStrip()

  expect(await screen.findByText("7 items")).not.toBeNull()
  expect(screen.getAllByRole("listitem")).toHaveLength(5)

  await userEvent.click(screen.getByRole("button", { name: "2 more…" }))
  expect(screen.getAllByRole("listitem")).toHaveLength(7)

  await userEvent.click(screen.getByRole("button", { name: "Show fewer" }))
  expect(screen.getAllByRole("listitem")).toHaveLength(5)
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
