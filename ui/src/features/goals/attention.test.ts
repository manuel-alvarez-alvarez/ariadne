// @vitest-environment jsdom

/**
 * What the attention list is made of: which tasks and sessions belong on it,
 * how the two fold into one row apiece, and what the hook the screens read it
 * through says while the three lists are still loading or one of them failed.
 *
 * jsdom for the hook alone — the collection below it is pure, and everything
 * that renders a row is `attention-strip.test.tsx`.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { renderHook, waitFor } from "@testing-library/react"
import { createElement, type ReactNode } from "react"
import { describe, expect, it } from "vitest"

import type { GoalDto, SessionDto, TaskDto } from "@/api"

import { sessionAttention } from "@/features/sessions/session-display"
import { paths } from "@/routes/paths"
import { daemonFetch, jsonResponse } from "@/test/harness"

import {
  attentionTarget,
  collectAttention,
  collectBoardAttention,
  taskAttentionReason,
  useAttention,
} from "./attention"

/** A row nobody has reported tokens for, which is every row here. */
const NO_TOKENS = { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 }

function task(overrides: Partial<TaskDto>): TaskDto {
  return {
    branch: "a-task-aaa111",
    created_at: "2026-08-16T10:00:00Z",
    depends_on: [],
    description: "",
    engineer_profile_id: "p1",
    goal_id: "g1",
    id: "t1",
    repo_id: "r1",
    review_round: 0,
    reviewers: [],
    stalled: false,
    status: "in_progress",
    title: "A task",
    usage: { total: NO_TOKENS, engineer: NO_TOKENS, reviewers: [] },
    updated_at: "2026-08-16T10:00:00Z",
    ...overrides,
  }
}

/**
 * A session the daemon has flagged — which is the only way one gets onto the
 * list. `attention_reason: null` is what the tests below hand a session that
 * has nothing owed to it, whatever its status.
 */
function session(overrides: Partial<SessionDto>): SessionDto {
  return {
    agent_kind: "claude_code",
    attention_reason: "disconnected",
    created_at: "2026-08-16T10:00:00Z",
    goal_id: "g1",
    id: "s1",
    profile_id: "p1",
    role: "engineer",
    status: "failed",
    tmux_session: "ariadne-s1",
    usage: NO_TOKENS,
    ...overrides,
  }
}

function goal(overrides: Partial<GoalDto>): GoalDto {
  return {
    created_at: "2026-08-16T09:00:00Z",
    description: "",
    id: "g1",
    planner_profile_id: "p1",
    repos: [],
    required_approvals: 1,
    status: "active",
    title: "A goal",
    usage: {
      total: NO_TOKENS,
      planner: NO_TOKENS,
      engineers: NO_TOKENS,
      reviewers: NO_TOKENS,
    },
    updated_at: "2026-08-16T09:00:00Z",
    ...overrides,
  }
}

describe("taskAttentionReason", () => {
  it("leaves a task that is simply making progress alone", () => {
    expect(taskAttentionReason(task({ status: "in_progress" }))).toBeNull()
    expect(taskAttentionReason(task({ status: "merged" }))).toBeNull()
    expect(taskAttentionReason(task({ status: "under_review" }))).toBeNull()
  })

  // The reviewer has spoken and the daemon resumes the engineer itself, so
  // the task is waiting on an agent rather than on a person.
  it("leaves a task whose review asked for changes alone", () => {
    expect(taskAttentionReason(task({ status: "changes_requested" }))).toBeNull()
  })

  it("reports the two states that want the user", () => {
    expect(taskAttentionReason(task({ status: "failed" }))).toBe("failed")
    expect(taskAttentionReason(task({ stalled: true }))).toBe("stalled")
  })

  it("prefers the status over the stall flag on top of it", () => {
    expect(taskAttentionReason(task({ status: "failed", stalled: true }))).toBe("failed")
  })

  // A stall is a flag on top of any status, including the one that is not a
  // reason of its own.
  it("still reports a task that stalled while changes were requested", () => {
    expect(taskAttentionReason(task({ status: "changes_requested", stalled: true }))).toBe(
      "stalled",
    )
  })
})

describe("sessionAttention", () => {
  it("leaves a session the daemon raised nothing for alone", () => {
    expect(sessionAttention(session({ status: "running", attention_reason: null }))).toBeNull()
    expect(sessionAttention(session({ status: "idle", attention_reason: null }))).toBeNull()
    expect(sessionAttention(session({ status: "exited", attention_reason: null }))).toBeNull()
  })

  // The daemon flags the agent it still owes work to as `disconnected` and
  // leaves the rest alone: a reviewer that exited after voting is finished,
  // not stuck, and the stored reason is what says which.
  it("leaves a dead session the daemon raised nothing for alone", () => {
    expect(sessionAttention(session({ status: "failed", attention_reason: null }))).toBeNull()
  })

  // The same five the daemon can raise, and nothing else.
  it("reports every reason the daemon can flag", () => {
    expect(
      sessionAttention(session({ status: "running", attention_reason: "waiting_permission" })),
    ).toBe("waiting_permission")
    expect(sessionAttention(session({ status: "idle", attention_reason: "waiting_input" }))).toBe(
      "waiting_input",
    )
    expect(sessionAttention(session({ status: "running", attention_reason: "agent_error" }))).toBe(
      "agent_error",
    )
    expect(sessionAttention(session({ status: "running", attention_reason: "disconnected" }))).toBe(
      "disconnected",
    )
    expect(sessionAttention(session({ status: "idle", attention_reason: "stalled" }))).toBe(
      "stalled",
    )
  })

  it("keeps the reason of a session that died after raising it", () => {
    expect(sessionAttention(session({ status: "failed", attention_reason: "agent_error" }))).toBe(
      "agent_error",
    )
  })

  // `waiting_user` used to mean a question sat in a thread the app no longer
  // has; the daemon may still raise it, but it is not a reason to add a row.
  it("no longer surfaces waiting_user, which was a question in a thread", () => {
    expect(
      sessionAttention(session({ status: "idle", attention_reason: "waiting_user" })),
    ).toBeNull()
  })
})

describe("collectAttention", () => {
  it("keeps only the tasks that want the user", () => {
    const items = collectAttention(
      [goal({})],
      [
        task({ id: "t1", status: "in_progress" }),
        task({ id: "t2", status: "failed" }),
        task({ id: "t3", stalled: true }),
      ],
      [],
    )

    expect(items.map((item) => item.id)).toEqual(["t2", "t3"])
    expect(items.map((item) => item.taskReason)).toEqual(["failed", "stalled"])
    expect(items.every((item) => item.session === undefined)).toBe(true)
  })

  it("keeps only the sessions that want the user", () => {
    const items = collectAttention(
      [goal({})],
      [],
      [
        session({ id: "s1", status: "running", attention_reason: null }),
        // Dead, and nothing owed to it: the daemon raised no reason, so it is
        // no more on the list than the healthy one above.
        session({ id: "s2", status: "failed", attention_reason: null }),
        session({ id: "s3", status: "failed", ended_at: "2026-08-16T10:00:00Z" }),
        session({
          id: "s4",
          status: "idle",
          attention_reason: "waiting_permission",
          attention_since: "2026-08-16T12:00:00Z",
        }),
      ],
    )

    expect(items.map((item) => item.id)).toEqual(["s4", "s3"])
    expect(items.map((item) => item.sessionReason)).toEqual(["waiting_permission", "disconnected"])
    expect(items.every((item) => item.taskReason === null)).toBe(true)
  })

  it("mixes tasks and sessions into one list, most recently moved first", () => {
    const items = collectAttention(
      [goal({})],
      [
        task({ id: "t1", status: "failed", updated_at: "2026-08-16T10:00:00Z" }),
        task({ id: "t2", stalled: true, updated_at: "2026-08-16T12:00:00Z" }),
      ],
      [session({ id: "s1", ended_at: "2026-08-16T11:00:00Z" })],
    )

    expect(items.map((item) => item.id)).toEqual(["t2", "s1", "t1"])
  })

  // The pair is one thing gone wrong — the agent died and the task it was
  // running failed with it — and two rows a line apart said it twice.
  it("folds a flagged session onto the row of the task it was running", () => {
    const items = collectAttention(
      [goal({})],
      [task({ id: "t1", status: "failed", updated_at: "2026-08-16T10:00:00Z" })],
      [
        session({
          id: "s1",
          task_id: "t1",
          status: "failed",
          attention_reason: "agent_error",
          ended_at: "2026-08-16T11:00:00Z",
        }),
      ],
    )

    expect(items).toHaveLength(1)
    expect(items[0]?.id).toBe("t1")
    expect(items[0]?.taskReason).toBe("failed")
    expect(items[0]?.sessionReason).toBe("agent_error")
    // The row is as recent as the later of its two reasons, which is what the
    // list is ordered by and what the row claims as "last moved".
    expect(items[0]?.at).toBe("2026-08-16T11:00:00Z")
  })

  // Several sessions of one task can be flagged at once — this round's
  // engineer waiting on a prompt while last round's reviewer sits
  // disconnected — and the row keeps the one the user has not seen yet.
  it("keeps the most recently raised of a task's flagged sessions", () => {
    const items = collectAttention(
      [goal({})],
      [],
      [
        session({
          id: "s1",
          task_id: "t1",
          attention_reason: "disconnected",
          attention_since: "2026-08-16T10:00:00Z",
        }),
        session({
          id: "s2",
          task_id: "t1",
          attention_reason: "waiting_permission",
          attention_since: "2026-08-16T12:00:00Z",
        }),
        session({
          id: "s3",
          task_id: "t1",
          attention_reason: "stalled",
          attention_since: "2026-08-16T11:00:00Z",
        }),
      ],
    )

    expect(items).toHaveLength(1)
    expect(items[0]?.id).toBe("t1")
    expect(items[0]?.session?.id).toBe("s2")
    expect(items[0]?.sessionReason).toBe("waiting_permission")
  })

  // It belongs to no task, so there is no row for it to fold into.
  it("gives a planner session a row of its own", () => {
    const items = collectAttention(
      [goal({})],
      [task({ id: "t1", status: "failed" })],
      [
        session({ id: "s1", role: "planner", attention_reason: "disconnected" }),
        session({ id: "s2", task_id: "t1", attention_reason: "agent_error" }),
      ],
    )

    expect(items.map((item) => item.id).sort()).toEqual(["s1", "t1"])
  })

  it("ages a session that has no end by when it started", () => {
    const [item] = collectAttention([], [], [session({ created_at: "2026-08-16T08:00:00Z" })])

    expect(item?.at).toBe("2026-08-16T08:00:00Z")
  })

  // The row is about the waiting, not about the agent: a session that has been
  // blocked on a prompt for an hour reads as an hour old, not as however long
  // ago it started.
  it("ages a flagged session by when its reason was raised", () => {
    const [item] = collectAttention(
      [],
      [],
      [
        session({
          status: "running",
          attention_reason: "waiting_permission",
          attention_since: "2026-08-16T11:00:00Z",
          created_at: "2026-08-16T08:00:00Z",
        }),
      ],
    )

    expect(item?.at).toBe("2026-08-16T11:00:00Z")
  })

  // The goal is where the row sits; the task is what the agent was doing.
  it("names the task a session was run for, and nothing for a planner's", () => {
    const items = collectAttention(
      [goal({})],
      [task({ id: "t1", title: "Wire the strip" })],
      [
        session({ id: "s1", task_id: "t1" }),
        session({ id: "s2", role: "planner" }),
        session({ id: "s3", task_id: "gone" }),
      ],
    )

    const byId = new Map(items.map((item) => [item.id, item]))
    expect(byId.get("t1")?.task?.title).toBe("Wire the strip")
    expect(byId.get("s2")?.taskId).toBeNull()
    // The task list did not carry it, and the row is still that task's.
    expect(byId.get("gone")?.taskId).toBe("gone")
    expect(byId.get("gone")?.task).toBeUndefined()
  })

  it("names the goal each row belongs to, and keeps rows whose goal is missing", () => {
    const items = collectAttention(
      [goal({ id: "g1", title: "Known" })],
      [
        task({ id: "t1", goal_id: "g1", status: "failed", updated_at: "2026-08-16T12:00:00Z" }),
        task({ id: "t2", goal_id: "g9", status: "failed", updated_at: "2026-08-16T11:00:00Z" }),
      ],
      [],
    )

    expect(items[0]?.goal?.title).toBe("Known")
    expect(items[1]?.goalId).toBe("g9")
    expect(items[1]?.goal).toBeUndefined()
  })

  // A failed query leaves its list undefined while the others answered: the
  // rows that did load are still a list, not an empty screen.
  it("reads a list that only partly loaded", () => {
    const items = collectAttention(undefined, undefined, [session({ id: "s1" })])

    expect(items.map((item) => item.id)).toEqual(["s1"])
    expect(items[0]?.goal).toBeUndefined()
  })
})
/**
 * Where a row lands is `attention-strip.test.tsx`'s subject for the board it
 * was written on. What is left for here is the screen the answer is given
 * *from*: the list is carried onto every one of them (`attention-alerts.tsx`),
 * and the sessions screen reads `?goal=` and `?task=` as its own filters rather
 * than as panels — so a row answered there has to leave for the board rather
 * than quietly narrowing the list under the alert.
 */
describe("attentionTarget, from the screen it is answered on", () => {
  const FAILED = task({ id: "t9", status: "failed" })

  it("opens a task on the board from the screen whose `?task=` is a filter", () => {
    const [item] = collectAttention([], [FAILED], [])
    if (!item) throw new Error("nothing was collected")

    expect(attentionTarget(item, new URLSearchParams("goal=g1"), paths.sessions())).toEqual({
      pathname: paths.goals(),
      search: "?task=t9",
    })
  })

  it("stacks it on the screen it was answered from anywhere else", () => {
    const [item] = collectAttention([], [FAILED], [])
    if (!item) throw new Error("nothing was collected")

    const target = attentionTarget(item, new URLSearchParams("status=active"), paths.goals())
    expect(target.pathname).toBeUndefined()
    expect(new URLSearchParams(target.search).get("task")).toBe("t9")
  })

  it("leaves the sessions screen's filters under a session it opens there", () => {
    const flagged = session({ id: "s9", task_id: null, attention_reason: "disconnected" })
    const [item] = collectAttention([], [], [flagged])
    if (!item) throw new Error("nothing was collected")

    const params = new URLSearchParams(
      attentionTarget(item, new URLSearchParams("goal=g1&task=t1"), paths.sessions()).search,
    )
    expect(params.get("session")).toBe("s9")
    expect(params.get("goal")).toBe("g1")
    expect(params.get("task")).toBe("t1")
  })
})

describe("collectBoardAttention", () => {
  it("indexes a flagged session by the task whose card should show it", () => {
    const board = collectBoardAttention([
      session({ id: "s1", task_id: "t1", status: "idle", attention_reason: "waiting_permission" }),
      // Working away with nothing owed to it: no badge on t2's card.
      session({ id: "s2", task_id: "t2", status: "running", attention_reason: null }),
    ])

    expect(board.byTask.get("t1")).toBe("waiting_permission")
    expect(board.byTask.has("t2")).toBe(false)
    expect(board.byGoal.size).toBe(0)
  })

  // A planner belongs to no task, so its goal's lane header is the only place
  // on the board it can ask for anything.
  it("indexes a session that has no task by its goal", () => {
    const board = collectBoardAttention([
      session({ id: "s1", goal_id: "g1", status: "idle", attention_reason: "waiting_input" }),
    ])

    expect(board.byGoal.get("g1")).toBe("waiting_input")
    expect(board.byTask.size).toBe(0)
  })

  // A card has room for one badge, and the reason the user has not seen yet is
  // the one raised last.
  it("shows the most recently raised reason when a task has several", () => {
    const board = collectBoardAttention([
      session({
        id: "s1",
        task_id: "t1",
        status: "failed",
        attention_reason: "disconnected",
        attention_since: "2026-08-16T10:00:00Z",
      }),
      session({
        id: "s2",
        task_id: "t1",
        status: "idle",
        attention_reason: "waiting_permission",
        attention_since: "2026-08-16T12:00:00Z",
      }),
    ])

    expect(board.byTask.get("t1")).toBe("waiting_permission")
  })

  it("reads a sessions list that never loaded", () => {
    const board = collectBoardAttention(undefined)

    expect(board.byTask.size).toBe(0)
    expect(board.byGoal.size).toBe(0)
  })
})

describe("useAttention", () => {
  /** The hook under the one provider it needs; nothing here has a router. */
  function renderAttention() {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    })
    return renderHook(() => useAttention(), {
      wrapper: ({ children }: { children: ReactNode }) =>
        createElement(QueryClientProvider, { client: queryClient }, children),
    })
  }

  /** Each of the three lists, by path; `fails` is the one that answers 500. */
  function stubDaemon({
    tasks = [] as TaskDto[],
    sessions = [] as SessionDto[],
    fails,
  }: {
    tasks?: TaskDto[]
    sessions?: SessionDto[]
    fails?: string | "all"
  } = {}) {
    daemonFetch.mockImplementation((input: Request | string | URL) => {
      const url = new URL(
        typeof input === "string" ? input : input instanceof URL ? input : input.url,
      )
      if (fails === "all" || url.pathname === fails) {
        return Promise.resolve(new Response("boom", { status: 500 }))
      }
      const body =
        url.pathname === "/v1/goals" ? [goal({})] : url.pathname === "/v1/tasks" ? tasks : sessions
      return Promise.resolve(jsonResponse(body))
    })
  }

  // The strip reads this to draw a placeholder instead of nothing: an empty
  // list that has not loaded is not the same claim as an empty list that has.
  it("says so while the lists have not answered", () => {
    const { result } = renderAttention()

    expect(result.current.isPending).toBe(true)
    expect(result.current.items).toEqual([])
    expect(result.current.error).toBeNull()
  })

  // Nothing answered and nothing to show: the failure is all there is to
  // report, and reporting it is what keeps a broken list from reading as a
  // quiet one.
  it("reports the failure when nothing loaded", async () => {
    stubDaemon({ fails: "all" })
    const { result } = renderAttention()

    await waitFor(() => expect(result.current.isPending).toBe(false))
    expect(result.current.error).not.toBeNull()
    expect(result.current.items).toEqual([])
    expect(result.current.partial).toBe(false)
  })

  // One list down, the others answered: what is on screen is a real list with
  // a hole in it, which is a different thing to say.
  it("calls a list that partly loaded partial", async () => {
    stubDaemon({ tasks: [task({ id: "t1", status: "failed" })], fails: "/v1/sessions" })
    const { result } = renderAttention()

    await waitFor(() => expect(result.current.items).toHaveLength(1))
    expect(result.current.error).not.toBeNull()
    expect(result.current.partial).toBe(true)
  })
})
